//
// connection.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Connection file management for kernel sessions.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{connection_file::ConnectionFile, kernel_state::KernelState};

/// Manages connection file operations for a kernel session.
pub struct ConnectionManager {
    /// Session ID for logging
    session_id: String,

    /// Shared kernel state
    state: Arc<RwLock<KernelState>>,

    /// Reserved ports across all kernel sessions
    reserved_ports: Arc<std::sync::RwLock<Vec<i32>>>,

    /// Connection key for authentication
    connection_key: Option<String>,
}

impl ConnectionManager {
    /// Create a new connection manager.
    pub fn new(
        session_id: String,
        state: Arc<RwLock<KernelState>>,
        reserved_ports: Arc<std::sync::RwLock<Vec<i32>>>,
        connection_key: Option<String>,
    ) -> Self {
        Self {
            session_id,
            state,
            reserved_ports,
            connection_key,
        }
    }

    /// Update the connection file for this kernel session.
    ///
    /// Used when connection details become available after handshaking.
    pub async fn update_connection_file(&self, connection_file: ConnectionFile) {
        let mut state = self.state.write().await;
        state.connection_file = Some(connection_file);
    }

    /// Get the current connection file, if available.
    pub async fn get_connection_file(&self) -> Option<ConnectionFile> {
        let state = self.state.read().await;
        state.connection_file.clone()
    }

    /// Ensure a connection file exists, generating one if necessary.
    ///
    /// This is used when we need connection information but may not have
    /// received it yet (e.g., for kernels that don't use JEP 66 handshaking).
    pub async fn ensure_connection_file(&self) -> Result<ConnectionFile, anyhow::Error> {
        match self.get_connection_file().await {
            Some(connection_file) => {
                // We have a connection file; return it
                Ok(connection_file)
            }
            None => {
                // Generate a new connection file
                let connection_file = ConnectionFile::generate(
                    "127.0.0.1".to_string(),
                    self.reserved_ports.clone(),
                    self.connection_key.clone(),
                )?;
                self.update_connection_file(connection_file.clone()).await;
                log::debug!(
                    "[session {}] Generated connection file with ports: shell={}, iopub={}, stdin={}, control={}, hb={}",
                    self.session_id,
                    connection_file.info.shell_port,
                    connection_file.info.iopub_port,
                    connection_file.info.stdin_port,
                    connection_file.info.control_port,
                    connection_file.info.hb_port
                );
                Ok(connection_file)
            }
        }
    }
}
