//
// wire_message.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use kcshared::jupyter_message::JupyterMessage;
use sha2::Sha256;

use crate::wire_message_header::WireMessageHeader;

use hmac::{Hmac, Mac};

pub struct WireMessage {
    /// The parts of the message, as an array of byte arrays
    pub parts: Vec<Vec<u8>>,
}

impl WireMessage {
    /// Create a new wire message from a Jupyter message.
    pub fn new(
        msg: JupyterMessage,
        session_id: String,
        hmac_key: Hmac<Sha256>,
    ) -> Result<Self, anyhow::Error> {
        let mut parts: Vec<Vec<u8>> = Vec::new();

        // Derive a wire message header from the Jupyter message header
        let header = WireMessageHeader::new(msg.header, session_id.clone());
        parts.push(serde_json::to_vec(&header)?);

        // Add the parent header, if any
        if msg.parent_header.is_some() {
            let parent_header =
                WireMessageHeader::new(msg.parent_header.unwrap(), session_id.clone());
            parts.push(serde_json::to_vec(&parent_header)?);
        } else {
            parts.push(serde_json::to_vec(&serde_json::Map::new())?);
        }

        // Add the metadata
        parts.push(serde_json::to_vec(&msg.metadata)?);

        // Add the content
        parts.push(serde_json::to_vec(&msg.content)?);

        // Compute the HMAC signature from all of the existing parts and prepend it
        let mut signature = hmac_key.clone();
        for part in &parts {
            signature.update(part);
        }
        let signature = hex::encode(signature.finalize().into_bytes());
        parts.insert(0, signature.as_bytes().to_vec());

        Ok(WireMessage { parts })
    }
}
