//
// websocket_message.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

use serde::{Deserialize, Serialize};

use crate::{
    jupyter_message::JupyterMessage, 
    kernel_message::KernelMessage,
};

/// A message sent over a WebSocket connection. This message can be either a
/// Jupyter message (conforming roughly to the Jupyter kernel protocol) or a
/// kernel message (sent from the Kallichore control plane concerning the kernel
/// itself).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum WebsocketMessage {
    /// Kernel messages are messages about the kernel itself that are outside
    /// the bounds of the Jupyter protocol, such as startup/shutdown messages
    /// and interstitial status messages.
    #[serde(rename = "kernel")]
    Kernel(KernelMessage),

    /// Jupyter messages are messages that conform to the Jupyter protocol. They
    /// are not parsed by Kallichore, but are passed through to the Jupyter
    /// client.
    #[serde(rename = "jupyter")]
    Jupyter(JupyterMessage),
}
