//
// jupyter_message.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use serde::{Deserialize, Serialize};

/// The (partial) header of a Jupyter message.
///
/// Additional header fields (such as the session ID) are not included here;
/// they are populated by Kallichore before sending the message over the ZeroMQ
/// socket.
#[derive(Serialize, Deserialize)]
pub struct JupyterMessageHeader {
    /// The message ID
    pub msg_id: String,
    /// The type of the message
    pub msg_type: String,
}

/// The set of all Jupyter sockets ("channels") over which messages are sent and
/// received.
#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum JupyterChannel {
    /// The shell channel
    Shell,

    /// The control channel
    Control,

    /// The stdin channel
    Stdin,

    /// The iopub channel
    IOPub,

    /// The heartbeat channel
    Heartbeat,
}

/// A serialized Jupyter message.
#[derive(Serialize, Deserialize)]
pub struct JupyterMessage {
    /// The header of the message
    pub header: JupyterMessageHeader,

    /// The header of the message's parent (the message that caused this message)
    pub parent_header: Option<JupyterMessageHeader>,

    /// The channel on which the message was sent (or is to be sent)
    pub channel: JupyterChannel,

    /// The message payload
    pub content: serde_json::Value,

    /// Additional metadata
    pub metadata: serde_json::Value,

    /// The message buffers
    pub buffers: Vec<serde_json::Value>,
}
