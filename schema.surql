-- Room 101 Database Schema

-- Peer
DEFINE TABLE IF NOT EXISTS peer SCHEMAFULL;

DEFINE FIELD IF NOT EXISTS node_id ON peer TYPE string;
DEFINE FIELD IF NOT EXISTS ticket ON peer TYPE string;
DEFINE FIELD IF NOT EXISTS hostname ON peer TYPE option<string>;
DEFINE FIELD IF NOT EXISTS last_seen ON peer TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS age_public_key ON peer TYPE option<string>;

-- Identity
DEFINE TABLE IF NOT EXISTS identity SCHEMAFULL;

DEFINE FIELD IF NOT EXISTS secret_key ON identity TYPE bytes;
DEFINE FIELD IF NOT EXISTS age_key ON identity TYPE string;

-- Audit Event
DEFINE TABLE IF NOT EXISTS audit_event SCHEMAFULL;

DEFINE FIELD IF NOT EXISTS event_type ON audit_event TYPE string;
DEFINE FIELD IF NOT EXISTS message ON audit_event TYPE string;
DEFINE FIELD IF NOT EXISTS data ON audit_event TYPE object;
DEFINE FIELD IF NOT EXISTS timestamp ON audit_event TYPE datetime;

-- Secret
DEFINE TABLE IF NOT EXISTS secret SCHEMAFULL;

DEFINE FIELD IF NOT EXISTS name ON secret TYPE string;
DEFINE FIELD IF NOT EXISTS node_id ON secret TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON secret TYPE datetime;
DEFINE FIELD IF NOT EXISTS data ON secret TYPE bytes;
