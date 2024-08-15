//
// connection_file.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

/// The contents of the Connection File as listed in the Jupyter specfication;
/// directly parsed from JSON.
#[derive(Serialize, Deserialize, Debug)]
pub struct ConnectionFile {
    /// ZeroMQ port: Control channel (kernel interrupts)
    pub control_port: u16,

    /// ZeroMQ port: Shell channel (execution, completion)
    pub shell_port: u16,

    /// ZeroMQ port: Standard input channel (prompts)
    pub stdin_port: u16,

    /// ZeroMQ port: IOPub channel (broadcasts input/output)
    pub iopub_port: u16,

    /// ZeroMQ port: Heartbeat messages (echo)
    pub hb_port: u16,

    /// The transport type to use for ZeroMQ; generally "tcp"
    pub transport: String,

    /// The signature scheme to use for messages; generally "hmac-sha256"
    pub signature_scheme: String,

    /// The IP address to bind to
    pub ip: String,

    /// The HMAC-256 signing key, or an empty string for an unauthenticated
    /// connection
    pub key: String,
}

impl ConnectionFile {
    /// Create a ConnectionFile by parsing the contents of a connection file.
    pub fn from_file<P: AsRef<Path>>(connection_file: P) -> Result<Self, Box<dyn Error>> {
        let file = File::open(connection_file)?;
        let reader = BufReader::new(file);
        let control = serde_json::from_reader(reader)?;

        Ok(control)
    }

    pub fn to_file<P: AsRef<Path>>(&self, connection_file: P) -> Result<(), Box<dyn Error>> {
        let file = File::create(connection_file)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    /// Generate a new ConnectionFile by picking free ports.
    pub fn generate(ip: String) -> Result<Self, anyhow::Error> {
        use rand::Rng;

        let key_bytes = rand::thread_rng().gen::<[u8; 16]>();
        let key = hex::encode(key_bytes);

        let control_port = match portpicker::pick_unused_port() {
            Some(port) => port,
            None => {
                return Err(anyhow::anyhow!(
                    "Failed to pick control port (no unused ports)"
                ))
            }
        };

        let shell_port = match portpicker::pick_unused_port() {
            Some(port) => port,
            None => {
                return Err(anyhow::anyhow!(
                    "Failed to pick stdin port (no unused ports)"
                ))
            }
        };
        let stdin_port = match portpicker::pick_unused_port() {
            Some(port) => port,
            None => {
                return Err(anyhow::anyhow!(
                    "Failed to pick stdin port (no unused ports)"
                ))
            }
        };
        let iopub_port = match portpicker::pick_unused_port() {
            Some(port) => port,
            None => {
                return Err(anyhow::anyhow!(
                    "Failed to pick iopub port (no unused ports)"
                ))
            }
        };
        let hb_port = match portpicker::pick_unused_port() {
            Some(port) => port,
            None => {
                return Err(anyhow::anyhow!(
                    "Failed to pick heartbeat port (no unused ports)"
                ))
            }
        };
        Ok(Self {
            control_port,
            shell_port,
            stdin_port,
            iopub_port,
            hb_port,
            transport: "tcp".to_string(),
            signature_scheme: "hmac-sha256".to_string(),
            key,
            ip,
        })
    }

    /// Given a port, return a URI-like string that can be used to connect to
    /// the port, given the other parameters in the connection file.
    ///
    /// Example: `32` => `"tcp://127.0.0.1:32"`
    pub fn endpoint(&self, port: u16) -> String {
        format!("{}://{}:{}", self.transport, self.ip, port)
    }
}
