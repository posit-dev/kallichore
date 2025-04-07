//
// kernel_message.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

use kallichore_api::models::{self, ConnectionInfo};
use serde::{Deserialize, Serialize};

/// Kernel output streams
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    /// Standard output
    Stdout,

    /// Standard error
    Stderr,
}

/// A status update from the kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusUpdate {
    /// The new status
    pub status: models::Status,

    /// The reason for the status change, if any
    pub reason: Option<String>,
}

/// Messages that are sent from Kallichore to the client about the kernel
/// itself. For messages bridging the Jupyter protocol, see `JupyterMessage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KernelMessage {
    /// The kernel's status has changed. The parameter is the new status,
    /// followed (optionally) by the reason for the status change.
    Status(StatusUpdate),

    /// The kernel process has emitted output. Most output gets emitted on
    /// iopub, so this is for output that escapes the standard stream capture or
    /// occurs before/after the kernel is fully online.
    Output(OutputStream, String),

    /// The kernel has queued an execution request. The parameter is the ID of
    /// the queued request. This message is sent when the client sends a request
    /// to execute code, but the kernel is busy executing other code.
    ExecutionQueued(String),

    /// The kernel's working directory has changed. The parameter is the new
    /// working directory.
    WorkingDirChanged(String),

    /// The websocket connection to the client is about to be closed. The
    /// parameter is the reason for the disconnection.
    ClientDisconnected(String),

    /// The kernel has exited
    Exited(i32),

    /// The kernel has completed the JEP 66 handshake. The parameters are the session
    /// ID and connection info.
    HandshakeCompleted(String, ConnectionInfo),
}
