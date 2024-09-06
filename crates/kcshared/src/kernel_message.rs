//
// kernel_message.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use kallichore_api::models;
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

/// Messages that are sent from Kallichore to the client about the kernel
/// itself. For messages bridging the Jupyter protocol, see `JupyterMessage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KernelMessage {
    /// The kernel's status has changed
    Status(models::Status),

    /// The kernel process has emitted output. Most output gets emitted on
    /// iopub, so this is for output that escapes the standard stream capture or
    /// occurs before/after the kernel is fully online.
    Output(OutputStream, String),

    /// The kernel has queued an execution request. The parameter is the ID of
    /// the queued request. This message is sent when the client sends a request
    /// to execute code, but the kernel is busy executing other code.
    ExecutionQueued(String),

    /// The kernel has exited
    Exited(i32),
}
