//
// main.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Main binary entry point for openapi_client implementation.

#![allow(missing_docs)]

use std::fs::File;

use clap::Parser;

mod client_session;
use log::LevelFilter;
use rand::Rng;
use simplelog::{ColorChoice, CombinedLogger, Config, TermLogger, TerminalMode, WriteLogger};
mod connection_file;
mod error;
mod execution_queue;
mod handshake_socket;
mod heartbeat;
mod jupyter_messages;
mod kernel_connection;
mod kernel_session;
mod kernel_state;
#[cfg(target_os = "linux")]
mod proc_stat;
mod process_tree;
mod registration_file;
mod registration_socket;
mod resource_monitor;
mod server;
mod startup_status;
mod transport;
mod websocket_service;
mod wire_message;
mod wire_message_header;
mod working_dir;
mod zmq_ws_proxy;

use transport::{TransportConfig, TransportError, TransportType};

/// Validate command line arguments for consistency and correctness
fn validate_args(args: &Args) -> Result<(), String> {
    // Get the effective transport type (what will actually be used)
    let effective_transport = determine_transport(args);

    // Check if --port is used with non-TCP transport
    if args.port != 0 && effective_transport != "tcp" {
        return Err(format!(
            "The --port argument can only be used with TCP transport. Current transport: {}. \
            Either remove --port or use --transport tcp.",
            effective_transport
        ));
    }

    // Check if --unix-socket is used with non-socket transport
    #[cfg(unix)]
    if args.unix_socket.is_some() && effective_transport != "socket" {
        return Err(format!(
            "The --unix-socket argument can only be used with socket transport. Current transport: {}. \
            Either remove --unix-socket or use --transport socket.",
            effective_transport
        ));
    }

    // Check for invalid transport types on specific platforms
    #[cfg(windows)]
    if let Some(ref transport) = args.transport {
        if transport == "socket" {
            return Err("Unix domain sockets (--transport socket) are not supported on Windows. Use --transport named-pipe or --transport tcp instead.".to_string());
        }
    }

    #[cfg(unix)]
    if let Some(ref transport) = args.transport {
        if transport == "named-pipe" {
            return Err("Named pipes (--transport named-pipe) are not supported on Unix systems. Use --transport socket or --transport tcp instead.".to_string());
        }
    }

    // Validate that transport type is recognized
    if let Some(ref transport) = args.transport {
        match transport.as_str() {
            "tcp" | "socket" | "named-pipe" => {
                // Valid transport types
            }
            _ => {
                return Err(format!(
                    "Invalid transport type '{}'. Valid values are 'tcp', 'socket' (Unix only), and 'named-pipe' (Windows only).",
                    transport
                ));
            }
        }
    }

    Ok(())
}

fn determine_transport(args: &Args) -> String {
    if let Some(ref transport) = args.transport {
        transport.clone()
    } else {
        // Infer transport from other arguments
        #[cfg(unix)]
        if args.unix_socket.is_some() {
            return "socket".to_string();
        }

        // Default to TCP. The handshake socket's transport is independent of the
        // main transport, so it is never used to infer the transport here.
        "tcp".to_string()
    }
}

/// Create the appropriate transport based on transport type and arguments
async fn create_transport(
    args: &Args,
    transport_type: &str,
) -> Result<TransportType, TransportError> {
    let config = TransportConfig {
        port: args.port,
        #[cfg(unix)]
        unix_socket_path: args.unix_socket.clone(),
        #[cfg(unix)]
        socket_dir: args.socket_dir.clone(),
        #[cfg(windows)]
        named_pipe_name: None, // Will be auto-generated
        #[cfg(unix)]
        server_created: args.unix_socket.is_none(),
        #[cfg(windows)]
        server_created: false,
    };

    TransportType::create(transport_type, config).await
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The port to bind the server to
    #[arg(short, long, default_value_t = 0)]
    port: u16,

    /// The path to a file containing the authentication token, or the special
    /// string "none" to disable authentication. If omitted, a random token will
    /// be generated.
    #[arg(short, long)]
    token: Option<String>,

    /// The path to a log file. If specified, log output will be written to this
    /// file in addition to standard streams.
    #[arg(long)]
    log_file: Option<String>,

    /// Path to a client-owned handshake socket. On Unix this is a filesystem
    /// path to a Unix domain socket the client is already listening on; on
    /// Windows it is the name of a named pipe the client has created (e.g.
    /// \\.\pipe\...). Immediately after binding its main transport, the server
    /// connects to this socket, writes a single JSON document describing the
    /// connection (transport, address, bearer token, server_id, pid, log path),
    /// and closes.
    #[arg(long)]
    handshake_socket: Option<String>,

    /// The number of hours of idle time before the server shuts down. The
    /// server is considered idle if all sessions are idle and no session is
    /// connected. If not specified, the server will not shut down due to
    /// inactivity; if set to 0, the server will shut down after 30 seconds when
    /// idle.
    #[arg(short, long)]
    idle_shutdown_hours: Option<u16>,

    /// The interval in milliseconds at which resource usage is sampled. A value
    /// of 0 disables resource usage sampling. If not specified, defaults to
    /// 1000 ms.
    #[arg(short, long)]
    resource_sample_interval: Option<u16>,

    /// The log level to use. Valid values are "trace", "debug", "info", "warn",
    /// and "error". If not specified, the default log level is "info", or the
    /// value of `RUST_LOG` if set.
    #[arg(short, long)]
    log_level: Option<String>,

    /// The path in which new Unix domain sockets will be created (Unix only).
    /// If not specified, defaults to the XDG runtime directory or the system's
    /// temporary directory.
    #[cfg(unix)]
    #[arg(long)]
    socket_dir: Option<String>,

    /// The path to an existing Unix domain socket to bind the server to (Unix only).
    /// If specified, the server will listen on this socket instead of TCP.
    #[cfg(unix)]
    #[arg(long)]
    unix_socket: Option<String>,

    /// The transport type to use for the main server connection. Valid values
    /// are "tcp", "socket" (Unix only), and "named-pipe" (Windows only).
    /// If not specified, defaults to "socket" when --unix-socket is given,
    /// otherwise "tcp".
    #[arg(long)]
    transport: Option<String>,
}

/// Create custom server, wire it to the autogenerated router,
/// and pass it to the web server.
#[tokio::main]
async fn main() {
    // Parse command line arguments
    let args = Args::parse();

    // Validate the arguments for consistency and correctness
    if let Err(e) = validate_args(&args) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    // Determine the transport type to use
    let transport_type = determine_transport(&args);

    // Derive the log level
    let log_level = match args.log_level {
        Some(ref level) => {
            // If the log level is set in the command-line arguments, use it
            level.to_string()
        }
        None => match std::env::var("RUST_LOG") {
            Ok(level) => {
                // If the log level is set in the RUST_LOG environment variable, use it
                level
            }
            Err(_) => {
                // If no log level is set, use "info"
                "info".to_string()
            }
        },
    };

    // Match the log level to a `LevelFilter`
    let log_level = match log_level.as_str() {
        "trace" => LevelFilter::Trace,
        "debug" => LevelFilter::Debug,
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => {
            println!("Invalid log level '{}'; using 'info'", log_level);
            LevelFilter::Info
        }
    };

    // Check to see if a log file was provided
    match args.log_file {
        Some(ref log_file) => {
            // A log file was provided; use a combined logger that writes to the
            // log file and stdout
            if let Err(err) = CombinedLogger::init(vec![
                TermLogger::new(
                    log_level,
                    Config::default(),
                    TerminalMode::Mixed,
                    ColorChoice::Auto,
                ),
                WriteLogger::new(
                    log_level,
                    Config::default(),
                    File::create(log_file).unwrap(),
                ),
            ]) {
                // Consider it a fatal error if we can't initialize logging
                println!(
                    "Failed to initialize combined file/terminal logging: {}",
                    err
                );
                std::process::exit(1);
            }
        }
        None => {
            // No log file was provided; use a terminal logger only
            if let Err(err) = TermLogger::init(
                log_level,
                Config::default(),
                TerminalMode::Mixed,
                ColorChoice::Auto,
            ) {
                // Consider it a fatal error if we can't initialize logging
                println!("Failed to initialize terminal logging: {}", err);
                std::process::exit(1);
            }
        }
    }

    // Create the appropriate transport based on transport type and arguments
    let transport = match create_transport(&args, &transport_type).await {
        Ok(transport) => transport,
        Err(e) => {
            log::error!("Failed to create transport: {}", e);
            std::process::exit(1);
        }
    };

    // Log connection information
    transport.log_connection_info();

    // See if a token file was provided
    let token = match args.token {
        Some(ref token_file) => {
            if token_file == "none" {
                log::warn!("Authentication was disabled with --token none.");
                None
            } else {
                match std::fs::read_to_string(token_file) {
                    Ok(token) => {
                        // Trim the whitespace from the token
                        let token = token.trim();

                        // Ensure the token isn't longer than 64 characters;
                        // this needs to fit in an HTTP header
                        if token.len() > 64 {
                            log::error!("Auth token is too long (max 64 characters)");
                            std::process::exit(1);
                        }

                        // Attempt to delete the file after reading it; since
                        // the path to the file is visible in the process list,
                        // this is a security measure
                        if let Err(e) = std::fs::remove_file(token_file) {
                            log::warn!("Failed to delete token file '{}': {}", token_file, e);
                        }

                        log::trace!("Using auth token from file");
                        Some(token.to_string())
                    }
                    Err(e) => {
                        log::error!("Failed to read token file '{}': {}", token_file, e);
                        std::process::exit(1);
                    }
                }
            }
        }
        None => {
            // Generate a random token. We use 32 bytes (256 bits) of entropy;
            // hex-encoded this is 64 characters, which is the maximum length
            // that fits in the auth token HTTP header (see the token-file
            // length check above).
            let mut rng = rand::thread_rng();
            let mut hex_string = String::with_capacity(64);

            for _ in 0..32 {
                let byte: u8 = rng.gen();
                hex_string.push_str(&format!("{:02x}", byte));
            }

            // If the token is generated and no handshake socket is specified,
            // log it to the console as there's otherwise no way to retrieve it
            if args.handshake_socket.is_none() {
                log::info!("Generated random auth token: {}", hex_string);
            }

            Some(hex_string)
        }
    };

    println!(
        r#"
  ,            _   _           _
 /|   /       | | | | o       | |
  |__/   __,  | | | |     __  | |     __   ,_    _
  | \   /  |  |/  |/  |  /    |/ \   /  \_/  |  |/
  |  \_/\_/|_/|__/|__/|_/\___/|   |_/\__/    |_/|__/
  A Jupyter Kernel supervisor. Version {}.
  Copyright (c) 2026, Posit Software PBC. All rights reserved.
"#,
        env!("CARGO_PKG_VERSION")
    );

    // Get connection info for the handshake and main server socket before consuming transport
    let connection_info = transport.to_server_connection_type();
    let main_server_socket = transport.main_server_socket();

    // Generate the server ID up front so we can report it in the handshake. The
    // same ID is threaded into the server so the value reported here matches the
    // one later returned from the status endpoint, which the client uses to
    // detect a replaced server.
    let server_id = uuid::Uuid::new_v4().to_string();

    // If a handshake socket path was specified, connect to it and report the
    // connection details. The transport's listener is already bound, so the
    // reported address is valid; connections the client makes immediately after
    // the handshake queue in the OS backlog until we start accepting below.
    if let Some(handshake_path) = &args.handshake_socket {
        let payload = match handshake_socket::HandshakePayload::new(
            &connection_info,
            &token,
            &args.log_file,
            &server_id,
        ) {
            Ok(payload) => payload,
            Err(e) => {
                log::error!("Failed to build handshake payload: {}", e);
                std::process::exit(1);
            }
        };

        if let Err(e) = handshake_socket::perform_handshake(handshake_path, &payload).await {
            log::error!(
                "Failed to report connection details over handshake socket '{}': {}",
                handshake_path,
                e
            );
            std::process::exit(1);
        }
        log::info!("Reported connection details over handshake socket {}", handshake_path);
    }

    log::debug!("Starting Kallichore");

    // Convert the transport to a server listener
    let server_listener = transport.into_server_listener();

    // Determine the resource sample interval (default to 1000ms if not specified)
    let resource_sample_interval_ms = args.resource_sample_interval.unwrap_or(1000) as u64;

    // Pass the listener to the server
    server::create_with_listener(
        server_listener,
        server_id,
        token,
        args.idle_shutdown_hours,
        args.log_level,
        #[cfg(unix)]
        args.socket_dir,
        main_server_socket,
        resource_sample_interval_ms,
    )
    .await;
}
