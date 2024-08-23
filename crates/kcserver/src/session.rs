//
// session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use futures::{SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use kallichore_api::models;
use kcshared::jupyter_message::JupyterMessage;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::connection_file;
use zeromq::{Socket, SocketRecv, SocketSend};

pub struct KernelSession {
    /// The ID of the session
    pub session_id: String,

    /// The command line arguments used to start the kernel. The first is the
    /// path to the kernel itself.
    pub argv: Vec<String>,

    /// The process ID of the kernel
    pub process_id: Option<u32>,

    /// The current status of the kernel
    pub status: models::Status,
}

impl KernelSession {
    /// Create a new kernel session.
    pub fn new(session: models::Session, connection: connection_file::ConnectionFile) -> Self {
        // Start the session in a new thread
        let argv = session.argv.clone();

        // Attempt to connect to the kernel using zeromq
        tokio::spawn(async move {
            // Connect to the shell socket
            log::info!(
                "Connecting to kernel shell at {}:{}",
                connection.ip,
                connection.shell_port
            );
            let mut socket = zeromq::DealerSocket::new();
            socket
                .connect(format!("tcp://{}:{}", connection.ip, connection.shell_port).as_str())
                .await
                .unwrap();

            // Connect to the heartbeat socket
            let mut socket = zeromq::ReqSocket::new();
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
            status: models::Status::Idle,
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
        let write = tokio::sync::Mutex::new(write);

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
                    .blocking_lock()
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
}
