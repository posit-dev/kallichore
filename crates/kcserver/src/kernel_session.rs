//
// kernel_session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use async_channel::{Receiver, Sender};
use kallichore_api::models;
use kcshared::jupyter_message::JupyterChannel;
use tokio::select;
use zeromq::SocketSend;

use crate::{
    connection_file,
    kernel_connection::KernelConnection,
    wire_message::{WireMessage, ZmqChannelMessage},
};
use zeromq::{DealerSocket, ReqSocket, Socket, SocketRecv, SubSocket, ZmqMessage};
pub struct KernelSession {
    /// Metadata about the session
    pub connection: KernelConnection,

    /// The command line arguments used to start the kernel. The first is the
    /// path to the kernel itself.
    pub argv: Vec<String>,

    /// The process ID of the kernel
    pub process_id: Option<u32>,

    /// The current status of the kernel
    pub status: models::Status,

    /// The connection information for the kernel
    pub connection_file: connection_file::ConnectionFile,

    pub ws_json_tx: Sender<String>,
    pub ws_json_rx: Receiver<String>,
    pub ws_zmq_tx: Sender<ZmqChannelMessage>,
    pub ws_zmq_rx: Receiver<ZmqChannelMessage>,

    /// The kernel's shell socket
    shell_socket: DealerSocket,

    /// The kernel's heartbeat socket
    hb_socket: ReqSocket,

    /// The kernel's iopub socket
    iopub_socket: SubSocket,
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

        let connection = KernelConnection::from_session(&session, connection_file.key.clone())?;
        let mut kernel_session = KernelSession {
            argv: session.argv,
            process_id: pid,
            status: models::Status::Idle,
            shell_socket: DealerSocket::new(),
            hb_socket: ReqSocket::new(),
            iopub_socket: SubSocket::new(),
            ws_json_tx: json_tx,
            ws_json_rx: json_rx,
            ws_zmq_tx: zmq_tx,
            ws_zmq_rx: zmq_rx,
            connection,
            connection_file,
        };

        tokio::spawn(async move {
            let status = child.wait().await.expect("Failed to wait on child process");
            // update the status of the session
            kernel_session.status = models::Status::Exited;
            log::info!(
                "Child process for session {} exited with status: {}",
                session.session_id,
                status
            );
        });

        Ok(kernel_session)
    }

    pub async fn connect(&mut self) -> Result<(), anyhow::Error> {
        self.shell_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.ip, self.connection_file.shell_port
                )
                .as_str(),
            )
            .await?;

        self.hb_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.ip, self.connection_file.hb_port
                )
                .as_str(),
            )
            .await?;

        self.iopub_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection_file.ip, self.connection_file.iopub_port
                )
                .as_str(),
            )
            .await?;

        // Subscribe to all messages
        self.iopub_socket.subscribe("").await?;

        // Wait for a message from any socket
        select! {
            shell_msg = self.shell_socket.recv() => {
                log::info!("Received message from shell socket");
                match shell_msg {
                    Ok(msg) => {
                        log::info!("Received message: {:?}", msg);
                        self.forward_zmq(JupyterChannel::Shell, msg).await?;
                    },
                    Err(e) => {
                        log::error!("Failed to receive message from shell socket: {}", e);
                    },
                }
            },
            _ = self.hb_socket.recv() => {
                log::info!("Received message from heartbeat socket");
            },
            iopub_msg = self.iopub_socket.recv() => {
                match iopub_msg {
                    Ok(msg) => {
                        log::info!("Received message: {:?}", msg);
                        self.forward_zmq(JupyterChannel::IOPub, msg).await?;
                    },
                    Err(e) => {
                        log::error!("Failed to receive message from iopub socket: {}", e);
                    },
                }
            },
            ws_msg = self.ws_zmq_rx.recv() => {
                match ws_msg {
                    Ok(msg) => {
                        match msg.channel {
                            JupyterChannel::Shell => {
                                self.shell_socket.send(msg.message).await?;
                            },
                            _ => {
                                log::error!("Unsupported channel: {:?}", msg.channel);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to receive message from websocket: {}", e);
                    },
                }
            }
        };

        Ok(())
    }

    async fn forward_zmq(
        &mut self,
        channel: JupyterChannel,
        message: ZmqMessage,
    ) -> Result<(), anyhow::Error> {
        let message = WireMessage::from(message);
        let message = message.to_jupyter(channel)?;
        let payload = serde_json::to_string(&message)?;
        match self.ws_json_tx.send(payload).await {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow::anyhow!(
                "Failed to send message to websocket: {}",
                e
            )),
        }
    }
}
