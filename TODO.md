# TODO

## MAJOR REFACTOR: Task-Based Broadcast Architecture
**Status**: Planning Phase
**Goal**: Refactor entire application to use tokio::sync::broadcast channel with task-based architecture

### Architecture Overview
Current architecture has complex channel topology (7 tasks, multiple tokio mpsc channels).
New architecture: Single tokio::sync::broadcast channel + task-per-file organization.

Each task subscribes to the broadcast channel and receives ALL messages, then filters and processes only the messages it cares about.

### Core Components

#### Central Message System (`src/messages.rs`)
All messages are broadcast to every task via `tokio::sync::broadcast`. Each task pattern matches on `AppMessage` variants to decide what to process.
```rust
#[derive(Debug, Clone)]
pub enum AppMessage {
    // System control
    Shutdown,
    TaskStarted { task_name: String },
    TaskError { task_name: String, error: String },

    // Network events (from peers)
    PeerJoined { node_id: NodeId, time: DateTime<Utc>, hostname: Option<String>, age_public_key: String },
    PeerLeft { node_id: NodeId, time: DateTime<Utc> },
    PeerIntroduction { node_id: NodeId, time: DateTime<Utc>, hostname: Option<String>, age_public_key: String },
    PeerHeartbeat { node_id: NodeId, time: DateTime<Utc>, age_public_key: String },

    // Secret management
    SecretReceived { name: String, encrypted_data: Vec<u8>, hash: String, target_node_id: NodeId, time: DateTime<Utc> },
    SecretDeleteReceived { name: String, hash: String, target_node_id: NodeId, time: DateTime<Utc> },
    SecretSyncRequest { target_node_id: NodeId },

    // Internal commands
    BroadcastMessage { message: PeerMessage },
    SendSecretToNode { secret: Secret, target_node_id: NodeId },
    SyncSecretsToSystemd,

    // Web interface commands
    CreateSecret { name: String, content: Vec<u8>, target_node_id: NodeId },
    DeleteSecret { name: String, hash: String, target_node_id: NodeId },
    ShareSecret { name: String, hash: String, target_node_id: NodeId },
}
```

#### File Organization Structure
```
src/
├── main.rs              # App entry point & task orchestration
├── messages.rs          # Central AppMessage enum
├── tasks/
│   ├── mod.rs          # Task registry and common utilities
│   ├── network/
│   │   ├── mod.rs      # Network task coordination
│   │   ├── gossip.rs   # Gossip protocol setup & management
│   │   ├── listener.rs # Message reception from peers
│   │   ├── sender.rs   # Message transmission to peers
│   │   └── heartbeat.rs # Periodic heartbeat generation
│   ├── secrets/
│   │   ├── mod.rs      # Secret management coordination
│   │   ├── manager.rs  # Secret creation/deletion/sync
│   │   └── systemd.rs  # SystemD credentials integration
│   ├── webserver.rs    # HTTP server task
│   └── database.rs     # Database operations task
├── db.rs               # Database models (unchanged)
├── network/            # Network utilities (keep PeerMessage)
│   └── protocol.rs     # Keep existing PeerMessage & SignedMessage
└── ... (other existing files)
```

#### Task Trait (`src/tasks/mod.rs`)
```rust
#[async_trait]
pub trait Task: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    async fn run(&self, mut rx: tokio::sync::broadcast::Receiver<AppMessage>, tx: tokio::sync::broadcast::Sender<AppMessage>) -> Result<()>;
}

// Each task implementation pattern:
// while let Ok(msg) = rx.recv().await {
//     match msg {
//         AppMessage::RelevantVariant { .. } => {
//             // Process this message
//         }
//         _ => {
//             // Ignore other messages
//         }
//     }
// }
```

### Implementation Plan

#### Phase 1: Infrastructure Setup
- [ ] Create `src/messages.rs` with complete AppMessage enum
- [ ] Create `src/tasks/mod.rs` with Task trait and utilities
- [ ] Update `src/main.rs` to use tokio::sync::broadcast channel instead of multiple channels
- [ ] Create directory structure: `src/tasks/network/`, `src/tasks/secrets/`
- [ ] Set up broadcast channel with appropriate buffer size (e.g., 1000)

#### Phase 2: Task Migration (Network)
Current network.rs has 5 tasks that need to be separated:
- [ ] Create `src/tasks/network/gossip.rs` - migrate gossip_setup_task logic
- [ ] Create `src/tasks/network/listener.rs` - migrate peer_message_listener_task logic
- [ ] Create `src/tasks/network/sender.rs` - migrate peer_message_sender_task logic
- [ ] Create `src/tasks/network/heartbeat.rs` - migrate heartbeat generation logic
- [ ] Create `src/tasks/network/mod.rs` - coordinate network tasks

#### Phase 2a: Network Task Details
**GossipTask** (`gossip.rs`):
- Setup iroh endpoint and gossip protocol
- Handle bootstrap node connections
- Send AppMessage::TaskStarted when ready
- Convert gossip events to AppMessages
- Subscribe to broadcast channel, ignore irrelevant messages

**NetworkListenerTask** (`listener.rs`):
- Receive PeerMessage from gossip
- Convert to appropriate AppMessage variants (PeerJoined, SecretReceived, etc.)
- Handle message verification and deserialization
- Subscribe to broadcast channel, ignore non-network messages

**NetworkSenderTask** (`sender.rs`):
- Subscribe to broadcast channel, filter for AppMessage::BroadcastMessage
- Convert AppMessages to PeerMessage and send via gossip
- Handle message signing and serialization
- Ignore all other message types

**HeartbeatTask** (`heartbeat.rs`):
- Generate periodic AppMessage::PeerHeartbeat via broadcast sender
- Include current node's age_public_key
- 10-second interval (configurable)
- Subscribe to broadcast channel for shutdown messages

#### Phase 3: Task Migration (Secrets & Web)
- [ ] Create `src/tasks/secrets/manager.rs` - subscribe to broadcast, filter for CreateSecret, DeleteSecret, ShareSecret AppMessages
- [ ] Create `src/tasks/secrets/systemd.rs` - subscribe to broadcast, filter for SyncSecretsToSystemd AppMessage
- [ ] Create `src/tasks/webserver.rs` - migrate webserver_task, convert HTTP requests to AppMessages, ignore non-web messages
- [ ] Create `src/tasks/database.rs` - centralize all DB operations, subscribe to broadcast for relevant messages (optional)

#### Phase 4: Testing & Cleanup
- [ ] Remove old tokio::sync::mpsc channel infrastructure from network.rs
- [ ] Remove network_manager_task function
- [ ] Test all functionality: peer discovery, secret sync, web interface
- [ ] Update error handling to use AppMessage::TaskError consistently
- [ ] Add message tracing/logging capabilities to broadcast channel
- [ ] Handle broadcast channel lag/overflow scenarios gracefully

### Benefits of New Architecture
1. **True Broadcast**: Every task receives every message, can react to any system event
2. **Simplified Communication**: Single broadcast channel eliminates complex channel topology
3. **Better Debugging**: All messages flow through central point (can add logging/tracing)
4. **Modular Design**: Each task in separate file, easy to understand and maintain
5. **Self-Filtering**: Tasks decide what messages to process, making system behavior clear
6. **Extensible**: Easy to add new tasks that can react to any existing message type
7. **Testable**: Tasks can be tested in isolation with mock broadcast channels
8. **Error Handling**: Uniform error propagation via AppMessage::TaskError

### Migration Notes
- Keep existing `PeerMessage` enum for network protocol compatibility
- Database models (`db.rs`) remain unchanged
- SystemD integration logic moves to dedicated task
- Web routes convert HTTP requests to AppMessages instead of direct DB calls

### Future Enhancements Enabled
- CLI task for secret management without web interface
- Metrics collection task for monitoring
- System info broadcast task for peer monitoring
- Message replay/audit logging task
- Secret versioning task with conflict resolution

---

## Existing Features (Pre-Refactor)

 - [x] Allow a node to announce it is deleting one of it's secrets by sending a signed message. Don't allow a node to send a delete for messages that belong to any other node
 - [x] Allow the WebUI to "share" a secret that that node owns to other nodes, but creating copies of it.
 - [x] Add copy buttons for places that node ids and hashs are displayed
 - [x] Add button to peer to view all secrets for that peer (using a new filter queryparam on the list page)
 - [x] Create comprehensive node details page showing all peer info and embedded secrets list:
   - Add `/peers/:node_id` route and handler
   - Create `tmpl_peer_detail()` template with peer metadata, connection info, and embedded secrets
   - Make node IDs in peer list clickable links to detail page
   - Keep existing "View Secrets" button as quick-access option
 - [x] Sync secrets that are for the current node into systemd encrypted secrets. Only do it when the secret is updated.
   - [x] Created systemd_secrets module with flexible wrapper around systemd-creds
   - [x] Added command line flags: --systemd-secrets-path, --systemd-user-scope
   - [x] Auto-sync on Secret::create() and Secret::upsert()
   - [x] Added SecretSyncRequest message type for manual sync
   - [x] Added web UI button to trigger sync requests
   - [ ] **BUG**: Remote nodes not writing received secrets to systemd - needs debugging
   - [ ] Clicking the request systemd sync for another node just "method not allowed"
 - [ ] **DEBUG SYSTEMD REMOTE SYNC ISSUE**:
   - [ ] Add detailed logging to PeerMessage::Secret handler in network.rs (line ~671)
   - [ ] Add logging to verify Secret::upsert() systemd sync path is executed
   - [ ] Add logging to verify target_node_id comparison works correctly
   - [ ] Add logging to verify decrypt_secret_for_identity() succeeds for received secrets
   - [ ] Test with two instances: create secret on Node A for Node B, verify Node B writes to systemd
   - [ ] Check systemd-creds command execution and permission issues
   - [ ] Use new sync request button to manually trigger and verify sync works
 - [ ] Add support for secret versions. Update schema to handle storing multiple versions of the same secret and making sure to actually use the last one. Maybe something we can use DB views for.
 - [ ] Allow systems to broadcast some system info about themself. Just to make things easier to manage. Things like the disk usage for each disk and all the network interfaces and their IP addresses
 - [ ] Make peer discovery logging quieter - move most to debug level, some to trace
 - [ ] Remove all emoji from logging messages
 - [x] Add no-emoji rule to CLAUDE.md for future development
