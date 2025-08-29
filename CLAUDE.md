# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Build and Check
- `cargo check` - Check code for errors without building (fast)
- `cargo build` - Build the project
- `cargo test` or `cargo nextest run` - Run tests (nextest is preferred)
- `treefmt` - Format all code files
- `pre-commit run -a` - Run all pre-commit hooks (formatting, linting, SQLx metadata)

### Database Operations
- `devenv tasks run db:reset` - Reset database completely
- `devenv tasks run db:start-fresh` - Reset and setup database
- `devenv tasks run db:sqlx-prepare` - Generate SQLx metadata (auto-runs on SQL changes)

### Running the Application
- `cargo run -- --start-web --db-path room_101.db [bootstrap-node-ids...]` - Run with web UI
- `cargo run -- --db-path room_101.db [bootstrap-node-ids...]` - Run without web UI

## Architecture Overview

Room 101 is a peer-to-peer networking application built with:
- **Iroh**: P2P networking library for node discovery and gossip communication
- **SQLite**: Local database for storing peers, events, and identity
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
- SQLx for compile-time checked database queries
- Database migrations in `migrations/` directory

## Development Environment

This project uses devenv.nix for reproducible development environments with:
- Rust toolchain with mold linker for fast builds
- Pre-commit hooks for code formatting (treefmt), linting (clippy), and SQLx metadata generation
- Task automation for database operations

## Project Planning
- Check `TODO.md` for current feature roadmap and implementation status
- This application focuses on cryptographic secrets management in a P2P network

## Testing
- Preferred test runner: `cargo nextest run` (faster than `cargo test`)
- Tests automatically run via `enterTest` in devenv configuration

## Rust Development Workflow
- Call `cargo check` before finishing any task where you modified Rust source files.
- Use inlined format arguments in `format!()` macros (e.g., `format!("Hello {name}")` instead of `format!("Hello {}", name)`) to pass clippy linting.
