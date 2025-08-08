# Database Configuration

Room 101 uses a **single `DATABASE_URL`** environment variable for all database configurations.

## Quick Start

```bash
# Default: embedded database at ./room_101.db
cargo run

# Custom local database
DATABASE_URL=surrealkv://my-database.db cargo run

# Remote database with authentication  
DATABASE_URL=ws://user:pass@localhost:8000 cargo run
```

## URL Formats

All database types use standard URL format with optional authentication:

### Local Databases
```bash
DATABASE_URL=surrealkv://room_101.db              # Default
DATABASE_URL=surrealkv://./data/my-app.db         # Relative path
DATABASE_URL=surrealkv:///var/lib/myapp/data.db   # Absolute path
```

### Remote Databases
```bash
DATABASE_URL=ws://localhost:8000                   # No auth
DATABASE_URL=ws://user:pass@localhost:8000         # With auth
DATABASE_URL=wss://admin:secret@prod.example.com   # Secure WebSocket
DATABASE_URL=http://user:pass@localhost:8000       # HTTP
DATABASE_URL=https://user:pass@api.example.com     # HTTPS
```

### Testing
```bash
DATABASE_URL=mem://                                # In-memory
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | Complete SurrealDB connection URL | `surrealkv://room_101.db` |

## Examples

```bash
# Local embedded (default) - creates ./room_101.db
cargo run

# Local embedded - custom location
DATABASE_URL=surrealkv://data/app.db cargo run

# Remote without authentication
DATABASE_URL=ws://localhost:8000 cargo run

# Remote with embedded credentials  
DATABASE_URL=ws://admin:secret@db.example.com:8000 cargo run

# Secure remote connection
DATABASE_URL=wss://user:pass@secure-db.example.com:443 cargo run
```

The application automatically:
- Creates the `room_101` namespace and `main` database
- Handles authentication via URL credentials
- Supports all SurrealDB connection types through the unified URL format