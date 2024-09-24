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

    /// The HMAC key used to sign messages
    pub hmac_key: Hmac<Sha256>,
}

impl KernelConnection {
    pub fn from_session(session: &models::NewSession, key: String) -> Result<Self, anyhow::Error> {
        // Create a new random HMAC key to sign messages for this session
        let hmac_key = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;

        Ok(Self {
            session_id: session.session_id.clone(),
            username: session.username.clone(),
            hmac_key,
        })
    }
}
