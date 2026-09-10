//
// kernel_message.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

use chrono::{DateTime, Utc};
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

/// A resource usage update from the kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUpdate {
    /// The CPU usage percentage for the kernel process and its children.
    pub cpu_percent: u64,

    /// The memory usage in bytes for the kernel process and its children.
    pub memory_bytes: u64,

    /// The thread count for the kernel process and its children.
    pub thread_count: u64,

    /// The current sampling period in milliseconds.
    pub sampling_period_ms: u64,

    /// A timestamp indicating when the resource usage was measured.
    pub timestamp: u64,
}

/// Describes who asked for an execution. iopub messages carry only a
/// `parent_header`, so this is the only way a client can tell code it did not
/// submit itself apart from its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionAttribution {
    /// The kind of actor that requested the execution, e.g. "agent".
    pub source: String,

    /// The name the agent reported in the MCP `clientInfo`, e.g. "claude-code".
    pub agent_name: Option<String>,

    /// The version the agent reported in the MCP `clientInfo`.
    pub agent_version: Option<String>,

    /// The MCP frontend whose token authorized the request.
    pub frontend_id: String,

    /// The MCP tool used, e.g. "execute_code" or "evaluate_code".
    pub tool: String,
}

/// An execution request submitted to a kernel by something other than the
/// connected client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRequested {
    /// The message ID of the `execute_request`; the parent ID of every iopub
    /// message the execution produces.
    pub msg_id: String,

    /// The code to be executed.
    pub code: String,

    /// When the request was submitted.
    pub requested_at: DateTime<Utc>,

    /// Who requested the execution.
    pub attribution: ExecutionAttribution,
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

    /// The kernel's resource usage has changed. The parameter is the new
    /// resource usage information.
    ResourceUsage(ResourceUpdate),

    /// The websocket connection to the client is about to be closed. The
    /// parameter is the reason for the disconnection.
    ClientDisconnected(String),

    /// The kernel has exited
    Exited(i32),

    /// The kernel has completed the JEP 66 handshake. The parameters are the session
    /// ID and connection info.
    HandshakeCompleted(String, ConnectionInfo),

    /// Code was submitted to the kernel by someone other than the connected
    /// client, such as an external agent using the MCP server. Sent just
    /// before the `execute_request` is queued, so it always precedes the
    /// iopub traffic it explains.
    ExecutionRequested(ExecutionRequested),
}
