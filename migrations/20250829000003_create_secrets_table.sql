-- Create secrets table for storing encrypted secrets with metadata
CREATE TABLE secrets (
    name TEXT NOT NULL,              -- filesystem-safe secret identifier
    encrypted_data BLOB NOT NULL,    -- Age-encrypted secret content
    hash TEXT NOT NULL,              -- SHA-256 hash of encrypted_data for version tracking
    target_node_id TEXT NOT NULL,    -- NodeId this secret is intended for
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (name, target_node_id)  -- allows multiple copies per secret for different targets
);

-- Index for efficient querying by target node
CREATE INDEX idx_secrets_target_node_id ON secrets(target_node_id);

-- Index for efficient querying by hash for deduplication
CREATE INDEX idx_secrets_hash ON secrets(hash);
