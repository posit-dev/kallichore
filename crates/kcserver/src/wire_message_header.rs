//
// wire_message_header.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

use kcshared::{
    handshake_protocol::HandshakeVersion,
    jupyter_message::JupyterMessageHeader,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMessageHeader {
    /// The message ID
    pub msg_id: String,

    /// The type of the message
    pub msg_type: String,

    /// The ID of the session
    pub session: String,

    /// The username of the user who owns the session
    pub username: String,

    /// The date/time the message was published
    pub date: String,

    /// The version of the Jupyter protocol
    pub version: String,
}

impl WireMessageHeader {
    /// Create a new wire message header from a Jupyter message header.
    pub fn new(
        jupyter_header: JupyterMessageHeader, 
        session: String, 
        username: String, 
        handshake_version: Option<&HandshakeVersion>,
    ) -> Self {
        // Create an ISO 8601 date string to use as a timestamp
        let date = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        // Determine which protocol version to use
        let version = match handshake_version {
            // If we've successfully negotiated a handshake protocol version, use it
            Some(version) => format!("{}.{}", version.major, version.minor),
            // Otherwise use the traditional Jupyter protocol version
            None => String::from("5.3"),
        };

        // Create the wire message header from the Jupyter message header
        WireMessageHeader {
            msg_id: jupyter_header.msg_id,
            msg_type: jupyter_header.msg_type,
            version,
            date,
            session,
            username,
        }
    }
    
    /// Create a new wire message header for the traditional Jupyter protocol.
    pub fn new_v5(jupyter_header: JupyterMessageHeader, session: String, username: String) -> Self {
        Self::new(jupyter_header, session, username, None)
    }
}

impl From<WireMessageHeader> for JupyterMessageHeader {
    fn from(header: WireMessageHeader) -> Self {
        JupyterMessageHeader {
            msg_id: header.msg_id,
            msg_type: header.msg_type,
        }
    }
}
