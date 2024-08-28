//
// session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use std::sync::Arc;

use bytes::Bytes;
use futures::{stream::SplitSink, StreamExt};
use hmac::{Hmac, Mac};
use hyper::upgrade::Upgraded;
use kallichore_api::models;
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage};
use sha2::Sha256;
use tokio::sync::Mutex;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

/// Separates ZeroMQ socket identities from the message body payload.
const MSG_DELIM: &[u8] = b"<IDS|MSG>";

use crate::{connection_file, socket_forwarder, wire_message::WireMessage};
use zeromq::{DealerSocket, ReqSocket, Socket, SocketRecv, SocketSend, SubSocket, ZmqMessage};

#[derive(Clone)]
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

    /// The HMAC key used to sign messages
    hmac_key: Hmac<Sha256>,

    /// The kernel's shell socket
    shell_socket: Arc<Mutex<DealerSocket>>,

    /// The kernel's heartbeat socket
    hb_socket: Arc<Mutex<ReqSocket>>,

    /// The kernel's iopub socket
    iopub_socket: Arc<Mutex<SubSocket>>,
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

        let mut kernel_session = KernelSession {
            session_id: session.session_id.clone(),
            argv: session.argv,
            process_id: pid,
            username: session.username.clone(),
            status: models::Status::Idle,
            shell_socket: Arc::new(Mutex::new(DealerSocket::new())),
            hb_socket: Arc::new(Mutex::new(ReqSocket::new())),
            iopub_socket: Arc::new(Mutex::new(SubSocket::new())),
            ws_write: Arc::new(Mutex::new(None)),
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
        let (write, read) = ws_stream.split();
        self.ws_write.try_lock().unwrap().replace(write);

        let session_id = self.session_id.clone();
        let username = self.username.clone();
        let hmac_key = self.hmac_key.clone();
        let shell_socket = self.shell_socket.clone();
        // Write some test data to the websocket
        tokio::spawn(async move {
            read.for_each(|message| async {
                let data = match message {
                    Ok(message) => message.into_data(),
                    Err(e) => {
                        // This is normal when the websocket is closed by the
                        // client without sending a close frame
                        log::info!(
                            "Failed to read message from websocket: {} (presuming disconnect)",
                            e
                        );
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
                    channel_message.header.msg_id,
                    channel_message.header.msg_type
                );

                // Convert the message to a wire message
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

                // Unlock the shell socket and send the message
                let mut socket = shell_socket.lock().await;
                socket.send(zmq_mesage).await.unwrap();
            })
            .await;
        });
    }

    pub fn connect(&self) {
        // Connect to the shell socket
        log::info!(
            "Connecting to kernel shell at {}:{}",
            self.connection.ip,
            self.connection.shell_port
        );

        let shell_socket = self.shell_socket.clone();
        let connection = self.connection.clone();
        let ws_write = self.ws_write.clone();

        // Attempt to connect to the kernel's shell socket using zeromq
        tokio::spawn(async move {
            {
                let mut socket = shell_socket.lock().await;
                socket
                    .connect(format!("tcp://{}:{}", connection.ip, connection.shell_port).as_str())
                    .await
                    .unwrap();
            }

            socket_forwarder::socket_forwarder(JupyterChannel::Shell, shell_socket, ws_write).await;
        });

        let hb_socket = self.hb_socket.clone();
        let connection = self.connection.clone();
        tokio::spawn(async move {
            let mut socket = hb_socket.lock().await;
            log::info!(
                "Connecting to kernel heartbeat at {}:{}",
                connection.ip,
                connection.hb_port
            );
            socket
                .connect(format!("tcp://{}:{}", connection.ip, connection.hb_port).as_str())
                .await
                .unwrap();
            log::info!(
                "Connected to kernel heartbeat at {}:{}; sending 'Hello'",
                connection.ip,
                connection.hb_port
            );
            socket.send("Hello".into()).await.unwrap();
            let repl = socket.recv().await.unwrap();
            log::info!("Received reply: {:?}", repl);
        });

        // Attempt to connect to the kernel's iopub socket using zeromq
        let iopub_socket = self.iopub_socket.clone();
        let connection = self.connection.clone();
        let ws_write = self.ws_write.clone();
        tokio::spawn(async move {
            {
                let mut socket = iopub_socket.lock().await;
                socket
                    .connect(format!("tcp://{}:{}", connection.ip, connection.iopub_port).as_str())
                    .await
                    .unwrap();
                // Subscribe to all messages
                socket.subscribe("").await.unwrap();
            }

            socket_forwarder::socket_forwarder(JupyterChannel::IOPub, iopub_socket, ws_write).await;
        });
    }
}
