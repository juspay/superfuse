# Superfuse

A virtual filesystem for templated configuration files powered by [Superposition](https://juspay.io/open-source/superposition). Superfuse mounts a FUSE filesystem that dynamically renders Handlebars templates with configuration values from Superposition, enabling real-time configuration management through standard filesystem operations.

## Features

- **Virtual Filesystem**: Mount a FUSE filesystem that presents templated configs as regular files
- **Dynamic Rendering**: Templates are rendered on-demand using live configuration values from Superposition
- **Context Support**: Access different configuration contexts via file paths (e.g., `/key=value/env=prod/app.yaml`)
- **Template Management**: Add, remove, update, and list file mappings via CLI
- **SQLite Persistence**: File mappings are stored locally for fast lookups
- **Multi-threaded**: Configurable worker threads for concurrent request handling
- **Graceful Shutdown**: Handles SIGINT, SIGTERM, and SIGHUP for clean unmounting

## Prerequisites

- Linux or macOS with FUSE support
- [FUSE 3](https://github.com/libfuse/libfuse) (Linux) or [macFUSE](https://osxfuse.github.io/) (macOS)
- Rust toolchain (see `rust-toolchain.toml`)
- Access to a Superposition instance

## Installation

### Using Cargo

```bash
cargo build --release
```

The binary will be available at `target/release/superfuse`.

### Using Nix

```bash
nix build
```

## Configuration

Superfuse uses environment variables for configuration. Create a `.env` file or export variables:

```bash
# Required
export SUPERPOSITION_ENDPOINT="https://superposition.juspay.io"
export SUPERPOSITION_TOKEN="your-api-token"
export SUPERPOSITION_ORG_ID="your-org"
export SUPERPOSITION_WORKSPACE_ID="your-workspace"

# Optional (with defaults)
export SUPERPOSITION_CACHE_SIZE=500        # Evaluation cache size
export SUPERPOSITION_CACHE_TTL=3600        # Cache TTL in seconds
export SUPERPOSITION_POLL_FREQUENCY=60     # Config polling interval
export SUPERPOSITION_POLL_TIMEOUT=30       # Polling timeout
export SUPERFUSE_LOG_LEVEL=info            # Logging level
```

## Usage

### Start the Filesystem

```bash
# Mount at /mnt/superfuse
superfuse start /mnt/superfuse

# With custom options
superfuse start /mnt/superfuse --n-threads 8 --auto-unmount
```

### Manage File Mappings

```bash
# Add a new template mapping
superfuse add --path /config/database.yaml --path /path/to/templates/database.hbs

# List all mappings
superfuse list

# Update a mapping
superfuse update --path /config/database.yaml --path /path/to/templates/database-v2.hbs

# Remove a mapping
superfuse remove --path /config/database.yaml
```

### Accessing Configuration Files

Once mounted, access your templated configs like regular files:

```bash
# Read rendered template with default context
cat /mnt/superfuse/config/app.yaml

# Read with specific context (using @context suffix)
cat /mnt/superfuse/config/app.yaml@production
```

## Template Syntax

Templates use [Handlebars](https://handlebarsjs.com/) syntax with Superposition configuration values:

```handlebars
# database.hbs
host: {{database.host}}
port: {{database.port}}
username: {{database.username}}
password: {{database.password}}
```

Configuration values are resolved through the Superposition provider at read time.

## Architecture

```
┌─────────────────┐     ┌──────────────┐     ┌─────────────────┐
│   Client App    │────▶│  Superfuse   │────▶│   FUSE Driver   │
│  (cat, vim, etc)│     │   Filesystem │     │                 │
└─────────────────┘     └──────────────┘     └─────────────────┘
                               │
                               ▼
                        ┌──────────────┐
                        │   SQLite DB  │ (File mappings)
                        └──────────────┘
                               │
                               ▼
                        ┌──────────────┐
                        │  Superposition│ (Config values)
                        │   Provider   │
                        └──────────────┘
```

- **SQLite**: Stores virtual path to template file mappings
- **FUSE**: Presents templates as regular files in the mounted filesystem
- **Superposition Provider**: Resolves configuration keys to values with caching and polling

## Project Structure

```
superfuse/
├── src/
│   ├── main.rs      # CLI and command dispatch
│   ├── config.rs    # Configuration and provider setup
│   ├── storage.rs   # Database operations and FUSE implementation
│   └── error.rs     # Error types
├── Cargo.toml
├── flake.nix        # Nix development environment
└── rust-toolchain.toml
```

## Development

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Development Environment

With Nix:

```bash
nix develop
```

## Data Directory

Superfuse stores data in platform-specific locations:

- **Linux**: `~/.local/share/superfuse/`
- **macOS**: `~/Library/Application Support/com.juspay.superfuse/`
- **Windows**: `%APPDATA%\juspay\superfuse\`

Contains:
- `superfuse.db` - SQLite database with file mappings
- `logs/` - Daily rotating logs

## License

[LICENSE](./LICENSE)

## Contributing

Contributions are welcome! Please ensure your code follows the existing patterns and includes appropriate tests.
