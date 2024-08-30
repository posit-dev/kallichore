//
// kernel_message.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use serde::{Deserialize, Serialize};

/// A superset of Jupyter kernel statuses
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KernelStatus {
    /// The kernel has not yet started
    Uninitialized,
    /// The kernel is in the process of starting
    Starting,
    /// The kernel is idle
    Idle,
    /// The kernel is busy
    Busy,
    /// The kernel is offline (it has not responded to a heartbeat message in the expected time)
    Offline,
    /// The kernel has exited
    Exited,
}

/// Messages that are sent from Kallichore to the client about the kernel
/// itself. For messages bridging the Jupyter protocol, see `JupyterMessage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KernelMessage {
    /// The kernel's status has changed
    Status(KernelStatus),

    /// The kernel has exited
    Exited(i32),
}
