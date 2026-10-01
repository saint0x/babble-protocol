-- SQLite schema for Babel Protocol local backend state

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
    is_context BOOLEAN DEFAULT FALSE,  -- Flag to indicate if this is a context post
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
    authenticity_score REAL DEFAULT 0,
    reputation_score REAL DEFAULT 0,
    truth_accuracy REAL DEFAULT 0,
    evidence_quality REAL DEFAULT 0,
    engagement_quality REAL DEFAULT 0,
    community_score REAL DEFAULT 0,
    last_active INTEGER NOT NULL,
    session_data TEXT,
    preferences TEXT,
    stake_amount REAL DEFAULT 0,
    stake_locked_until INTEGER,
    domain_expertise JSON,
    verification_level INTEGER DEFAULT 0,
    total_contributions INTEGER DEFAULT 0
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
    evidence_type TEXT NOT NULL CHECK(evidence_type IN ('url', 'image', 'text', 'author_context')),
    evidence_text TEXT NOT NULL,
    references TEXT, -- JSON array of reference URLs/citations
    quality_score REAL DEFAULT 0.0 CHECK(quality_score BETWEEN 0 AND 1),
    context_score REAL DEFAULT 0.0 CHECK(context_score BETWEEN 0 AND 1),
    display_position TEXT DEFAULT 'standard' CHECK(display_position IN ('top', 'standard')),
    timestamp INTEGER NOT NULL,
    last_updated INTEGER,
    verification_status TEXT DEFAULT 'pending' CHECK(verification_status IN ('pending', 'verified', 'rejected')),
    metadata TEXT, -- JSON for additional metadata
    PRIMARY KEY (content_id, evidence_id),
    FOREIGN KEY (content_id) REFERENCES content(id),
    FOREIGN KEY (submitter_id) REFERENCES users(id)
);

-- User Activity History
CREATE TABLE IF NOT EXISTS user_activity (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    activity_type TEXT NOT NULL,
    target_id TEXT,
    timestamp INTEGER NOT NULL,
    impact_score REAL,
    metadata JSON,
    FOREIGN KEY (user_id) REFERENCES users(id)
);

-- User Relationships
CREATE TABLE IF NOT EXISTS user_relationships (
    follower_id TEXT NOT NULL,
    following_id TEXT NOT NULL,
    relationship_type TEXT NOT NULL,
    trust_score REAL DEFAULT 0,
    interaction_count INTEGER DEFAULT 0,
    last_interaction INTEGER,
    metadata JSON,
    PRIMARY KEY (follower_id, following_id),
    FOREIGN KEY (follower_id) REFERENCES users(id),
    FOREIGN KEY (following_id) REFERENCES users(id)
);

-- User Achievements
CREATE TABLE IF NOT EXISTS user_achievements (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    achievement_type TEXT NOT NULL,
    earned_at INTEGER NOT NULL,
    level INTEGER DEFAULT 1,
    progress REAL DEFAULT 0,
    metadata JSON,
    FOREIGN KEY (user_id) REFERENCES users(id)
);

-- Domain Expertise
CREATE TABLE IF NOT EXISTS domain_expertise (
    user_id TEXT NOT NULL,
    domain TEXT NOT NULL,
    expertise_score REAL DEFAULT 0,
    confidence_score REAL DEFAULT 0,
    last_updated INTEGER NOT NULL,
    verification_proofs JSON,
    PRIMARY KEY (user_id, domain),
    FOREIGN KEY (user_id) REFERENCES users(id)
);

-- User Verification History
CREATE TABLE IF NOT EXISTS user_verification (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    verification_type TEXT NOT NULL,
    status TEXT NOT NULL,
    verified_at INTEGER,
    verifier_id TEXT,
    proof_data JSON,
    metadata JSON,
    FOREIGN KEY (user_id) REFERENCES users(id),
    FOREIGN KEY (verifier_id) REFERENCES users(id)
);

-- Direct Messages
CREATE TABLE IF NOT EXISTS direct_messages (
    id TEXT PRIMARY KEY,
    sender_id TEXT NOT NULL,
    receiver_id TEXT NOT NULL,
    text TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    read_at INTEGER,
    FOREIGN KEY (sender_id) REFERENCES users(id),
    FOREIGN KEY (receiver_id) REFERENCES users(id)
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

-- User Activity indices
CREATE INDEX IF NOT EXISTS idx_user_activity_user_time ON user_activity(user_id, timestamp);

-- User Relationships indices
CREATE INDEX IF NOT EXISTS idx_user_relationships_follower ON user_relationships(follower_id);
CREATE INDEX IF NOT EXISTS idx_user_relationships_following ON user_relationships(following_id);

-- User Achievements indices
CREATE INDEX IF NOT EXISTS idx_user_achievements_user ON user_achievements(user_id);

-- Domain Expertise indices
CREATE INDEX IF NOT EXISTS idx_domain_expertise_score ON domain_expertise(expertise_score);

-- User Verification indices
CREATE INDEX IF NOT EXISTS idx_user_verification_status ON user_verification(user_id, status);

-- Add index for context posts
CREATE INDEX IF NOT EXISTS idx_content_context ON content(parent_id, is_context);

-- Direct Messages indices
CREATE INDEX IF NOT EXISTS idx_direct_messages_sender ON direct_messages(sender_id);
CREATE INDEX IF NOT EXISTS idx_direct_messages_receiver ON direct_messages(receiver_id);
CREATE INDEX IF NOT EXISTS idx_direct_messages_timestamp ON direct_messages(timestamp);

-- Triggers for automatic updates
CREATE TRIGGER IF NOT EXISTS update_user_last_active
AFTER INSERT ON user_activity
BEGIN
    UPDATE users 
    SET last_active = NEW.timestamp,
        total_contributions = total_contributions + 1
    WHERE id = NEW.user_id;
END;
``` 