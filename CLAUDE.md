# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Kallichore is a headless Jupyter kernel supervisor written in Rust that provides HTTP and WebSocket APIs for managing Jupyter kernel sessions. It serves as a bridge between frontend applications (like Positron IDE) and Jupyter kernels, handling session lifecycle, message routing, and kernel supervision.

## Tech Stack

- **Language**: Rust (Edition 2021, minimum version 1.75)
- **Architecture**: Multi-crate workspace with auto-generated API layer
- **Protocols**: Jupyter Protocol, WebSocket, ZeroMQ messaging
- **Authentication**: Bearer token with HMAC message signing

## Development Commands

### Building
```bash
cargo build              # Debug build
cargo build --release    # Release build
cargo clean             # Clean build artifacts
```

### Running the Server
```bash
export RUST_LOG=trace    # Optional: detailed logging
./target/debug/kcserver [--port PORT] [--token TOKEN_FILE]
```

### API Development
```bash
./scripts/regen-api.sh   # Regenerate API client/server from OpenAPI spec
```

**Important**: When regenerating API code, manual edits are preserved using `--- Start/End Kallichore ---` comment blocks.

### Testing
```bash
cargo test               # Run all tests
cargo test --package kcserver  # Test specific crate
```

## Architecture

### Workspace Structure
- **`kcserver/`**: Main server implementing the supervisor logic
- **`kallichore_api/`**: Auto-generated OpenAPI client/server code
- **`kcshared/`**: Shared types for Jupyter and WebSocket messages
- **`kcclient/`**: Command-line test client

### Message Flow
```
Frontend ↔ HTTP/WebSocket ↔ Kallichore ↔ ZeroMQ ↔ Jupyter Kernels
```

### Key Components

**Server (`kcserver/src/`)**:
- `main.rs`: Entry point and CLI argument parsing
- `session.rs`: Core session management and kernel lifecycle
- `websocket.rs`: WebSocket-to-ZeroMQ message proxy
- `auth.rs`: Authentication and HMAC message signing
- `kernel.rs`: Kernel process spawning and supervision

**API Layer (`kallichore_api/`)**:
- Generated from `kallichore.json` OpenAPI specification
- RESTful endpoints for session CRUD operations
- WebSocket upgrade handling
- Bearer token authentication middleware

**Shared Types (`kcshared/src/`)**:
- Jupyter protocol message structures
- WebSocket message wrappers
- Kernel handshake and discovery protocols

## Key Features

1. **Session Management**: Create, list, restart, and shutdown kernel sessions with per-session isolation
2. **Protocol Translation**: Seamless bridging between WebSocket and ZeroMQ protocols
3. **Kernel Supervision**: Process lifecycle management with heartbeat monitoring
4. **Security**: Bearer token authentication with HMAC-SHA256 message signing
5. **Multi-Platform**: Supports macOS, Windows, and Linux with automated releases

## API Development Notes

- The OpenAPI specification is in `kallichore.json`
- Generated code is in `kallichore_api/` with docs in `kallichore_api/docs/`
- Manual modifications to generated code must be wrapped in `--- Start/End Kallichore ---` comments
- Examples with TLS setup are in `kallichore_api/examples/`

## Dependencies

- ZeroMQ libraries (libsodium, pkg-config)
- OpenAPI Generator CLI for API regeneration
- Platform-specific build requirements for cross-compilation

## Logging

Use `RUST_LOG` environment variable for detailed debugging:
- `RUST_LOG=trace` for maximum verbosity
- `RUST_LOG=kallichore=debug` for library-specific logging