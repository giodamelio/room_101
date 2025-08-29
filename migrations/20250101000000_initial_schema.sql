-- Initial schema for Room 101 database

-- Identities table - stores cryptographic identities
CREATE TABLE identities (
    secret_key TEXT NOT NULL PRIMARY KEY -- hex encoded SecretKey
);

-- Peers table - stores known network peers
CREATE TABLE peers (
    node_id TEXT NOT NULL PRIMARY KEY, -- NodeId as string
    last_seen DATETIME, -- UTC datetime
    hostname TEXT
);

-- Events table - stores application events with JSON data
CREATE TABLE events (
    id TEXT NOT NULL PRIMARY KEY, -- UUID string
    event_type TEXT NOT NULL, -- JSON serialized EventType
    message TEXT NOT NULL,
    time DATETIME NOT NULL, -- UTC datetime
    data TEXT NOT NULL -- JSON data
);