//
// session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use kallichore_api::models;
use kcshared::jupyter_message::JupyterMessage;
use tokio::sync::Mutex;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::connection_file;
use zeromq::{DealerSocket, ReqSocket, Socket, SocketRecv, SocketSend};

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

    // The kernel's shell socket
    shell_socket: Arc<Mutex<DealerSocket>>,

    // The kernel's heartbeat socket
    hb_socket: Arc<Mutex<ReqSocket>>,
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

        let mut kernel_session = KernelSession {
            session_id: session.session_id.clone(),
            argv: session.argv,
            process_id: pid,
            username: session.username.clone(),
            status: models::Status::Idle,
            shell_socket: Arc::new(Mutex::new(DealerSocket::new())),
            hb_socket: Arc::new(Mutex::new(ReqSocket::new())),
            connection,
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

    pub fn handle_channel_ws(&self, ws_stream: WebSocketStream<Upgraded>) {
        let (write, read) = ws_stream.split();
        let write = Mutex::new(write);

        // Write some test data to the websocket
        tokio::spawn(async move {
            read.for_each(|message| async {
                let data = message.unwrap().into_data();

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

                // acknowledge the message; TODO: write to the correct socket
                write
                    .lock()
                    .await
                    .send(Message::text(format!(
                        "got message {}",
                        channel_message.header.msg_id
                    )))
                    .await
                    .unwrap();
                print!("{}", String::from_utf8_lossy(&data));
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

        // Attempt to connect to the kernel using zeromq
        tokio::spawn(async move {
            let mut socket = shell_socket.lock().await;
            socket
                .connect(format!("tcp://{}:{}", connection.ip, connection.shell_port).as_str())
                .await
                .unwrap();
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
    }
}
