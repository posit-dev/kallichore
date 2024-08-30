//
// kernel_session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use std::sync::Arc;

use async_channel::{Receiver, Sender};
use chrono::{DateTime, Utc};
use kallichore_api::models;
use tokio::sync::RwLock;

use crate::{
    connection_file, kernel_connection::KernelConnection, kernel_state::KernelState,
    wire_message::ZmqChannelMessage,
};

#[derive(Debug, Clone)]
pub struct KernelSession {
    /// Metadata about the session
    pub connection: KernelConnection,

    /// The command line arguments used to start the kernel. The first is the
    /// path to the kernel itself.
    pub argv: Vec<String>,

    /// The process ID of the kernel
    pub process_id: Option<u32>,

    /// The current state of the kernel
    pub state: Arc<RwLock<KernelState>>,

    /// The date and time the kernel was started
    pub started: DateTime<Utc>,

    /// The date and time the kernel was started, as an ISO 8601 string
    pub ws_json_tx: Sender<String>,
    pub ws_json_rx: Receiver<String>,
    pub ws_zmq_tx: Sender<ZmqChannelMessage>,
    pub ws_zmq_rx: Receiver<ZmqChannelMessage>,
}

impl KernelSession {
    /// Create a new kernel session.
    pub fn new(
        session: models::Session,
        connection_file: connection_file::ConnectionFile,
    ) -> Result<Self, anyhow::Error> {
        // Start the session in a new thread
        let argv = session.argv.clone();

        let mut child = tokio::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(session.working_directory.clone())
            .envs(&session.env)
            .spawn()
            .expect("Failed to start child process");

        // Get the process ID of the child process
        let pid = child.id();

        // Add an unbounded MPSC channel to the session
        let (zmq_tx, zmq_rx) = async_channel::unbounded::<ZmqChannelMessage>();
        let (json_tx, json_rx) = async_channel::unbounded::<String>();
        let kernel_state = Arc::new(RwLock::new(KernelState::new(
            session.working_directory.clone(),
        )));
        let connection = KernelConnection::from_session(&session, connection_file.key.clone())?;
        let started = Utc::now();
        let kernel_session = KernelSession {
            argv: session.argv,
            process_id: pid,
            state: kernel_state.clone(),
            ws_json_tx: json_tx,
            ws_json_rx: json_rx,
            ws_zmq_tx: zmq_tx,
            ws_zmq_rx: zmq_rx,
            connection,
            started,
        };

        tokio::spawn(async move {
            let status = child.wait().await.expect("Failed to wait on child process");
            // update the status of the session
            let mut state = kernel_state.write().await;
            state.status = models::Status::Exited;
            log::info!(
                "Child process for session {} exited with status: {}",
                session.session_id,
                status
            );
        });

        Ok(kernel_session)
    }
}
