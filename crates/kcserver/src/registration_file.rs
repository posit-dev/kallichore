//
// registration_file.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::path::Path;

/// The contents of the Registration File as specified in JEP 66.
/// Used for kernel handshaking protocol.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegistrationFile {
    /// The transport type to use for ZeroMQ; generally "tcp"
    pub transport: String,

    /// The signature scheme to use for messages; generally "hmac-sha256"
    pub signature_scheme: String,

    /// The IP address to bind to
    pub ip: String,

    /// The HMAC-256 signing key, or an empty string for an unauthenticated
    /// connection
    pub key: Option<String>,

    /// ZeroMQ port: Registration messages (handshake)
    pub registration_port: u16,
}

impl RegistrationFile {
    /// Create a RegistrationFile from the parts needed to connect
    pub fn new(ip: String, port: u16, key: Option<String>) -> Self {
        Self {
            transport: "tcp".to_string(),
            signature_scheme: "hmac-sha256".to_string(),
            ip,
            key,
            registration_port: port,
        }
    }

    /// Write the registration file to disk
    pub fn to_file<P: AsRef<Path>>(&self, file_path: P) -> Result<(), Box<dyn Error>> {
        let file = File::create(file_path)?;
        serde_json::to_writer_pretty(file, &self)?;
        Ok(())
    }
}
