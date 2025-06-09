//
// wire_message.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

/// Separates ZeroMQ socket identities from the message body payload.
pub const MSG_DELIM: &[u8] = b"<IDS|MSG>";

use bytes::Bytes;
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader};
use zeromq::ZmqMessage;

use crate::{kernel_connection::KernelConnection, wire_message_header::WireMessageHeader};

use base64::engine::Engine;
use hmac::Mac;
use sha2::Sha256;

pub struct WireMessage {
    /// The session ID the message is destined for
    pub session_id: String,

    /// The channel the message is destined for
    pub channel: JupyterChannel,

    /// The parts of the message, as an array of byte arrays
    pub parts: Vec<Vec<u8>>,
}

impl WireMessage {
    /// Create a new wire message from a Jupyter message.
    pub fn from_jupyter(
        msg: JupyterMessage,
        connection: KernelConnection,
    ) -> Result<Self, anyhow::Error> {
        let mut parts: Vec<Vec<u8>> = Vec::new();
        let username = connection.username.clone();
        let session = connection.session_id.clone();
        let hmac_key = connection.hmac_key.clone();

        // Derive a wire message header from the Jupyter message header
        let header = WireMessageHeader::new(
            msg.header,
            session.clone(),
            username.clone(),
            connection.protocol_version.clone(),
        );
        parts.push(serde_json::to_vec(&header)?);

        // Add the parent header, if any
        if msg.parent_header.is_some() {
            let parent_header = WireMessageHeader::new(
                msg.parent_header.unwrap(),
                session.clone(),
                username.clone(),
                connection.protocol_version.clone(),
            );
            parts.push(serde_json::to_vec(&parent_header)?);
        } else {
            parts.push(serde_json::to_vec(&serde_json::Map::new())?);
        }

        // Add the metadata
        parts.push(serde_json::to_vec(&msg.metadata)?);

        // Add the content
        parts.push(serde_json::to_vec(&msg.content)?);

        if let Some(hmac_key) = hmac_key {
            // If we have a key, compute the HMAC signature from all of the existing parts
            // and prepend it
            let mut signature = hmac_key.clone();
            for part in &parts {
                signature.update(part);
            }
            let signature = hex::encode(signature.finalize().into_bytes());
            parts.insert(0, signature.as_bytes().to_vec());
        } else {
            // No key, insert an empty signature
            parts.insert(0, Vec::new());
        }

        // Add the buffers
        // We do this here because we need to compute the HMAC signature first
        // https://jupyter-client.readthedocs.io/en/latest/messaging.html#the-wire-protocol
        for buf_str in msg.buffers.iter() {
            let decoded_buf = base64::engine::general_purpose::STANDARD.decode(buf_str)?;
            parts.push(decoded_buf);
        }

        Ok(WireMessage {
            session_id: connection.session_id.clone(),
            channel: msg.channel,
            parts,
        })
    }

    /// Convert the wire message to a Jupyter message with optional HMAC validation.
    pub fn to_jupyter(
        &self,
        channel: JupyterChannel,
        hmac_key: Option<hmac::Hmac<Sha256>>,
    ) -> Result<JupyterMessage, anyhow::Error> {
        let mut parts = self.parts.clone();
        let mut iter = self.parts.iter();
        let pos = match iter.position(|buf| &buf[..] == MSG_DELIM) {
            Some(pos) => pos,
            None => return Err(anyhow::anyhow!("No message delimiter found")),
        };
        let parts = parts.drain(pos + 1..).collect::<Vec<_>>();

        // Validate HMAC signature if a key is provided
        if let Some(key) = hmac_key {
            if parts.is_empty() {
                return Err(anyhow::anyhow!("Message has no signature part"));
            }

            // The signature is in parts[0], and it should be validated against parts[1-4]
            let signature_bytes = &parts[0];
            if !signature_bytes.is_empty() {
                let signature_str = std::str::from_utf8(signature_bytes)
                    .map_err(|_| anyhow::anyhow!("Invalid signature encoding"))?;

                // Compute expected signature over parts 1-4 (header, parent_header, metadata, content)
                let mut expected_mac = key.clone();
                for part in parts.iter().skip(1).take(4) {
                    expected_mac.update(part);
                }
                let expected_signature = hex::encode(expected_mac.finalize().into_bytes());

                if signature_str != expected_signature {
                    return Err(anyhow::anyhow!("HMAC signature validation failed"));
                }
            }
        }
        let header: WireMessageHeader = serde_json::from_value(Self::parse_buffer(&parts[1])?)?;
        let jupyter_header: JupyterMessageHeader = header.into();
        let parent_header: Option<JupyterMessageHeader> = if parts[2].len() < 5 {
            None
        } else {
            let header: WireMessageHeader = serde_json::from_value(Self::parse_buffer(&parts[2])?)?;
            Some(header.into())
        };

        // The remaining parts of the message are buffers; base64 encode their
        // contents for the Jupyter message
        let mut buffers = Vec::<String>::new();
        for (_i, buf) in parts.iter().enumerate().skip(5) {
            buffers.push(base64::engine::general_purpose::STANDARD.encode(buf));
        }

        Ok(JupyterMessage {
            header: jupyter_header,
            parent_header,
            metadata: serde_json::from_slice(&parts[3])?,
            content: serde_json::from_slice(&parts[4])?,
            channel,
            buffers,
        })
    }

    fn parse_buffer(buf: &[u8]) -> Result<serde_json::Value, anyhow::Error> {
        let contents = std::str::from_utf8(buf)?;
        let val = serde_json::from_str(contents)?;
        Ok(val)
    }

    pub fn from_zmq(session_id: String, channel: JupyterChannel, msg: ZmqMessage) -> Self {
        let parts: Vec<Vec<u8>> = msg.iter().map(|frame| frame.to_vec()).collect();
        Self {
            session_id,
            channel,
            parts,
        }
    }
}

impl Into<ZmqMessage> for WireMessage {
    fn into(self) -> ZmqMessage {
        let mut zmq_message = match self.channel {
            JupyterChannel::Shell | JupyterChannel::Stdin => {
                // The Shell and Stdin channels share a socket identity, which we derive from the session ID
                let mut message = ZmqMessage::from(Bytes::from(self.session_id));
                message.push_back(Bytes::from(MSG_DELIM));
                message
            }
            _ => ZmqMessage::from(MSG_DELIM.to_vec()),
        };
        for part in self.parts {
            zmq_message.push_back(Bytes::from(part));
        }
        zmq_message
    }
}
