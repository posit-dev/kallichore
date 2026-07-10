//
// handshake_socket.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Client-owned handshake socket support.
//!
//! The client creates and listens on a handshake socket, then launches the
//! server pointing at it. Once the server has bound its main transport, it
//! connects to the handshake socket and reports its connection details in a
//! single JSON document. Because the client owns the socket, there is no
//! filesystem artifact for antivirus to scan and no window in which the client
//! cannot see the server's details; the bearer token also never touches disk.
//!
//! On Unix the handshake socket is a Unix domain socket; on Windows it is a
//! named pipe. The client creates and secures the socket, so its permissions
//! are the client's responsibility. This module only connects to the path it is
//! given, writes the payload, and closes.

use serde::{Deserialize, Serialize};

use crate::transport::ServerConnectionType;

/// The connection details reported to the client over the handshake socket.
///
/// Field names are snake_case to match the client's `KallichoreServerState`.
/// Transport-specific address fields are omitted when not relevant.
#[derive(Serialize, Deserialize)]
pub struct HandshakePayload {
    /// The port the server is listening on (TCP only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// The full API basepath, starting with 'http' (TCP only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,

    /// The path to the Unix domain socket (Unix only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_path: Option<String>,

    /// The named pipe path (Windows only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named_pipe: Option<String>,

    /// The transport type: "tcp", "socket", or "named-pipe"
    pub transport: String,

    /// The path to the server executable (this process)
    pub server_path: String,

    /// The PID of the server process
    pub server_pid: u32,

    /// The authentication token, if any (null when --token none)
    pub bearer_token: Option<String>,

    /// The path to the log file, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,

    /// A unique identifier generated each time the server starts. The client
    /// uses it to detect that a persisted token belongs to a server that has
    /// since been replaced.
    pub server_id: String,
}

impl HandshakePayload {
    /// Build a handshake payload from the resolved connection details.
    pub fn new(
        connection_info: &ServerConnectionType,
        token: &Option<String>,
        log_file: &Option<String>,
        server_id: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Get the server path
        let server_path = std::env::current_exe()?
            .to_str()
            .ok_or("Failed to convert server path to string")?
            .to_string();

        // Get the server PID
        let server_pid = std::process::id();

        // Fill in the transport-specific address fields based on the connection type
        let payload = match connection_info {
            ServerConnectionType::Tcp { port, base_path } => HandshakePayload {
                port: Some(*port),
                base_path: Some(base_path.clone()),
                socket_path: None,
                named_pipe: None,
                transport: "tcp".to_string(),
                server_path,
                server_pid,
                bearer_token: token.clone(),
                log_path: log_file.clone(),
                server_id: server_id.to_string(),
            },
            #[cfg(unix)]
            ServerConnectionType::Socket { socket_path, .. } => HandshakePayload {
                port: None,
                base_path: None,
                socket_path: Some(socket_path.clone()),
                named_pipe: None,
                transport: "socket".to_string(),
                server_path,
                server_pid,
                bearer_token: token.clone(),
                log_path: log_file.clone(),
                server_id: server_id.to_string(),
            },
            #[cfg(windows)]
            ServerConnectionType::NamedPipe { pipe_name } => HandshakePayload {
                port: None,
                base_path: None,
                socket_path: None,
                named_pipe: Some(pipe_name.clone()),
                transport: "named-pipe".to_string(),
                server_path,
                server_pid,
                bearer_token: token.clone(),
                log_path: log_file.clone(),
                server_id: server_id.to_string(),
            },
        };

        Ok(payload)
    }
}

/// Perform the handshake: serialize the payload, connect to the client's
/// handshake socket, write the JSON document, and close.
///
/// This runs only at initial launch. A failure here is fatal for the launch,
/// since the client will never learn how to reach us; callers should exit
/// non-zero on error.
pub async fn perform_handshake(
    handshake_path: &str,
    payload: &HandshakePayload,
) -> Result<(), Box<dyn std::error::Error>> {
    // Serialize to JSON and append a trailing newline. The client reads to EOF,
    // so the newline is not strictly required, but it is harmless and makes the
    // stream easier to read.
    let mut json = serde_json::to_string(payload)?;
    json.push('\n');

    connect_and_send(handshake_path, json.as_bytes()).await?;

    Ok(())
}

#[cfg(unix)]
async fn connect_and_send(path: &str, payload: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;

    let mut stream = UnixStream::connect(path).await?;
    stream.write_all(payload).await?;
    // Half-close the write side so the client sees EOF.
    stream.shutdown().await?;
    Ok(())
}

#[cfg(windows)]
async fn connect_and_send(
    pipe_name: &str,
    payload: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ClientOptions;
    use windows::Win32::Foundation::ERROR_PIPE_BUSY;

    // The client's named pipe instance may not be ready to accept a connection
    // yet (ERROR_PIPE_BUSY). Retry a bounded number of times before giving up.
    const MAX_ATTEMPTS: u32 = 20;
    let pipe_busy = ERROR_PIPE_BUSY.0 as i32;
    let mut client = None;
    for attempt in 0..MAX_ATTEMPTS {
        match ClientOptions::new().open(pipe_name) {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(e) if e.raw_os_error() == Some(pipe_busy) && attempt + 1 < MAX_ATTEMPTS => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(Box::new(e)),
        }
    }

    let mut client =
        client.ok_or_else(|| "Timed out waiting for handshake named pipe to become available")?;
    client.write_all(payload).await?;
    client.flush().await?;
    Ok(())
}
