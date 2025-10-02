# TODO

## Current Tasks

No current tasks in progress.

## Completed

### Add Custom DateTime Serialization to All Models
**Completed**: 2025-10-02

Successfully implemented custom serde modules for chrono DateTime types to ensure proper serialization with SurrealDB.

**Changes Made**:

1. **Added `optional_chrono_datetime_as_sql` module** (`src/custom_serde.rs:102-124`):
   - Handles `Option<DateTime<Utc>>` serialization/deserialization
   - Uses `default` and `skip_serializing_if` attributes for proper optional field handling

2. **Updated `Peer` model** (`src/db/peer.rs:17-22`):
   - Applied `optional_chrono_datetime_as_sql` to `last_seen` field with proper attributes
   - Updated `bump_last_seen` helper struct to use custom serde

3. **Updated `Secret` model** (`src/db/secret.rs:17-20`):
   - Applied `chrono_datetime_as_sql` to `created_at` field
   - Applied `node_id_serde` to `node_id` field

4. **Fixed peer auto-discovery** (`src/actors/gossip/gossip_receiver.rs:140-142`):
   - Added `Peer::insert_from_node_id()` on `NeighborUp` event
   - Peers are now automatically added to database when discovered via gossip network
   - Made `bump_last_seen` fail silently (best-effort) to handle edge cases

**Results**:
- All models use portable `chrono::DateTime<Utc>` types
- Proper serialization to/from SurrealDB's datetime format
- No lock-in to SurrealDB-specific types in domain models
- Peer discovery now works automatically via gossip protocol

## Features to Add

### High Priority
- [ ] **Secret Versions**: Update schema to support multiple versions of the same secret
  - Store version history in database
  - Use database views to always fetch latest version
  - Update UI to show version information

### Medium Priority
- [ ] **System Info Broadcasting**: Allow nodes to broadcast system information
  - Disk usage for each disk
  - Network interfaces and IP addresses
  - System resource utilization

- [ ] **UI Improvements**:
  - Add navbar along top to switch between pages
  - Add status bar showing active peer count and secret count
  - Improve overall navigation and user experience

### Low Priority
- [ ] **Logging Cleanup**:
  - Move peer discovery logging to debug/trace levels
  - Ensure all log messages are professional (no emojis)
  - Optimize log verbosity for production use

## Code Quality & Maintenance
- [ ] **Performance Optimization**: Review and optimize database queries
- [ ] **Error Handling**: Audit error handling throughout codebase
- [ ] **Documentation**: Add inline documentation for complex functions
- [ ] **Testing**: Add unit tests for core functionality

## Architecture Notes
- Currently uses Ractor actor system with supervisor pattern
- Iroh for P2P networking and gossip protocol
- Age encryption for secure data handling
