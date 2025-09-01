# TODO

## MAJOR REFACTOR: Ractor Actor Architecture
**Status**: Planning Phase
**Goal**: Refactor entire application to use Ractor actors with simple supervision

### Architecture Overview
Current architecture has complex channel topology (7 tasks, multiple tokio mpsc channels).
New architecture: Ractor actors with simple supervisor pattern - whole app dies if any actor dies.

Each actor owns a specific domain and communicates via typed messages. Simple fail-fast supervision.

### Core Actor System

#### Supervisor Pattern (`src/main.rs`)
Simple supervisor actor that starts all child actors with `start_linked()`. Uses ractor::registry for actor discovery.
```rust
use ractor::registry;

pub struct SupervisorActor;

impl Actor for SupervisorActor {
    type Msg = SupervisorMessage;
    type State = ();
    type Arguments = AppConfig;

    async fn pre_start(&self, myself: ActorRef<Self::Msg>, config: AppConfig) -> Result<Self::State> {
        // Start all child actors with start_linked() - if any die, supervisor dies
        let (_gossip_actor, _gossip_handle) = Actor::spawn_linked(
            Some("gossip".into()),
            GossipActor,
            config.gossip_config,
            myself.clone()
        ).await?;

        let (_systemd_actor, _systemd_handle) = Actor::spawn_linked(
            Some("systemd".into()),
            SystemdActor,
            config.systemd_config,
            myself.clone()
        ).await?;

        if config.enable_webserver {
            let (_webserver_actor, _webserver_handle) = Actor::spawn_linked(
                Some("webserver".into()),
                WebServerActor,
                config.webserver_config,
                myself.clone()
            ).await?;
        }

        Ok(())
    }
}
```

#### Actor Message Pattern
Each actor defines its own message enum within its module (following WebServerActor pattern):
```rust
// In src/actors/gossip/mod.rs
#[derive(Debug)]
pub enum GossipMessage {
    SendPeerMessage(PeerMessage),
    PeerConnected(NodeId),
    PeerDisconnected(NodeId),
}

// In src/actors/systemd.rs
#[derive(Debug)]
pub enum SystemdMessage {
    SyncSecret { name: String, content: Vec<u8> },
    SyncAllSecrets,
    RemoveSecret { name: String },
}

// In src/actors/webserver.rs (already exists)
#[derive(Debug)]
pub enum WebServerMessage {}
```

#### Actor Discovery Pattern
Actors use ractor::registry to find other actors by name:
```rust
use ractor::registry;

// Example: GossipActor sending message to SystemdActor
let systemd_actor: ActorRef<SystemdMessage> = registry::where_is("systemd".to_string())
    .expect("SystemdActor not found")
    .into();
systemd_actor.cast(SystemdMessage::SyncSecret { name, content })?;

// Example: WebServerActor sending message to GossipActor
let gossip_actor: ActorRef<GossipMessage> = registry::where_is("gossip".to_string())
    .expect("GossipActor not found")
    .into();
gossip_actor.cast(GossipMessage::SendPeerMessage(peer_message))?;
```

#### Actor Organization Structure
```
src/
├── main.rs              # SupervisorActor & app entry point
├── actors/
│   ├── mod.rs          # Actor registry and common utilities
│   ├── gossip/
│   │   ├── mod.rs      # Main GossipActor (replaces network.rs)
│   │   ├── listener.rs # Gossip message listener component
│   │   ├── sender.rs   # Gossip message sender component
│   │   └── heartbeat.rs # Heartbeat generation component
│   ├── systemd.rs      # SystemdActor for credential management
│   └── webserver.rs    # WebServerActor (already exists)
├── network/
│   └── protocol.rs     # PeerMessage, SignedMessage (moved from network.rs)
├── db.rs               # Database models (unchanged)
└── ... (other existing files)
```

### Implementation Plan

#### Phase 1: Infrastructure Setup
- [ ] Create `src/actors/gossip/` directory structure
- [ ] Create `src/network/protocol.rs` and move PeerMessage + SignedMessage from network.rs
- [ ] Create SupervisorActor in main.rs with simple start_linked() pattern
- [ ] Update `src/actors/mod.rs` to include gossip and systemd modules

#### Phase 2: GossipActor Migration
Migrate network.rs functionality into GossipActor:
- [ ] Create `src/actors/gossip/mod.rs` - Main GossipActor that replaces network_manager_task
- [ ] Create `src/actors/gossip/listener.rs` - Component for peer_message_listener_task logic
- [ ] Create `src/actors/gossip/sender.rs` - Component for peer_message_sender_task logic
- [ ] Create `src/actors/gossip/heartbeat.rs` - Component for peer_message_heartbeat logic

#### Phase 2a: GossipActor Details
**GossipActor** (`gossip/mod.rs`):
- Setup iroh endpoint and gossip protocol in pre_start()
- Coordinate child components (listener, sender, heartbeat)
- Handle GossipMessage variants
- Call db.rs functions directly for database operations
- Communicate with SystemdActor via ActorRegistry for credential sync
- Graceful shutdown in post_stop()

**Components**:
- **Listener**: Receive PeerMessage from gossip, call db.rs directly, send SystemdMessage as needed
- **Sender**: Send PeerMessage via gossip when requested by other actors
- **Heartbeat**: Generate periodic heartbeat messages every 10 seconds

#### Phase 3: SystemdActor
- [ ] Create `src/actors/systemd.rs` - SystemdActor for systemd credential management
  - Handle SyncSecret, SyncAllSecrets, RemoveSecret messages
  - Encapsulate all systemd-creds operations
  - Move sync_all_secrets_to_systemd() logic from network.rs
  - Define SystemdMessage enum within the module

#### Phase 4: Actor Integration
- [ ] Update WebServerActor to use ractor::registry for actor discovery
- [ ] Implement actor-to-actor communication via ActorRef.cast()
- [ ] Remove all mpsc channel infrastructure from network.rs
- [ ] Update main.rs to use SupervisorActor instead of manual task spawning

#### Phase 5: Testing & Cleanup
- [ ] Remove old network.rs file after migration complete
- [ ] Test all functionality: peer discovery, secret sync, web interface
- [ ] Verify graceful shutdown works with actor system
- [ ] Run `cargo check` and ensure no compilation errors

### Benefits of Ractor Architecture
1. **Type-Safe Messaging**: Actors communicate via typed message enums
2. **Simple Supervision**: Linked actors ensure fail-fast behavior
3. **Clean Lifecycle**: Actors have proper startup/shutdown hooks
4. **Isolation**: Each actor owns its domain, no shared state
5. **Testable**: Actors can be tested in isolation with mock messages
6. **Maintainable**: Clear separation of concerns vs monolithic network.rs

### Migration Notes
- Database models (`db.rs`) remain unchanged
- Move/refactor PeerMessage and SignedMessage to `src/network/protocol.rs` (can modify as needed)
- Move systemd functionality from network.rs to SystemdActor
- Actors communicate via `ActorRef.cast()` instead of channels
- Simple fail-fast: if any actor dies, whole app exits cleanly
- Network protocol can be modified freely - no compatibility requirements

### SystemdActor Integration
The SystemdActor will handle all systemd-creds operations:
- Receive SyncSecret messages when secrets are created/updated
- Handle SyncAllSecrets for bulk operations (manual sync requests)
- Handle RemoveSecret when secrets are deleted
- Encapsulate systemd_secrets module functionality
- GossipActor and WebServerActor send messages to SystemdActor instead of calling sync functions directly

### Actor Communication Pattern
Actors use ractor::registry::where_is() to find other actors by name:
- GossipActor calls db.rs directly for database operations
- GossipActor uses registry to find SystemdActor and sends SystemdMessage for credential sync
- WebServerActor uses registry to find GossipActor and sends GossipMessage for network operations
- WebServerActor uses registry to find SystemdActor and sends SystemdMessage for sync requests
- No centralized message definitions - each actor owns its message enum
- Registry automatically manages actor lifecycle (removed on shutdown)

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
