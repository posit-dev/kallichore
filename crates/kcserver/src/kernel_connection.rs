//
// kernel_connection.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use hmac::{Hmac, Mac};
use kallichore_api::models;
use sha2::Sha256;

#[derive(Debug, Clone)]
pub struct KernelConnection {
    /// The ID of the session
    pub session_id: String,

    /// The username of the user who owns the session
    pub username: String,

    /// The signing key, as a string
    pub key: Option<String>,

    /// The Jupyter protocol version
    pub protocol_version: String,

    /// The HMAC key used to sign messages, if any
    pub hmac_key: Option<Hmac<Sha256>>,
}

impl KernelConnection {
    pub fn from_session(session: &models::NewSession, key: String) -> Result<Self, anyhow::Error> {
        // Create a new random HMAC key to sign messages for this session
        let hmac_key = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;

        Ok(Self {
            session_id: session.session_id.clone(),
            username: session.username.clone(),
            protocol_version: match session.protocol_version.as_deref() {
                Some(version) => version.to_string(),
                None => String::from("5.3"),
            },
            key: Some(key),
            hmac_key: Some(hmac_key),
        })
    }
}
