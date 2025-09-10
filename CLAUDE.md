# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Build and Check
- `cargo check` - Check code for errors without building (fast)
- `cargo build` - Build the project
- `cargo test` or `cargo nextest run` - Run tests (nextest is preferred)
- `treefmt` - Format all code files
- `pre-commit run -a` - Run all pre-commit hooks (formatting, linting)

### Running the Application
- `cargo run -- --start-web --db-path room_101.db [bootstrap-node-ids...]` - Run with web UI
- `cargo run -- --db-path room_101.db [bootstrap-node-ids...]` - Run without web UI

## Architecture Overview

Room 101 is a peer-to-peer networking application built with:
- **Iroh**: P2P networking library for node discovery and gossip communication
- **SurrealDB**: Local database for storing peers, events, and identity
- **Poem**: Web framework for the optional HTTP interface
- **Age**: Encryption library for secure data handling

### Core Components

**main.rs**: Application entry point that coordinates the network manager and optional web server tasks with graceful shutdown handling.

**network.rs**: Core P2P networking layer that:
- Manages iroh endpoint and gossip protocol
- Handles peer discovery and communication via signed messages
- Coordinates multiple async tasks: gossip setup, message listener/sender, heartbeat

**db.rs**: Database abstraction layer providing:
- `Identity` model for cryptographic keys (Iroh SecretKey + Age private key)
- `Peer` model for tracking network nodes and their status
- `Event` model for logging application events with structured JSON data

**webserver.rs**: Optional HTTP interface using Poem + Maud for:
- Viewing connected peers and their status
- Adding new bootstrap peers
- Browsing application event history

### Key Design Patterns
- All network communication uses cryptographically signed messages
- Graceful shutdown coordination via broadcast channels
- Database migrations in `migrations/` directory

## Development Environment

This project uses devenv.nix for reproducible development environments with:
- Rust toolchain with mold linker for fast builds
- Pre-commit hooks for code formatting (treefmt) and linting (clippy)
- Task automation for database operations

## Project Planning and Task Management
- **Primary Source of Truth**: `TODO.md` contains all current tasks, architectural plans, and implementation details
- **Context Persistence**: Due to chat context limitations, always update TODO.md when:
  - Starting work on new features or refactors
  - Completing tasks or milestones
  - Discovering issues or blockers
  - Planning multi-phase implementations
- **Major Refactor in Progress**: The application is currently being refactored to use a task-based broadcast architecture with tokio::sync::broadcast channels - see TODO.md for complete details
- **Task Organization**: TODO.md uses nested hierarchical structure with detailed implementation notes, code examples, and migration strategies
- This application focuses on cryptographic secrets management in a P2P network

## Testing
- Preferred test runner: `cargo nextest run` (faster than `cargo test`)
- Tests automatically run via `enterTest` in devenv configuration

## Rust Development Workflow
- **Frequent Checking**: Run `cargo check` and `cargo clippy` often during development, not just at the end
- **Before Task Completion**: Always call `cargo check` before finishing any task where you modified Rust source files
- **Clippy Compliance**: Use inlined format arguments in `format!()` macros (e.g., `format!("Hello {name}")` instead of `format!("Hello {}", name)`) to pass clippy linting
- **Git Commits**: Proactively suggest making git commits when completing logical units of work or reaching stable milestones
- **Clean Logging**: Never use emoji in log messages. Keep logging output clean and professional for production systems

## Code Quality Guidelines
- **Simplicity First**: Write simple, readable code. Using `.clone()` is perfectly acceptable until performance becomes an issue.
- **Proper Error Handling**: Use proper error handling everywhere. Never use `.expect()` or `.unwrap()` in production code - only in tests where panics are acceptable.
- **Error Types**: Use `anyhow::Result<T>` for functions that return errors. Use `anyhow::Context` to add context to error chains.
- **Custom Errors**: Use `thiserror` derive macro to create custom error enums when you need structured error types.
- **Error Propagation**: Use the `?` operator for error propagation. Prefer `anyhow::bail!()` and `anyhow::ensure!()` for early returns with errors.
- **Error Context**: Always add meaningful context when errors bubble up using `.context()` or `.with_context()`.

## Logging Guidelines
- **Info Level**: Keep info logs minimal and quiet - only for important application events (startup, shutdown, major state changes)
- **Debug Level**: Add solid debug logging for debugging application flow and important operations
- **Trace Level**: Add extensive trace logging for detailed execution flow, message handling, and low-level operations
- **ABSOLUTELY NO EMOJIS**: Never use emojis in log messages - keep output clean and professional for production systems
- **Professional Output**: Log messages must be professional, clean, and machine-readable
- **Structured Data**: Include relevant context like node_ids, message types, timestamps in log messages
- **Use Proper Logging**: Never use `println!` or `print!` - always use tracing macros (info!, debug!, trace!, error!, warn!)
