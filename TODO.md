# TODO

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
- SQLite database with SQLx for compile-time checked queries
- Iroh for P2P networking and gossip protocol
- Age encryption for secure data handling
- Poem web framework for optional HTTP interface
