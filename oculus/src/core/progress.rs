use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::{Arc, OnceLock, RwLock};
use std::collections::HashMap;

static GLOBAL_PROGRESS_TREE: OnceLock<Arc<ProgressTree>> = OnceLock::new();

const DEFAULT_TEMPLATE: &str = "[{job_name}] [{bar:40.green/blue}] {pos}/{len} ({percent}%) {msg} ETA: {eta}";
const PROGRESS_CHARS: &str = "#>-";

pub struct ProgressTree {
    multi: MultiProgress,
    job_bars: RwLock<HashMap<String, Arc<JobProgressBar>>>,
}

impl ProgressTree {
    fn new() -> Self {
        let multi = MultiProgress::new();
        
        Self {
            multi,
            job_bars: RwLock::new(HashMap::new()),
        }
    }
    
    pub fn global() -> &'static Arc<ProgressTree> {
        GLOBAL_PROGRESS_TREE.get_or_init(|| Arc::new(ProgressTree::new()))
    }
    
    pub fn get_or_create_job_progress(&self, job_name: &str) -> Arc<JobProgressBar> {
        {
            let bars = self.job_bars.read().unwrap();
            if let Some(bar) = bars.get(job_name) {
                return Arc::clone(bar);
            }
        }
        
        let mut bars = self.job_bars.write().unwrap();
        
        if let Some(bar) = bars.get(job_name) {
            return Arc::clone(bar);
        }
        
        let progress_bar = self.create_progress_bar(job_name);
        progress_bar.set_message("Initializing...".to_string());
        
        let managed_bar = self.multi.add(progress_bar);
        let job_bar = Arc::new(JobProgressBar { 
            bar: managed_bar, 
            job_name: job_name.to_string() 
        });
        
        bars.insert(job_name.to_string(), Arc::clone(&job_bar));
        job_bar
    }
    
    pub fn add_job_progress(&self, job_name: &str, description: &str) -> JobProgressBar {
        let bar = self.create_progress_bar(job_name);
        bar.set_message(description.to_string());
        
        let managed_bar = self.multi.add(bar);
        JobProgressBar { bar: managed_bar, job_name: job_name.to_string() }
    }
    
    pub fn add_embedding_progress(&self, job_name: &str, item_type: &str) -> EmbeddingProgressBar {
        let bar = ProgressBar::new(100);
        bar.set_style(
            ProgressStyle::default_bar()
                .template(&format!("[{}] Embedding {} [{{bar:40.green/blue}}] {{pos}}/{{len}} ({{percent}}%) ETA: {{eta}}", job_name, item_type))
                .unwrap()
                .progress_chars(PROGRESS_CHARS)
        );
        
        let managed_bar = self.multi.add(bar);
        EmbeddingProgressBar { bar: managed_bar }
    }

    fn create_progress_bar(&self, job_name: &str) -> ProgressBar {
        let bar = ProgressBar::new(100);
        bar.set_style(
            ProgressStyle::default_bar()
                .template(&DEFAULT_TEMPLATE.replace("{job_name}", job_name))
                .unwrap()
                .progress_chars(PROGRESS_CHARS)
        );
        bar
    }
}

pub struct JobProgressBar {
    bar: ProgressBar,
    job_name: String,
}

impl JobProgressBar {
    pub fn init(&self, total: Option<usize>, unit: Option<&str>) {
        if let Some(total) = total {
            self.bar.set_length(total as u64);
        }
        if let Some(unit) = unit {
            self.bar.set_message(format!("Processing {}", unit));
        }
    }
    
    pub fn set(&self, position: usize) {
        self.bar.set_position(position as u64);
    }
    
    pub fn update_progress(&self, current: usize, total: usize, message: &str) {
        self.bar.set_length(total as u64);
        self.bar.set_position(current as u64);
        self.bar.set_message(message.to_string());
    }
    
    pub fn done(&self, message: String) {
        self.bar.set_message(format!("{} ✓", message));
        tracing::info!("[{}] {}", self.job_name, message);
    }
}

#[derive(Clone)]
pub struct EmbeddingProgressBar {
    bar: ProgressBar,
}

impl EmbeddingProgressBar {
    pub fn init(&self, total: usize) {
        self.bar.set_length(total as u64);
    }
    
    pub fn increment(&self) {
        self.bar.inc(1);
    }
    
    pub fn done(&self, message: String) {
        self.bar.finish_with_message(message);
    }
}


pub fn get_job_progress(job_name: &str) -> Arc<JobProgressBar> {
    ProgressTree::global().get_or_create_job_progress(job_name)
}


pub fn create_progress_with_job(job_name: &str, description: &str) -> JobProgressBar {
    ProgressTree::global().add_job_progress(job_name, description)
}

pub fn create_embedding_progress(job_name: &str, item_type: &str) -> EmbeddingProgressBar {
    ProgressTree::global().add_embedding_progress(job_name, item_type)
}


pub fn init_global_progress() -> &'static Arc<ProgressTree> {
    ProgressTree::global()
}


pub struct ProgressReporter {
    bar: JobProgressBar,
}

impl ProgressReporter {
    pub fn new_with_job(job_name: &str, description: &str) -> Self {
        Self {
            bar: create_progress_with_job(job_name, description),
        }
    }
    
    pub fn report(&self, current: usize, total: usize) {
        self.bar.init(Some(total), None);
        self.bar.set(current);
    }
    
    pub fn finish(&self) {
        self.bar.done("Completed".to_string());
    }
}