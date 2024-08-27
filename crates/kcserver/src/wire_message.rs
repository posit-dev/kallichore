//
// wire_message.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use kcshared::jupyter_message::JupyterMessage;

/// Separates ZeroMQ socket identities from the message body payload.
const MSG_DELIM: &[u8] = b"<IDS|MSG>";

pub struct WireMessageHeader {
    /// The message ID
    pub msg_id: String,

    /// The type of the message
    pub msg_type: String,
}

pub struct WireMessage {
    /// The parts of the message, as an array of byte arrays
    parts: Vec<Vec<u8>>,
}

impl WireMessage {
    /// Create a new wire message from a Jupyter message.
    pub fn new(_msg: JupyterMessage) -> Self {
        WireMessage { parts: Vec::new() }
    }
}
