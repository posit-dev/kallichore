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

## Transport Types

Kallichore supports three different transport mechanisms for client connections:

### TCP (Default)
- **Platform**: All platforms (macOS, Windows, Linux)
- **Usage**: `--transport tcp` or omit (default)
- **Description**: Standard TCP sockets on 127.0.0.1
- **Best for**: General purpose, cross-platform compatibility

### Unix Domain Sockets
- **Platform**: Unix-like systems only (macOS, Linux)
- **Usage**: `--transport socket`
- **Description**: Unix domain sockets for inter-process communication with WebSocket protocol support
- **Protocol**: WebSocket over Unix domain sockets (supports `ws+unix:` URLs)
- **Best for**: High performance local communication, reduced overhead
- **Note**: Not supported on Windows

### Named Pipes
- **Platform**: Windows only
- **Usage**: `--transport named-pipe`
- **Description**: Windows named pipes for inter-process communication
- **Best for**: Windows-native IPC, integration with Windows applications
- **Note**: Not supported on Unix systems

Example usage:
```bash
# TCP (default)
./target/debug/kcserver --port 8080

# Unix domain socket with WebSocket protocol (macOS/Linux)
./target/debug/kcserver --transport socket

# Named pipe (Windows)
./target/debug/kcserver --transport named-pipe
```

### WebSocket Protocol Support

**Unix Domain Sockets**: Starting with this version, Unix domain socket kernel client sessions use the WebSocket protocol instead of raw JSON. This enables clients to connect using `ws+unix:` URLs and provides a consistent WebSocket interface across all transport types.

**Client Connection Examples**:
- TCP WebSocket: `ws://localhost:8080/sessions/{session_id}/channels`
- Unix Socket WebSocket: `ws+unix:/path/to/socket:/sessions/{session_id}/channels` (conceptual - actual connection via domain socket path)

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run with verbose output
cargo test -- --nocapture
```

### Test Types

- **Unit tests**: Located within `src/` files using `#[cfg(test)]` modules
- **Integration tests**: Located in `crates/kcserver/tests/`
  - `integration_test.rs`: General TCP and WebSocket functionality
  - `named_pipe_test.rs`: Windows named pipe specific tests (Windows only)
- **Platform-specific tests**: Automatically disabled on unsupported platforms

### Test Environment

Tests automatically:
- Start temporary server instances
- Create isolated temporary directories
- Handle platform-specific transport mechanisms
- Clean up resources after completion
