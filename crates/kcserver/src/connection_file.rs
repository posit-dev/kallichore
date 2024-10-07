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
use std::sync::Arc;
use std::sync::RwLock;

use serde::Deserialize;
use serde::Serialize;

/// The contents of the Connection File as listed in the Jupyter specfication;
/// directly parsed from JSON.
#[derive(Serialize, Deserialize, Debug, Clone)]
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
    #[allow(dead_code)]
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

    /// Find a free port that is not in the reserved list.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the port to find. This is used for logging.
    /// * `reserved_ports` - A list of ports that should not be used.
    fn find_port(
        name: String,
        reserved_ports: Arc<RwLock<Vec<u16>>>,
    ) -> Result<u16, anyhow::Error> {
        // The current candidate port; 0 indicates we haven't found one yet
        let mut port = 0;

        // The number of times we've tried to find an unused, unreserved port
        let mut tries = 0;

        while port == 0 {
            // Find a free port
            let candidate = match portpicker::pick_unused_port() {
                Some(port) => port,
                None => {
                    return Err(anyhow::anyhow!(
                        "Failed to pick {} port; no free ports available or port range exhausted",
                        name
                    ));
                }
            };

            // Check if the port is reserved
            {
                let reserved_ports = reserved_ports.read().unwrap();
                if reserved_ports.contains(&candidate) {
                    // Try up to 10 times to find an unreserved port. Since
                    // we're picking from a large range of ports, hitting a
                    // previously reserved port is unlikely, but possible. If it
                    // happens 10 times in a row, something is probably wrong.
                    tries += 1;
                    if tries > 10 {
                        return Err(anyhow::anyhow!(
                            "Failed to pick unreserved {} port after 10 tries",
                            name
                        ));
                    }
                    log::trace!(
                        "Port {} is reserved; trying again (attempt {})",
                        candidate,
                        tries
                    );
                    continue;
                }
            }

            // Reserve the port
            {
                let mut reserved_ports = reserved_ports.write().unwrap();
                reserved_ports.push(candidate);
                log::trace!(
                    "Picked {} port: {} ({} ports reserved)",
                    name,
                    candidate,
                    reserved_ports.len()
                );
            }

            port = candidate;
            break;
        }

        Ok(port)
    }

    /// Generate a new ConnectionFile by picking free ports.
    ///
    /// # Arguments
    ///
    /// * `ip` - The IP address to bind to
    /// * `reserved_ports` - A list of ports that should not be used. These are
    /// generally ports that are already in use by other running kernels, or
    /// have been reserved for use by another kernel that's also starting up.
    pub fn generate(
        ip: String,
        reserved_ports: Arc<RwLock<Vec<u16>>>,
    ) -> Result<Self, anyhow::Error> {
        use rand::Rng;

        let key_bytes = rand::thread_rng().gen::<[u8; 16]>();
        let key = hex::encode(key_bytes);

        let control_port =
            ConnectionFile::find_port(String::from("control"), reserved_ports.clone())?;
        let shell_port = ConnectionFile::find_port(String::from("shell"), reserved_ports.clone())?;
        let iopub_port = ConnectionFile::find_port(String::from("iopub"), reserved_ports.clone())?;
        let hb_port = ConnectionFile::find_port(String::from("heartbeat"), reserved_ports.clone())?;
        let stdin_port = ConnectionFile::find_port(String::from("stdin"), reserved_ports.clone())?;
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
    #[allow(dead_code)]
    pub fn endpoint(&self, port: u16) -> String {
        format!("{}://{}:{}", self.transport, self.ip, port)
    }
}
