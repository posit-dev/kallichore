//
// kernel_message.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use kallichore_api::models;
use serde::{Deserialize, Serialize};

/// Messages that are sent from Kallichore to the client about the kernel
/// itself. For messages bridging the Jupyter protocol, see `JupyterMessage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KernelMessage {
    /// The kernel's status has changed
    Status(models::Status),

    /// The kernel has exited
    Exited(i32),
}
