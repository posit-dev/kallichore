//
// wire_message_header.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use kcshared::jupyter_message::JupyterMessageHeader;

pub struct WireMessageHeader {
    /// The message ID
    pub msg_id: String,

    /// The type of the message
    pub msg_type: String,

    /// The ID of the session
    pub session_id: String,

    /// The date/time the message was published
    pub date: String,

    /// The version of the Jupyter protocol
    pub version: String,
}

impl WireMessageHeader {
    /// Create a new wire message header from a Jupyter message header.
    pub fn new(jupyter_header: JupyterMessageHeader) -> Self {
        // Create an ISO 8601 date string
        let date = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        WireMessageHeader {
            msg_id: jupyter_header.msg_id,
            msg_type: jupyter_header.msg_type,
            version: String::from("5.3"),
            date: date,
            session_id: String::from(""),
        }
    }
}
