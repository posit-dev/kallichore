//
// session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use std::sync::Arc;

use bytes::Bytes;
use futures::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use hmac::{Hmac, Mac};
use hyper::upgrade::Upgraded;
use kallichore_api::models;
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage};
use sha2::Sha256;
use tokio::{
    select,
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender},
        Mutex,
    },
};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};
use zeromq::SocketSend;

/// Separates ZeroMQ socket identities from the message body payload.
const MSG_DELIM: &[u8] = b"<IDS|MSG>";

use crate::{connection_file, wire_message::WireMessage};
use zeromq::{DealerSocket, ReqSocket, Socket, SocketRecv, SubSocket, ZmqMessage};

struct ZmqChannelMessage {
    channel: JupyterChannel,
    message: ZmqMessage,
}

pub struct KernelSession {
    /// The ID of the session
    pub session_id: String,

    /// The username of the user who owns the session
    pub username: String,

    /// The command line arguments used to start the kernel. The first is the
    /// path to the kernel itself.
    pub argv: Vec<String>,

    /// The process ID of the kernel
    pub process_id: Option<u32>,

    /// The current status of the kernel
    pub status: models::Status,

    /// The connection information for the kernel
    pub connection: connection_file::ConnectionFile,

    /// The websocket used to write to the client
    ws_write: Arc<Mutex<Option<SplitSink<WebSocketStream<Upgraded>, Message>>>>,

    /// The websocket used to read from the client
    ws_read: Arc<Mutex<Option<SplitStream<WebSocketStream<Upgraded>>>>>,

    ws_tx: UnboundedSender<ZmqChannelMessage>,

    ws_rx: UnboundedReceiver<ZmqChannelMessage>,

    /// The HMAC key used to sign messages
    hmac_key: Hmac<Sha256>,

    /// The kernel's shell socket
    shell_socket: DealerSocket,

    /// The kernel's heartbeat socket
    hb_socket: ReqSocket,

    /// The kernel's iopub socket
    iopub_socket: SubSocket,
}

impl KernelSession {
    /// Create a new kernel session.
    pub fn new(session: models::Session, connection: connection_file::ConnectionFile) -> Self {
        // Start the session in a new thread
        let argv = session.argv.clone();

        let mut child = tokio::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(session.working_directory)
            .envs(&session.env)
            .spawn()
            .expect("Failed to start child process");

        // Get the process ID of the child process
        let pid = child.id();

        // Create a new random HMAC key to sign messages for this session
        let hmac_key = Hmac::<Sha256>::new_from_slice(connection.key.as_bytes())
            .expect("Failed to create HMAC key");

        // Add an unbounded MPSC channel to the session
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ZmqChannelMessage>();
        let mut kernel_session = KernelSession {
            session_id: session.session_id.clone(),
            argv: session.argv,
            process_id: pid,
            username: session.username.clone(),
            status: models::Status::Idle,
            shell_socket: DealerSocket::new(),
            hb_socket: ReqSocket::new(),
            iopub_socket: SubSocket::new(),
            ws_read: Arc::new(Mutex::new(None)),
            ws_write: Arc::new(Mutex::new(None)),
            ws_tx: tx,
            ws_rx: rx,
            connection,
            hmac_key,
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

        kernel_session
    }

    pub fn handle_channel_ws(&mut self, ws_stream: WebSocketStream<Upgraded>) {
        let (ws_write, ws_read) = ws_stream.split();
        {
            self.ws_write.blocking_lock().replace(ws_write);
            self.ws_read.blocking_lock().replace(ws_read);
        }

        let session_id = self.session_id.clone();
        let username = self.username.clone();
        let hmac_key = self.hmac_key.clone();
        let ws_read = self.ws_read.clone();
        let ws_tx = self.ws_tx.clone();
        tokio::spawn(async move {
            loop {
                let data = {
                    let mut ws_read = ws_read.lock().await;
                    let ws_read = ws_read.as_mut().unwrap();
                    ws_read.next().await
                };
                let data = match data {
                    Some(data) => match data {
                        Ok(data) => data.into_data(),
                        Err(e) => {
                            log::error!("Failed to read data from websocket: {}", e);
                            return;
                        }
                    },
                    None => {
                        log::error!("No data from websocket");
                        return;
                    }
                };

                // parse the message into a JupyterMessage
                let channel_message = serde_json::from_slice::<JupyterMessage>(&data);

                // if the message is not a Jupyter message, log an error and return
                let channel_message = match channel_message {
                    Ok(channel_message) => channel_message,
                    Err(e) => {
                        log::error!("Failed to parse Jupyter message: {}", e);
                        return;
                    }
                };

                // Log the message ID and type
                log::info!(
                    "Got message {} of type {}",
                    channel_message.header.msg_id.clone(),
                    channel_message.header.msg_type.clone()
                );

                // Convert the message to a wire message
                let channel = channel_message.channel.clone();
                let wire_message = WireMessage::from_jupyter(
                    channel_message,
                    session_id.clone(),
                    username.clone(),
                    hmac_key.clone(),
                )
                .unwrap();

                let mut zmq_mesage = ZmqMessage::from(MSG_DELIM.to_vec());
                for part in wire_message.parts {
                    zmq_mesage.push_back(Bytes::from(part));
                }

                match ws_tx.send(ZmqChannelMessage {
                    channel,
                    message: zmq_mesage,
                }) {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("Failed to send message to ZMQ: {}", e);
                    }
                }
            }
        });
    }

    pub async fn connect(&mut self) -> Result<(), anyhow::Error> {
        self.shell_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection.ip, self.connection.shell_port
                )
                .as_str(),
            )
            .await?;

        self.hb_socket
            .connect(format!("tcp://{}:{}", self.connection.ip, self.connection.hb_port).as_str())
            .await?;

        self.iopub_socket
            .connect(
                format!(
                    "tcp://{}:{}",
                    self.connection.ip, self.connection.iopub_port
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
            ws_msg = self.ws_rx.recv() => {
                match ws_msg {
                    Some(msg) => {
                        match msg.channel {
                            JupyterChannel::Shell => {
                                self.shell_socket.send(msg.message).await?;
                            },
                            _ => {
                                log::error!("Unsupported channel: {:?}", msg.channel);
                            }
                        }
                    }
                    None => {
                        log::error!("Failed to receive message from websocket");
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
        match self.ws_write.lock().await.as_mut() {
            None => {
                log::error!("No websocket stream to forward message to");
                return Ok(());
            }
            Some(ref mut ws_stream) => {
                ws_stream.send(Message::text(payload)).await?;
            }
        }
        Ok(())
    }
}
