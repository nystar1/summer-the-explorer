use std::{
    sync::Arc,
    collections::HashMap,
    time::{Duration, Instant},
};

use ort::{
    execution_providers::CoreMLExecutionProvider,
    session::{Session, builder::GraphOptimizationLevel},
};
use ndarray::{Array1, Array2, ArrayViewD, IxDyn};
use futures::stream::{FuturesUnordered, StreamExt};
use parking_lot::Mutex;// just faster!
use tokenizers::Tokenizer;
use tokio::sync::Semaphore;
use tracing::{info, instrument};

use crate::utils::error::{ApiError, Result};

// self containment, baby!

static MODEL_BYTES: &[u8] = include_bytes!("../../../minilm-build/model.onnx");
static TOKENIZER_JSON: &str = include_str!("../../../minilm-build/tokenizer.json");

const MAX_MODEL_INPUT_LENGTH: usize = 512;
const EMBEDDING_DIM: usize = 384;
const OVERLAP: usize = 64;

pub struct EmbeddingModel {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl EmbeddingModel {
    pub fn new() -> Result<Self> {
        let session_builder = {
            let builder = Session::builder()?
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .with_memory_pattern(true)?;

            match builder
                .clone()
                .with_execution_providers([CoreMLExecutionProvider::default().build()])
            {
                Ok(b) => {
                    info!("Using CoreML execution provider");
                    b
                }
                Err(e) => {
                    info!(
                        "CoreML execution provider not available: {}, falling back to CPU",
                        e
                    );
                    builder
                }
            }
        };

        let session = Mutex::new(session_builder.commit_from_memory(MODEL_BYTES)?);

        let tokenizer = Tokenizer::from_bytes(TOKENIZER_JSON.as_bytes())
            .map_err(|e| ApiError::Embedding(format!("Failed to load tokenizer: {e}")))?;

        Ok(Self { session, tokenizer })
    }

    #[allow(clippy::significant_drop_tightening)]
    pub fn forward(&self, input_ids: Vec<i64>, attention_mask: Vec<i64>) -> Result<Vec<f32>> {
        let input_ids_array = Array2::from_shape_vec((1, MAX_MODEL_INPUT_LENGTH), input_ids)?;
        let attention_mask_array =
            Array2::from_shape_vec((1, MAX_MODEL_INPUT_LENGTH), attention_mask.clone())?;

        let mut session = self.session.lock();
        let outputs = session.run(ort::inputs! {
            "input_ids" => ort::value::TensorRef::from_array_view(&input_ids_array)?,
            "attention_mask" => ort::value::TensorRef::from_array_view(&attention_mask_array)?
        })?;

        let (shape, data) = if outputs.contains_key("last_hidden_state") {
            outputs["last_hidden_state"].try_extract_tensor::<f32>()
        } else {
            outputs[0].try_extract_tensor::<f32>()
        }?;

        let shape_usize: Vec<usize> = shape.iter().map(|&d| {
            usize::try_from(d).expect("Shape dimension should fit in usize")
        }).collect();
        let output = ArrayViewD::from_shape(IxDyn(&shape_usize), data)
            .map_err(|e| ApiError::Embedding(format!("Failed to create ndarray view: {e}")))?;

        let attention_mask_u32: Vec<u32> = attention_mask.into_iter().map(|x| {
            u32::try_from(x).expect("Attention mask value should fit in u32")
        }).collect();
        let shape = output.shape();
        let (_batch_size, seq_len, hidden_size) = (shape[0], shape[1], shape[2]);

        let mut pooled = Array1::zeros(hidden_size);
        let mut total_mask = 0.0;

        for (i, &mask_value) in attention_mask_u32.iter().take(seq_len).enumerate() {
            #[allow(clippy::cast_precision_loss)] // it's ok lol
            let mask_f32 = mask_value as f32;
            total_mask += mask_f32;

            for j in 0..hidden_size {
                pooled[j] += output[[0, i, j]] * mask_f32;
            }
        }

        if total_mask > 0.0 {
            pooled /= total_mask;
        }

        let norm = pooled.dot(&pooled).sqrt();
        let result = if norm > 1e-6 {
            (pooled / norm).to_vec()
        } else {
            pooled.to_vec()
        };
        Ok(result)
    }

}

#[derive(Clone, Hash, PartialEq, Eq)]
struct CacheKey(String);

#[derive(Clone)]
struct CacheEntry {
    embedding: Vec<f32>,
    created_at: Instant,
}

pub struct EmbeddingService {
    model: Arc<EmbeddingModel>,
    semaphore: Arc<Semaphore>,
    cache: Arc<Mutex<HashMap<CacheKey, CacheEntry>>>,
    cache_ttl: Duration,
}

impl EmbeddingService {
    pub fn new(force_regenerate: bool) -> Result<Self> {
        let cpu_count = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let env_override = std::env::var("EMBED_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&v| v > 0);

        let max_concurrent = env_override.unwrap_or(cpu_count);

        let model = EmbeddingModel::new()?;

        let cache_ttl = if force_regenerate {
            Duration::from_secs(0)
        } else {
            Duration::from_secs(3600)
        };

        info!(
            "Embedding service initialized with {} concurrent slots (CPU count: {}). Cache enabled: {}.",
            max_concurrent, cpu_count, !force_regenerate
        );

        Ok(Self {
            model: Arc::new(model),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            cache: Arc::new(Mutex::new(HashMap::with_capacity(1000))),
            cache_ttl,
        })
    }

    #[instrument(skip(self, sentences))]
    pub async fn embed_batch(&self, sentences: Vec<String>) -> Result<Vec<Vec<f32>>> {        
        if sentences.is_empty() {
            return Ok(Vec::new());
        }

        let mut futures = FuturesUnordered::new();
        for (idx, sentence) in sentences.into_iter().enumerate() {
            let future = async move {
                let embedding = self.embed_text(&sentence).await?;
                crate::utils::error::Result::Ok((idx, embedding))
            };
            futures.push(future);
        }

        let mut results = Vec::new();
        while let Some(result) = futures.next().await {
            results.push(result?);
        }

        
        results.sort_by_key(|(idx, _)| *idx);
        Ok(results.into_iter().map(|(_, embedding)| embedding).collect())
    }

    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        if text.trim().is_empty() {
            return Ok(vec![0.0; EMBEDDING_DIM]);
        }

        let encoding = self
            .model
            .tokenizer
            .encode(text, false)
            .map_err(|e| ApiError::Embedding(format!("Tokenization failed: {e}")))?;

        if encoding.get_ids().len() < 8 {
            return Ok(vec![0.0; EMBEDDING_DIM]);
        }

        let cache_key = CacheKey(text.to_string());
        {
            let cache = self.cache.lock();
            if let Some(cached_entry) = cache.get(&cache_key)
                .filter(|entry| entry.created_at.elapsed() < self.cache_ttl) {
                return Ok(cached_entry.embedding.clone());
            }
        }

        let embedding = self.embed_single_text(text).await?;
        {
            let mut cache = self.cache.lock();

            if cache.len() > 1000 {
                let now = Instant::now();
                cache.retain(|_, entry| now.duration_since(entry.created_at) < self.cache_ttl);
            }

            cache.insert(
                cache_key,
                CacheEntry {
                    embedding: embedding.clone(),
                    created_at: Instant::now(),
                },
            );
        }

        Ok(embedding)
    }

    async fn embed_single_text(&self, text: &str) -> Result<Vec<f32>> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| ApiError::Embedding("Failed to acquire semaphore".to_owned()))?;

        let model = Arc::clone(&self.model);
        let text = text.to_string();

        tokio::task::spawn_blocking(move || -> Result<Vec<f32>> {
            let encoding = model
                .tokenizer
                .encode(text, true)
                .map_err(|e| ApiError::Embedding(format!("Tokenization failed: {e}")))?;

            let input_ids = encoding.get_ids();
            let attention_mask = encoding.get_attention_mask();

            if input_ids.len() > MAX_MODEL_INPUT_LENGTH {
                let step = MAX_MODEL_INPUT_LENGTH - OVERLAP;
                let num_windows = if input_ids.len() <= step {
                    1
                } else {
                    (input_ids.len() - MAX_MODEL_INPUT_LENGTH).div_ceil(step) + 1
                };

                let mut embeddings = Vec::with_capacity(num_windows);
                let mut pos = 0;

                while pos < input_ids.len() {
                    let end = (pos + MAX_MODEL_INPUT_LENGTH).min(input_ids.len());
                    let window_input_ids = &input_ids[pos..end];
                    let window_attention_mask = &attention_mask[pos..end];

                    let mut padded_input_ids = window_input_ids.to_vec();
                    padded_input_ids.resize(MAX_MODEL_INPUT_LENGTH, 0);
                    let mut padded_attention_mask = window_attention_mask.to_vec();
                    padded_attention_mask.resize(MAX_MODEL_INPUT_LENGTH, 0);

                    let input_ids_i64: Vec<i64> = padded_input_ids.iter().map(|&x| i64::from(x)).collect();
                    let attention_mask_i64: Vec<i64> =
                        padded_attention_mask.iter().map(|&x| i64::from(x)).collect();

                    let embedding = model.forward(input_ids_i64, attention_mask_i64)?;
                    embeddings.push(embedding);

                    if end >= input_ids.len() {
                        break;
                    }
                    pos += MAX_MODEL_INPUT_LENGTH - OVERLAP;
                }

                let embedding_len = embeddings[0].len();
                let mut averaged = vec![0.0; embedding_len];

                for embedding in &embeddings {
                    for (i, &val) in embedding.iter().enumerate() {
                        averaged[i] += val;
                    }
                }

                #[allow(clippy::cast_precision_loss)] // SAFETY: we ain losing any data
                let count = embeddings.len() as f32;
                for val in &mut averaged {
                    *val /= count;
                }

                let norm = averaged.iter().map(|&x| x * x).sum::<f32>().sqrt();
                if norm > 1e-6 {
                    for val in &mut averaged {
                        *val /= norm;
                    }
                }

                return Ok(averaged);
            }

            let mut padded_input_ids = input_ids.to_vec();
            padded_input_ids.resize(MAX_MODEL_INPUT_LENGTH, 0);
            let mut padded_attention_mask = attention_mask.to_vec();
            padded_attention_mask.resize(MAX_MODEL_INPUT_LENGTH, 0);

            let input_ids_i64: Vec<i64> = padded_input_ids.iter().map(|&x| i64::from(x)).collect();
            let attention_mask_i64: Vec<i64> =
                padded_attention_mask.iter().map(|&x| i64::from(x)).collect();

            model.forward(input_ids_i64, attention_mask_i64)
        })
        .await
        .map_err(|e| ApiError::Embedding(format!("Task join error: {e}")))?
    }

}

