//
// main.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

//! Main binary entry point for openapi_client implementation.

#![allow(missing_docs)]

use std::fs::File;

use clap::{command, Parser};

mod client_session;
use log::LevelFilter;
use rand::Rng;
use simplelog::{ColorChoice, CombinedLogger, Config, TermLogger, TerminalMode, WriteLogger};
mod connection_file;
mod error;
mod execution_queue;
mod heartbeat;
mod jupyter_messages;
mod kernel_connection;
mod kernel_session;
mod kernel_state;
#[cfg(windows)]
mod named_pipe_connection;
mod registration_file;
mod registration_socket;
mod server;
mod startup_status;
mod transport;
mod websocket_service;
mod wire_message;
mod wire_message_header;
mod working_dir;
mod zmq_ws_proxy;

use transport::{ServerConnectionType, TransportConfig, TransportError, TransportType};

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

        if args.connection_file.is_some() {
            // Default to socket/named-pipe when using connection file
            #[cfg(unix)]
            {
                "socket".to_string()
            }
            #[cfg(windows)]
            {
                "named-pipe".to_string()
            }
            #[cfg(not(any(unix, windows)))]
            {
                "tcp".to_string()
            }
        } else {
            "tcp".to_string()
        }
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

    /// The path to a connection file. If specified, the server will write
    /// connection details to the given file, choosing any options not specified
    /// in the command line arguments (e.g., port, transport type).
    #[arg(long)]
    connection_file: Option<String>,

    /// The number of hours of idle time before the server shuts down. The
    /// server is considered idle if all sessions are idle and no session is
    /// connected. If not specified, the server will not shut down due to
    /// inactivity; if set to 0, the server will shut down after 30 seconds when
    /// idle.
    #[arg(short, long)]
    idle_shutdown_hours: Option<u16>,

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

    /// The transport type to use when creating a connection file. Valid values
    /// are "tcp", "socket" (Unix only), and "named-pipe" (Windows only).
    /// If not specified, defaults to "socket" on Unix and "named-pipe" on Windows
    /// when using --connection-file, otherwise "tcp".
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
            // Generate a random token
            let mut rng = rand::thread_rng();
            let mut hex_string = String::with_capacity(32);

            for _ in 0..8 {
                let byte: u8 = rng.gen();
                hex_string.push_str(&format!("{:02x}", byte));
            }

            // Log the generated token for debugging purposes
            log::info!("Generated random auth token: {}", hex_string);

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
  Copyright (c) 2025, Posit Software PBC. All rights reserved.
"#,
        env!("CARGO_PKG_VERSION")
    );

    // Display connection information - already handled by transport.log_connection_info()

    // Get connection info for file writing and main server socket before consuming transport
    let connection_info = transport.to_server_connection_type();
    let main_server_socket = transport.main_server_socket();

    // If a connection file path was specified, write the connection details to it
    if let Some(connection_file_path) = &args.connection_file {
        if let Err(e) = write_server_connection_file_new(
            connection_file_path,
            &connection_info,
            &token,
            &args.log_file,
        ) {
            log::error!("Failed to write connection file: {}", e);
            std::process::exit(1);
        }
        log::info!("Wrote connection details to {}", connection_file_path);
    }

    log::debug!("Starting Kallichore");

    // Convert the transport to a server listener
    let server_listener = transport.into_server_listener();

    // Pass the listener to the server
    server::create_with_listener(
        server_listener,
        token,
        args.idle_shutdown_hours,
        args.log_level,
        #[cfg(unix)]
        args.socket_dir,
        main_server_socket,
    )
    .await;
}

/// Write server connection details to a file
#[allow(dead_code)]
fn write_server_connection_file(
    path: &str,
    port: u16,
    token: &Option<String>,
    log_file: &Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use serde::{Deserialize, Serialize};
    use std::fs::File;
    use std::io::Write;

    #[derive(Serialize, Deserialize)]
    struct ServerConnectionInfo {
        /// The port the server is listening on
        port: u16,

        /// The full API basepath, starting with 'http'
        base_path: String,

        /// The path to the server executable (this process)
        server_path: String,

        /// The PID of the server process
        server_pid: u32,

        /// The authentication token, if any
        bearer_token: Option<String>,

        /// The path to the log file, if any
        log_path: Option<String>,
    }

    // Get the server path
    let server_path = std::env::current_exe()?
        .to_str()
        .ok_or("Failed to convert server path to string")?
        .to_string();

    // Get the server PID
    let server_pid = std::process::id();

    // Create the connection info struct
    let connection_info = ServerConnectionInfo {
        port,
        base_path: format!("http://127.0.0.1:{}", port),
        server_path,
        server_pid,
        bearer_token: token.clone(),
        log_path: log_file.clone(),
    };

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&connection_info)?;

    // Write to file
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;

    Ok(())
}

/// Write server connection details to a file (new format supporting multiple transports)
fn write_server_connection_file_new(
    path: &str,
    connection_info: &ServerConnectionType,
    token: &Option<String>,
    log_file: &Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use serde::{Deserialize, Serialize};
    use std::fs::File;
    use std::io::Write;

    #[derive(Serialize, Deserialize)]
    struct ServerConnectionInfoNew {
        /// The port the server is listening on (TCP only)
        #[serde(skip_serializing_if = "Option::is_none")]
        port: Option<u16>,

        /// The full API basepath, starting with 'http' (TCP only)
        #[serde(skip_serializing_if = "Option::is_none")]
        base_path: Option<String>,

        /// The path to the Unix domain socket (Unix only)
        #[serde(skip_serializing_if = "Option::is_none")]
        socket_path: Option<String>,

        /// The named pipe path (Windows only)
        #[serde(skip_serializing_if = "Option::is_none")]
        named_pipe: Option<String>,

        /// The transport type: "tcp", "socket", or "named-pipe"
        transport: String,

        /// The path to the server executable (this process)
        server_path: String,

        /// The PID of the server process
        server_pid: u32,

        /// The authentication token, if any
        bearer_token: Option<String>,

        /// The path to the log file, if any
        log_path: Option<String>,
    }

    // Get the server path
    let server_path = std::env::current_exe()?
        .to_str()
        .ok_or("Failed to convert server path to string")?
        .to_string();

    // Get the server PID
    let server_pid = std::process::id();

    // Create the connection info struct based on the connection type
    let connection_info_new = match connection_info {
        ServerConnectionType::Tcp { port, base_path } => ServerConnectionInfoNew {
            port: Some(*port),
            base_path: Some(base_path.clone()),
            socket_path: None,
            named_pipe: None,
            transport: "tcp".to_string(),
            server_path,
            server_pid,
            bearer_token: token.clone(),
            log_path: log_file.clone(),
        },
        #[cfg(unix)]
        ServerConnectionType::Socket { socket_path, .. } => ServerConnectionInfoNew {
            port: None,
            base_path: None,
            socket_path: Some(socket_path.clone()),
            named_pipe: None,
            transport: "socket".to_string(),
            server_path,
            server_pid,
            bearer_token: token.clone(),
            log_path: log_file.clone(),
        },
        #[cfg(windows)]
        ServerConnectionType::NamedPipe { pipe_name } => ServerConnectionInfoNew {
            port: None,
            base_path: None,
            socket_path: None,
            named_pipe: Some(pipe_name.clone()),
            transport: "named-pipe".to_string(),
            server_path,
            server_pid,
            bearer_token: token.clone(),
            log_path: log_file.clone(),
        },
    };

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&connection_info_new)?;

    // Write to file
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;

    Ok(())
}
