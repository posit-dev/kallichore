//
// main.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! kcclient
//!
//! Kallichore Client
#![allow(missing_docs, unused_variables, trivial_casts)]

use directories::BaseDirs;
#[allow(unused_imports)]
use futures::{future, stream, SinkExt, Stream};
use kallichore_api::NewSessionResponse;
#[allow(unused_imports)]
use kallichore_api::{models, Api, ApiNoContext, Client, ContextWrapperExt, ListSessionsResponse};

use kcshared::{
    jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader},
    websocket_message::WebsocketMessage,
};
#[allow(unused_imports)]
use log::info;

use clap::{Parser, Subcommand};

use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use futures::stream::StreamExt;

mod kernel_spec;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Optional URL to use as the base for API requests
    #[arg(
        short,
        long,
        value_name = "URL",
        default_value_t = String::from("http://localhost:8182")
    )]
    url: String,

    /// Subcommands
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Lists active sessions
    List,

    /// Start a new session
    Start {
        /// The previously registered kernel to use
        #[arg(short, long)]
        kernel: String,
    },

    /// Get kernel info on a to a running session
    Info {
        /// The session to get info for. Optional; if not provided, the first
        /// running session will be used
        #[arg(short, long)]
        session_id: Option<String>,
    },

    /// Execute code in a running session
    Execute {
        /// The session to execute code in. Optional; if not provided, the first
        /// running session will be used
        #[arg(short, long)]
        session_id: Option<String>,

        /// The code to execute
        #[arg(short, long)]
        code: String,
    },

    /// Shut down a running session
    Shutdown {
        /// The session to shut down. Optional; if not provided, the first
        /// running session will be used
        #[arg(short, long)]
        session_id: Option<String>,
    },
}

use log::{debug, trace};
// swagger::Has may be unused if there are no examples
#[allow(unused_imports)]
use swagger::{AuthData, ContextBuilder, EmptyContext, Has, Push, XSpanIdString};

type ClientContext = swagger::make_context_ty!(
    ContextBuilder,
    EmptyContext,
    Option<AuthData>,
    XSpanIdString
);

async fn connect_to_session(
    url: String,
    session_id: String,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Box<dyn std::error::Error>> {
    // extract the host from the url
    let url = url.parse::<url::Url>().expect("Failed to parse URL");
    let authority = url.authority();
    // form a URL to the websocket endpoint
    let ws_url = format!("ws://{}/sessions/{}/channels", authority, session_id);
    info!("Connecting to {}", ws_url);
    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    info!("WebSocket handshake has been successfully completed");
    Ok(ws_stream)
}

async fn request_shutdown(ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>) {
    let (mut write, read) = ws_stream.split();
    let message = JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: "CB2B8818-E5EB-419F-A046-23DD3B45E1BD".to_string(),
            msg_type: "shutdown_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Control,
        content: serde_json::json!({
            "restart": false
        }),
        buffers: Vec::new(),
        metadata: serde_json::json!({}),
    };
    write
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&message).unwrap(),
        ))
        .await
        .expect("Failed to send message");
}

async fn get_kernel_info(ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>) {
    let (mut write, mut read) = ws_stream.split();

    let message = JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: "7C857F22-013E-4ECD-89ED-9A1E6BAA0F98".to_string(),
            msg_type: "kernel_info_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: serde_json::json!({}),
        buffers: Vec::new(),
        metadata: serde_json::json!({}),
    };

    // Write the message to the websocket
    write
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&message).unwrap(),
        ))
        .await
        .expect("Failed to send message");

    // Wait for a kernel info reply
    loop {
        match read.next().await {
            Some(message) => {
                let data = message.unwrap().into_data();
                let payload = String::from_utf8_lossy(&data);
                let message = serde_json::from_str::<WebsocketMessage>(&payload).unwrap();
                match message {
                    WebsocketMessage::Jupyter(message) => {
                        if message.header.msg_type == "kernel_info_reply" {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&message.content).unwrap()
                            );
                            break;
                        }
                    }
                    _ => {
                        debug!("Ignoring message {:?}", message);
                    }
                }
            }
            None => {
                debug!("No message received");
                break;
            }
        }
    }

    write.close().await.expect("Failed to close connection");
}

// rt may be unused if there are no examples
#[allow(unused_mut)]
fn main() {
    env_logger::init();

    // Read command line arguments
    let args = Args::parse();

    let context: ClientContext = swagger::make_context!(
        ContextBuilder,
        EmptyContext,
        None as Option<AuthData>,
        XSpanIdString::default()
    );
    let base_url = args.url;
    let client = Box::new(Client::try_new_http(&base_url).expect("Failed to create HTTP client"));
    let client = Box::new(client.with_context(context));

    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let session_id = format!("{:08x}", rand::random::<u32>());

    let working_directory = std::env::current_dir().unwrap();

    // Use the command froom the command line arguments to determine which operation to perform
    match args.command {
        Some(Commands::List) => {
            let result = rt.block_on(client.list_sessions());
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
            // If the result is successful, pretty-print the list of sessions as JSON
            if let Ok(ListSessionsResponse::ListOfActiveSessions(sessions)) = result {
                println!("{}", serde_json::to_string_pretty(&sessions).unwrap());
            }
        }
        Some(Commands::Start { kernel }) => {
            let base_dir = BaseDirs::new().unwrap();

            // On macOS, Jupyter doens't follow XDG Base Directory
            // Specification; it stores its data in `~/Library/Jupyter` instead
            // of the "correct" XDG location in `~/Library/Application Support`.
            #[cfg(target_os = "macos")]
            let mut jupyter_dir = base_dir.home_dir().join("Library").join("Jupyter");

            #[cfg(not(target_os = "macos"))]
            let mut jupyter_dir = directories::ProjectDirs::from("Jupyter", "", "")
                .unwrap()
                .data_dir();

            let kernel_spec_json = jupyter_dir
                .join("kernels")
                .join(kernel.clone())
                .join("kernel.json");

            // Parse the kernel spec from the JSON file using the serde json library
            let kernel_spec: kernel_spec::KernelSpec =
                serde_json::from_reader(std::fs::File::open(kernel_spec_json).unwrap())
                    .expect("Failed to parse kernel spec");

            // Convert the environment variables from the kernel spec to a
            // HashMap for use in the session
            let mut env = std::collections::HashMap::new();
            for (key, value) in kernel_spec.env.iter() {
                trace!(
                    "Session {}: Setting env var {}={}",
                    session_id.clone(),
                    key,
                    value
                );
                if value.is_string() {
                    let value = value.as_str().unwrap();
                    env.insert(key.clone(), value.to_string());
                }
            }

            let session = models::Session {
                session_id: session_id.clone(),
                argv: kernel_spec.argv,
                username: String::from("testuser"),
                working_directory: working_directory.to_string_lossy().to_string(),
                env,
            };
            info!(
                "Creating new session for '{}' kernel ({}) with id {}",
                kernel,
                kernel_spec.display_name,
                session.session_id.clone()
            );

            let result = rt.block_on(client.new_session(session));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
            // Pretty print the result as JSON
            match result {
                Ok(NewSessionResponse::TheSessionID(session_id)) => {
                    println!("{}", serde_json::to_string_pretty(&session_id).unwrap());
                }
                Ok(NewSessionResponse::InvalidRequest(error)) => {
                    println!("{}", serde_json::to_string_pretty(&error).unwrap());
                }
                _ => {}
            }

            // Start the new session
            let result = rt.block_on(client.start_session(session_id.clone()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
        }
        Some(Commands::Execute { session_id, code }) => {
            // TODO
        }
        Some(Commands::Info { session_id }) => {
            let session_id = match session_id {
                Some(session_id) => session_id,
                None => {
                    let result = rt.block_on(client.list_sessions());
                    if let Ok(ListSessionsResponse::ListOfActiveSessions(sessions)) = result {
                        if let Some(session) = sessions.sessions.first() {
                            session.session_id.clone()
                        } else {
                            eprintln!("No sessions available to connect to");
                            return;
                        }
                    } else {
                        eprintln!("Failed to list sessions");
                        return;
                    }
                }
            };
            log::info!("Getting kernel info from  session '{}'", session_id);
            let ws_stream = rt
                .block_on(connect_to_session(base_url, session_id))
                .unwrap();
            rt.block_on(get_kernel_info(ws_stream));
        }
        Some(Commands::Shutdown { session_id }) => {
            let session_id = match session_id {
                Some(session_id) => session_id,
                None => {
                    let result = rt.block_on(client.list_sessions());
                    if let Ok(ListSessionsResponse::ListOfActiveSessions(sessions)) = result {
                        if let Some(session) = sessions.sessions.first() {
                            session.session_id.clone()
                        } else {
                            eprintln!("No sessions available to shut down");
                            return;
                        }
                    } else {
                        eprintln!("Failed to list sessions");
                        return;
                    }
                }
            };
            log::info!("Shutting down session '{}'", session_id.clone());
            let ws_stream = rt.block_on(connect_to_session(base_url, session_id.clone()));
            rt.block_on(request_shutdown(ws_stream.unwrap()));
            println!("Shutdown requested for session {} ", session_id);
        }
        None => {
            eprintln!("No command specified");
        }
    }
}
