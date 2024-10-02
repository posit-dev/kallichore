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

use std::path::PathBuf;

use directories::BaseDirs;
#[allow(unused_imports)]
use futures::{future, stream, SinkExt, Stream};
#[allow(unused_imports)]
use kallichore_api::{models, Api, ApiNoContext, Client, ContextWrapperExt, ListSessionsResponse};
use kallichore_api::{
    DeleteSessionResponse, InterruptSessionResponse, NewSessionResponse, RestartSessionResponse,
};

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

        /// Whether to wait for execution to finish
        #[arg(short, long)]
        wait: bool,
    },

    /// Restart a running session
    Restart {
        /// The session to restart. Optional; if not provided, the first
        /// running session will be used
        #[arg(short, long)]
        session_id: Option<String>,
    },

    /// Listen to events from a running session
    Listen {
        /// The session to interrupt. Optional; if not provided, the first
        /// running session will be used
        #[arg(short, long)]
        session_id: Option<String>,
    },

    /// Shut down a running session, or all sessions (exiting the server)
    Shutdown {
        /// The session to shut down.
        #[arg(short, long)]
        session_id: Option<String>,
    },

    /// Interrupt a running session
    Interrupt {
        /// The session to interrupt. Optional; if not provided, the first
        /// running session will be used
        #[arg(short, long)]
        session_id: Option<String>,
    },

    /// Kill a running session
    Kill {
        /// The session to shut kill. Optional; if not provided, the first
        /// running session will be used
        #[arg(short, long)]
        session_id: Option<String>,
    },

    /// Delete an exited session
    Delete {
        /// The session to delete.
        #[arg(short, long)]
        session_id: String,
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
            msg_id: uuid::Uuid::new_v4().to_string(),
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

async fn listen(ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>) {
    let (mut _write, mut read) = ws_stream.split();
    loop {
        match read.next().await {
            Some(message) => {
                let data = message.unwrap().into_data();
                let payload = String::from_utf8_lossy(&data);
                let message = serde_json::from_str::<WebsocketMessage>(&payload).unwrap();
                // print a timestamp
                println!(
                    "[{}] {}",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                    serde_json::to_string_pretty(&message).unwrap()
                );
            }
            None => {
                debug!("No message received");
                break;
            }
        }
    }
}
async fn get_kernel_info(ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>) {
    let (mut write, mut read) = ws_stream.split();

    let message = JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: uuid::Uuid::new_v4().to_string(),
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

async fn execute_request(
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    code: String,
    wait: bool,
) {
    let (mut write, mut read) = ws_stream.split();

    let request = JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: uuid::Uuid::new_v4().to_string(),
            msg_type: "execute_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: serde_json::json!({
            "code": code,
            "silent": false,
            "store_history": true,
            "user_expressions": {},
            "allow_stdin": true,
            "stop_on_error": true,
        }),
        buffers: Vec::new(),
        metadata: serde_json::json!({}),
    };

    // Write the message to the websocket
    write
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&request).unwrap(),
        ))
        .await
        .expect("Failed to send message");

    // If we're not waiting for the result, return early
    if !wait {
        return;
    }

    // Wait for a reply to the execute request
    loop {
        match read.next().await {
            Some(raw_message) => {
                let data = raw_message.unwrap().into_data();
                let payload = String::from_utf8_lossy(&data);
                let ws_message = serde_json::from_str::<WebsocketMessage>(&payload).unwrap();
                match ws_message {
                    WebsocketMessage::Jupyter(jupyter_message) => {
                        if jupyter_message.parent_header.is_some()
                            && jupyter_message.parent_header.unwrap().msg_id
                                == request.header.msg_id
                        {
                            println!(
                                "--- {:?}: {} ---\n{}",
                                jupyter_message.channel,
                                jupyter_message.header.msg_type,
                                serde_json::to_string_pretty(&jupyter_message.content).unwrap()
                            );

                            // Stop listening when the kernel becomes idle
                            if jupyter_message.header.msg_type == "status"
                                && jupyter_message.content["execution_state"] == "idle"
                            {
                                break;
                            }
                        }
                    }
                    _ => {
                        debug!("Ignoring message {:?}", ws_message);
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

#[cfg(target_os = "macos")]
fn jupyter_dir() -> PathBuf {
    // On macOS, Jupyter doens't follow the XDG Base Directory
    // Specification; it stores its data in `~/Library/Jupyter` instead
    // of the "correct" XDG location in `~/Library/Application Support`.
    let base_dir = BaseDirs::new().unwrap();
    base_dir.home_dir().join("Library").join("Jupyter")
}

#[cfg(not(target_os = "macos"))]
fn jupyter_dir() -> PathBuf {
    let dir = directories::ProjectDirs::from("Jupyter", "", "").unwrap();
    dir.data_dir().to_path_buf()
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

            let kernel_spec_json = jupyter_dir()
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
            if kernel_spec.env.is_some() {
                let kernel_env = kernel_spec.env.as_ref().unwrap();
                for (key, value) in kernel_env.iter() {
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
            }

            let session = models::NewSession {
                session_id: session_id.clone(),
                argv: kernel_spec.argv,
                display_name: format!("{} - {}", kernel_spec.display_name, session_id.clone()),
                language: kernel_spec.language,
                username: String::from("testuser"),
                input_prompt: String::from("In> "),
                continuation_prompt: String::from("..."),
                working_directory: working_directory.to_string_lossy().to_string(),
                env,
                interrupt_mode: models::InterruptMode::Message,
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
        Some(Commands::Execute {
            session_id,
            code,
            wait,
        }) => {
            let session_id = match session_id {
                Some(session_id) => session_id,
                None => {
                    let result = rt.block_on(client.list_sessions());
                    if let Ok(ListSessionsResponse::ListOfActiveSessions(sessions)) = result {
                        if let Some(session) = sessions.sessions.first() {
                            session.session_id.clone()
                        } else {
                            eprintln!("No sessions available to execute code");
                            return;
                        }
                    } else {
                        eprintln!("Failed to list sessions");
                        return;
                    }
                }
            };
            let ws_stream = rt
                .block_on(connect_to_session(base_url, session_id))
                .unwrap();
            rt.block_on(execute_request(ws_stream, code, wait));
        }
        Some(Commands::Kill { session_id }) => {
            let session_id = match session_id {
                Some(session_id) => session_id,
                None => {
                    let result = rt.block_on(client.list_sessions());
                    if let Ok(ListSessionsResponse::ListOfActiveSessions(sessions)) = result {
                        if let Some(session) = sessions.sessions.first() {
                            session.session_id.clone()
                        } else {
                            eprintln!("No sessions available to kill");
                            return;
                        }
                    } else {
                        eprintln!("Failed to list sessions");
                        return;
                    }
                }
            };
            let result = rt.block_on(client.kill_session(session_id.clone()));
            info!(
                "{:?} (X-Span-ID: {:?})",
                result,
                (client.context() as &dyn Has<XSpanIdString>).get().clone()
            );
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
            // Get details for the named session
            let details = rt.block_on(client.get_session(session_id.clone()));
            match details {
                Ok(details) => {
                    println!("{}", serde_json::to_string_pretty(&details).unwrap());
                }
                Err(e) => {
                    eprintln!("Failed to get session details: {:?}", e);
                }
            }
            log::info!("Getting kernel info from  session '{}'", session_id);
            let ws_stream = rt
                .block_on(connect_to_session(base_url, session_id))
                .unwrap();
            rt.block_on(get_kernel_info(ws_stream));
        }
        Some(Commands::Shutdown { session_id }) => {
            match session_id {
                Some(session_id) => {
                    log::info!("Shutting down session '{}'", session_id.clone());
                    let ws_stream = rt.block_on(connect_to_session(base_url, session_id.clone()));
                    rt.block_on(request_shutdown(ws_stream.unwrap()));
                    println!("Shutdown requested for session {} ", session_id);
                }
                None => {
                    let result = rt.block_on(client.shutdown_server());
                    println!("Server shutdown requested");
                }
            };
        }
        Some(Commands::Listen { session_id }) => {
            let session_id = match session_id {
                Some(session_id) => session_id,
                None => {
                    let result = rt.block_on(client.list_sessions());
                    if let Ok(ListSessionsResponse::ListOfActiveSessions(sessions)) = result {
                        if let Some(session) = sessions.sessions.first() {
                            session.session_id.clone()
                        } else {
                            eprintln!("No sessions available to listen to");
                            return;
                        }
                    } else {
                        eprintln!("Failed to list sessions");
                        return;
                    }
                }
            };
            let ws_stream = rt
                .block_on(connect_to_session(base_url, session_id.clone()))
                .unwrap();
            println!("Listening to session {} (^C to exit)", session_id.clone());
            rt.block_on(listen(ws_stream))
        }
        Some(Commands::Restart { session_id }) => {
            let session_id = match session_id {
                Some(session_id) => session_id,
                None => {
                    let result = rt.block_on(client.list_sessions());
                    if let Ok(ListSessionsResponse::ListOfActiveSessions(sessions)) = result {
                        if let Some(session) = sessions.sessions.first() {
                            session.session_id.clone()
                        } else {
                            eprintln!("No sessions available to restart");
                            return;
                        }
                    } else {
                        eprintln!("Failed to list sessions");
                        return;
                    }
                }
            };
            log::info!("Restarting session '{}'", session_id.clone());
            match rt.block_on(client.restart_session(session_id.clone())) {
                Ok(resp) => match resp {
                    RestartSessionResponse::Restarted(_) => {
                        println!("Session {} restarted", session_id);
                    }
                    RestartSessionResponse::SessionNotFound => {
                        println!("Session {} doesn't exist", session_id);
                    }
                    RestartSessionResponse::RestartFailed(error) => {
                        println!("{}", serde_json::to_string_pretty(&error).unwrap());
                    }
                    RestartSessionResponse::AccessTokenIsMissingOrInvalid => {
                        println!("Access token is missing or invalid");
                    }
                },
                Err(e) => {
                    eprintln!("Failed to restart session: {:?}", e);
                }
            }
        }
        Some(Commands::Delete { session_id }) => {
            log::info!("Deleting session '{}'", session_id.clone());
            match rt.block_on(client.delete_session(session_id.clone())) {
                Ok(resp) => match resp {
                    DeleteSessionResponse::SessionDeleted(_) => {
                        println!("Session {} deleted", session_id);
                    }
                    DeleteSessionResponse::SessionNotFound => {
                        println!("Session {} doesn't exist", session_id);
                    }
                    DeleteSessionResponse::FailedToDeleteSession(error) => {
                        println!("Failed to delete session {}: {:?}", session_id, error);
                    }
                    DeleteSessionResponse::AccessTokenIsMissingOrInvalid => {
                        println!("Access token is missing or invalid");
                    }
                },
                Err(e) => {
                    eprintln!("Failed to delete session: {:?}", e);
                }
            }
        }
        Some(Commands::Interrupt { session_id }) => {
            let session_id = match session_id {
                Some(session_id) => session_id,
                None => {
                    let result = rt.block_on(client.list_sessions());
                    if let Ok(ListSessionsResponse::ListOfActiveSessions(sessions)) = result {
                        if let Some(session) = sessions.sessions.first() {
                            session.session_id.clone()
                        } else {
                            eprintln!("No sessions available to interrupt");
                            return;
                        }
                    } else {
                        eprintln!("Failed to list sessions");
                        return;
                    }
                }
            };
            log::info!("Interrupting session '{}'", session_id.clone());
            match rt.block_on(client.interrupt_session(session_id.clone())) {
                Ok(resp) => match resp {
                    InterruptSessionResponse::Interrupted(_) => {
                        println!("Session {} interrupted", session_id);
                    }
                    InterruptSessionResponse::SessionNotFound => {
                        println!("Session {} doesn't exist", session_id);
                    }
                    InterruptSessionResponse::InterruptFailed(error) => {
                        println!("{}", serde_json::to_string_pretty(&error).unwrap());
                    }
                    InterruptSessionResponse::AccessTokenIsMissingOrInvalid => {
                        println!("Access token is missing or invalid");
                    }
                },
                Err(e) => {
                    eprintln!("Failed to interrupt session: {:?}", e);
                }
            }
        }
        None => {
            eprintln!("No command specified");
        }
    }
}
