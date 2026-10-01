-- +goose Up
-- SQL in this section is executed when the migration is applied.

PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

-- Core Tables

CREATE TABLE IF NOT EXISTS content (
    id TEXT PRIMARY KEY,
    author_id TEXT NOT NULL,
    content_type TEXT NOT NULL CHECK(content_type IN ('post', 'comment', 'evidence')),
    content_text TEXT NOT NULL,
    media_urls TEXT,  -- JSON array
    parent_id TEXT,
    timestamp INTEGER NOT NULL,
    signature TEXT,
    hash TEXT,
    processing_status TEXT DEFAULT 'pending' CHECK(processing_status IN ('pending', 'processing', 'completed', 'failed')),
    last_updated INTEGER,
    FOREIGN KEY (author_id) REFERENCES users(id),
    FOREIGN KEY (parent_id) REFERENCES content(id)
);

CREATE TABLE IF NOT EXISTS content_metadata (
    content_id TEXT PRIMARY KEY,
    topics TEXT,  -- JSON array
    entities TEXT,  -- JSON array
    language TEXT,
    sentiment_score REAL,
    evidence_count INTEGER DEFAULT 0,
    context_references TEXT,  -- JSON array
    analysis_version TEXT,
    last_analyzed INTEGER,
    FOREIGN KEY (content_id) REFERENCES content(id)
);

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    public_key TEXT NOT NULL,
    username TEXT UNIQUE NOT NULL,
    created_at INTEGER NOT NULL,
    authenticity_score REAL DEFAULT 0.0,
    reputation_score REAL DEFAULT 0.0,
    last_active INTEGER,
    session_data TEXT  -- JSON for session info
);

-- Algorithm Tables

CREATE TABLE IF NOT EXISTS algorithm_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    algorithm_name TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    value REAL NOT NULL,
    timestamp INTEGER NOT NULL,
    metadata TEXT  -- JSON for additional metadata
);

CREATE TABLE IF NOT EXISTS algorithm_cache (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,  -- JSON serialized
    expiry INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    last_accessed INTEGER
);

CREATE TABLE IF NOT EXISTS algorithm_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    algorithm_name TEXT NOT NULL,
    error_type TEXT NOT NULL,
    error_message TEXT NOT NULL,
    context TEXT,  -- JSON serialized
    timestamp INTEGER NOT NULL,
    resolved INTEGER DEFAULT 0,
    resolution_notes TEXT
);

CREATE TABLE IF NOT EXISTS algorithm_feedback (
    id TEXT PRIMARY KEY,
    algorithm_type TEXT NOT NULL,
    user_id TEXT NOT NULL,
    feedback_type TEXT NOT NULL,
    feedback_data TEXT NOT NULL,  -- JSON serialized
    timestamp INTEGER NOT NULL,
    processed INTEGER DEFAULT 0,
    FOREIGN KEY (user_id) REFERENCES users(id)
);

-- Consensus Tables

CREATE TABLE IF NOT EXISTS truth_consensus (
    content_id TEXT NOT NULL,
    voter_id TEXT NOT NULL,
    vote_type TEXT NOT NULL CHECK(vote_type IN ('true', 'false', 'uncertain')),
    vote_weight REAL NOT NULL CHECK(vote_weight BETWEEN 0 AND 1),
    evidence_ids TEXT,  -- JSON array
    timestamp INTEGER NOT NULL,
    last_updated INTEGER,
    PRIMARY KEY (content_id, voter_id),
    FOREIGN KEY (content_id) REFERENCES content(id),
    FOREIGN KEY (voter_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS evidence_chain (
    content_id TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    submitter_id TEXT NOT NULL,
    evidence_type TEXT NOT NULL CHECK(evidence_type IN ('url', 'image', 'text')),
    evidence_text TEXT NOT NULL,
    quality_score REAL DEFAULT 0.0 CHECK(quality_score BETWEEN 0 AND 1),
    timestamp INTEGER NOT NULL,
    verification_status TEXT DEFAULT 'pending' CHECK(verification_status IN ('pending', 'verified', 'rejected')),
    PRIMARY KEY (content_id, evidence_id),
    FOREIGN KEY (content_id) REFERENCES content(id),
    FOREIGN KEY (submitter_id) REFERENCES users(id)
);

-- Indices

-- Content indices
CREATE INDEX IF NOT EXISTS idx_content_author ON content(author_id);
CREATE INDEX IF NOT EXISTS idx_content_parent ON content(parent_id);
CREATE INDEX IF NOT EXISTS idx_content_timestamp ON content(timestamp);
CREATE INDEX IF NOT EXISTS idx_content_status ON content(processing_status);

-- Content metadata indices
CREATE INDEX IF NOT EXISTS idx_content_metadata_topics ON content_metadata(topics);
CREATE INDEX IF NOT EXISTS idx_content_metadata_language ON content_metadata(language);
CREATE INDEX IF NOT EXISTS idx_content_metadata_analyzed ON content_metadata(last_analyzed);

-- User indices
CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_reputation ON users(reputation_score);
CREATE INDEX IF NOT EXISTS idx_users_active ON users(last_active);

-- Algorithm indices
CREATE INDEX IF NOT EXISTS idx_algorithm_metrics_name ON algorithm_metrics(algorithm_name);
CREATE INDEX IF NOT EXISTS idx_algorithm_metrics_timestamp ON algorithm_metrics(timestamp);
CREATE INDEX IF NOT EXISTS idx_algorithm_cache_expiry ON algorithm_cache(expiry);
CREATE INDEX IF NOT EXISTS idx_algorithm_errors_name ON algorithm_errors(algorithm_name);
CREATE INDEX IF NOT EXISTS idx_algorithm_errors_timestamp ON algorithm_errors(timestamp);
CREATE INDEX IF NOT EXISTS idx_algorithm_feedback_user ON algorithm_feedback(user_id);
CREATE INDEX IF NOT EXISTS idx_algorithm_feedback_type ON algorithm_feedback(algorithm_type);

-- Consensus indices
CREATE INDEX IF NOT EXISTS idx_truth_consensus_content ON truth_consensus(content_id);
CREATE INDEX IF NOT EXISTS idx_truth_consensus_voter ON truth_consensus(voter_id);
CREATE INDEX IF NOT EXISTS idx_truth_consensus_timestamp ON truth_consensus(timestamp);
CREATE INDEX IF NOT EXISTS idx_evidence_chain_content ON evidence_chain(content_id);
CREATE INDEX IF NOT EXISTS idx_evidence_chain_submitter ON evidence_chain(submitter_id);
CREATE INDEX IF NOT EXISTS idx_evidence_chain_status ON evidence_chain(verification_status);

-- +goose Down
-- SQL in this section is executed when the migration is rolled back.

DROP INDEX IF EXISTS idx_evidence_chain_status;
DROP INDEX IF EXISTS idx_evidence_chain_submitter;
DROP INDEX IF EXISTS idx_evidence_chain_content;
DROP INDEX IF EXISTS idx_truth_consensus_timestamp;
DROP INDEX IF EXISTS idx_truth_consensus_voter;
DROP INDEX IF EXISTS idx_truth_consensus_content;
DROP INDEX IF EXISTS idx_algorithm_feedback_type;
DROP INDEX IF EXISTS idx_algorithm_feedback_user;
DROP INDEX IF EXISTS idx_algorithm_errors_timestamp;
DROP INDEX IF EXISTS idx_algorithm_errors_name;
DROP INDEX IF EXISTS idx_algorithm_cache_expiry;
DROP INDEX IF EXISTS idx_algorithm_metrics_timestamp;
DROP INDEX IF EXISTS idx_algorithm_metrics_name;
DROP INDEX IF EXISTS idx_users_active;
DROP INDEX IF EXISTS idx_users_reputation;
DROP INDEX IF EXISTS idx_users_username;
DROP INDEX IF EXISTS idx_content_metadata_analyzed;
DROP INDEX IF EXISTS idx_content_metadata_language;
DROP INDEX IF EXISTS idx_content_metadata_topics;
DROP INDEX IF EXISTS idx_content_status;
DROP INDEX IF EXISTS idx_content_timestamp;
DROP INDEX IF EXISTS idx_content_parent;
DROP INDEX IF EXISTS idx_content_author;

DROP TABLE IF EXISTS evidence_chain;
DROP TABLE IF EXISTS truth_consensus;
DROP TABLE IF EXISTS algorithm_feedback;
DROP TABLE IF EXISTS algorithm_errors;
DROP TABLE IF EXISTS algorithm_cache;
DROP TABLE IF EXISTS algorithm_metrics;
DROP TABLE IF EXISTS content_metadata;
DROP TABLE IF EXISTS content;
DROP TABLE IF EXISTS users;
``` 