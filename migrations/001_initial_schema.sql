DROP TABLE IF EXISTS shell_history CASCADE;
DROP TABLE IF EXISTS comments CASCADE;
DROP TABLE IF EXISTS logs CASCADE;
DROP TABLE IF EXISTS projects CASCADE;
DROP TABLE IF EXISTS users CASCADE;
DROP TABLE IF EXISTS sync_metadata CASCADE;
DROP TABLE IF EXISTS __migrations CASCADE;

CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS __migrations (
    filename TEXT PRIMARY KEY,
    applied_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS projects (
    id BIGINT PRIMARY KEY,
    title VARCHAR(255) NOT NULL,
    description TEXT,
    category VARCHAR(255),
    readme_link VARCHAR(500),
    demo_link VARCHAR(500),
    repo_link VARCHAR(500),
    slack_id VARCHAR(50) NOT NULL,
    username VARCHAR(255),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    last_synced TIMESTAMP WITH TIME ZONE,
    title_description_embedding vector(384)
);

CREATE TABLE IF NOT EXISTS users (
    slack_id VARCHAR(50) PRIMARY KEY,
    username VARCHAR(255),
    pfp_url VARCHAR(500) DEFAULT 'notfound',
    image_24 VARCHAR(500),
    image_32 VARCHAR(500),
    image_48 VARCHAR(500),
    image_72 VARCHAR(500),
    image_192 VARCHAR(500),
    image_512 VARCHAR(500),
    trust_level VARCHAR(50) DEFAULT 'unavailable',
    trust_value INTEGER DEFAULT 0,
    current_shells INTEGER,
    last_synced TIMESTAMP WITH TIME ZONE,
    stats_last_synced TIMESTAMP WITH TIME ZONE
);

CREATE TABLE IF NOT EXISTS logs (
    id BIGINT PRIMARY KEY,
    text TEXT NOT NULL,
    attachment VARCHAR(500),
    project_id BIGINT NOT NULL REFERENCES projects(id),
    slack_id VARCHAR(50) NOT NULL REFERENCES users(slack_id),
    username VARCHAR(255),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    last_synced TIMESTAMP WITH TIME ZONE,
    text_embedding vector(384)
);

CREATE TABLE IF NOT EXISTS comments (
    id BIGSERIAL PRIMARY KEY,
    text TEXT NOT NULL,
    devlog_id BIGINT NOT NULL REFERENCES logs(id),
    slack_id VARCHAR(50) NOT NULL REFERENCES users(slack_id),
    username VARCHAR(255),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    last_synced TIMESTAMP WITH TIME ZONE,
    text_embedding vector(384),
    UNIQUE(devlog_id, slack_id)
);

CREATE TABLE IF NOT EXISTS shell_history (
    id BIGSERIAL PRIMARY KEY,
    slack_id VARCHAR(50) NOT NULL REFERENCES users(slack_id),
    shells_then INTEGER,                    
    shell_diff INTEGER,
    shells INTEGER NOT NULL,
    recorded_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    UNIQUE(slack_id, recorded_at)
);

CREATE TABLE IF NOT EXISTS sync_metadata (
    key VARCHAR(50) PRIMARY KEY,
    last_sync TIMESTAMP WITH TIME ZONE,
    last_page INTEGER,
    status VARCHAR(20) DEFAULT 'pending'
);

CREATE INDEX IF NOT EXISTS idx_projects_slack_id ON projects(slack_id);
CREATE INDEX IF NOT EXISTS idx_logs_project_id ON logs(project_id);
CREATE INDEX IF NOT EXISTS idx_logs_slack_id ON logs(slack_id);
CREATE INDEX IF NOT EXISTS idx_comments_devlog_id ON comments(devlog_id);
CREATE INDEX IF NOT EXISTS idx_comments_slack_id ON comments(slack_id);
CREATE INDEX IF NOT EXISTS idx_shell_history_slack_id_recorded_at ON shell_history(slack_id, recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_comments_created_at ON comments(created_at);
CREATE INDEX IF NOT EXISTS idx_projects_created_at ON projects(created_at);
CREATE INDEX IF NOT EXISTS idx_projects_updated_at ON projects(updated_at);
CREATE INDEX IF NOT EXISTS idx_logs_created_at ON logs(created_at);
CREATE INDEX IF NOT EXISTS idx_projects_category ON projects(category) WHERE category IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_users_current_shells_desc ON users(current_shells DESC) WHERE current_shells > 0;

CREATE INDEX IF NOT EXISTS idx_projects_embedding ON projects 
USING ivfflat (title_description_embedding vector_cosine_ops) 
WITH (lists = 100)
WHERE title_description_embedding IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_comments_embedding ON comments 
USING ivfflat (text_embedding vector_cosine_ops) 
WITH (lists = 100)
WHERE text_embedding IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_logs_embedding ON logs 
USING ivfflat (text_embedding vector_cosine_ops) 
WITH (lists = 100)
WHERE text_embedding IS NOT NULL;