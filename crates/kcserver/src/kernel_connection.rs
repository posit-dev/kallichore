//
// kernel_connection.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
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

    /// Return a copy of this connection that signs/validates messages with
    /// `key` instead of the key it was created with.
    ///
    /// This is used when adopting a kernel that's already running (see
    /// `KernelSession::connect`): the kernel was started with its own signing
    /// key, which is only known once the adopting client sends it in the
    /// `ConnectionInfo` it supplies. That key must be trusted over the key
    /// this session was created with, or messages to/from the adopted kernel
    /// will fail HMAC validation.
    ///
    /// An empty key follows the Jupyter convention that no key means messages
    /// are unsigned.
    pub fn with_key(&self, key: &str) -> Result<Self, anyhow::Error> {
        let mut connection = self.clone();
        if key.is_empty() {
            connection.key = None;
            connection.hmac_key = None;
        } else {
            connection.hmac_key = Some(Hmac::<Sha256>::new_from_slice(key.as_bytes())?);
            connection.key = Some(key.to_string());
        }
        Ok(connection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `KernelConnection` directly (bypassing `from_session`) so
    /// tests don't need to construct a full `models::NewSession`.
    fn test_connection(key: &str) -> KernelConnection {
        let (key, hmac_key) = if key.is_empty() {
            (None, None)
        } else {
            let hmac_key = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
            (Some(key.to_string()), Some(hmac_key))
        };
        KernelConnection {
            session_id: "test-session".to_string(),
            username: "test-user".to_string(),
            key,
            protocol_version: "5.3".to_string(),
            hmac_key,
        }
    }

    #[test]
    fn with_key_installs_the_supplied_key() {
        let original = test_connection("original-key");
        let adopted = original.with_key("adopted-key").unwrap();

        assert_eq!(adopted.key.as_deref(), Some("adopted-key"));
        assert_eq!(adopted.session_id, original.session_id);
        assert_eq!(adopted.username, original.username);

        // The HMAC state was actually rebuilt from the new key, not just the
        // `key` string: signing the same content produces a different
        // signature under each connection's key.
        let mut original_mac = original.hmac_key.unwrap();
        let mut adopted_mac = adopted.hmac_key.unwrap();
        original_mac.update(b"payload");
        adopted_mac.update(b"payload");
        assert_ne!(
            original_mac.finalize().into_bytes(),
            adopted_mac.finalize().into_bytes()
        );
    }

    #[test]
    fn with_key_empty_disables_signing() {
        let original = test_connection("original-key");
        let adopted = original.with_key("").unwrap();

        assert_eq!(adopted.key, None);
        assert!(adopted.hmac_key.is_none());
    }
}
