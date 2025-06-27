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
mod websocket_service;
mod wire_message;
mod wire_message_header;
mod working_dir;
mod zmq_ws_proxy;

// Enum to handle different listener types
#[cfg(unix)]
enum ListenerType {
    Tcp(std::net::TcpListener),
    Unix(tokio::net::UnixListener),
}

#[cfg(windows)]
enum ListenerType {
    Tcp(std::net::TcpListener),
    NamedPipe(String), // Store the pipe name for Windows named pipes
}

#[cfg(not(any(unix, windows)))]
enum ListenerType {
    Tcp(std::net::TcpListener),
}

// Helper function to create TCP listener
fn create_tcp_listener(port: u16) -> std::net::TcpListener {
    match port {
        0 => {
            // If the port is 0, let the OS pick a random port
            match std::net::TcpListener::bind("127.0.0.1:0") {
                Ok(listener) => {
                    let port = listener.local_addr().unwrap().port();
                    log::info!("Using OS-assigned port: {}", port);
                    listener
                }
                Err(e) => {
                    log::error!("Failed to bind to any available port: {}", e);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            // If the port is specified, try to bind to it
            let addr = format!("127.0.0.1:{}", port);
            match std::net::TcpListener::bind(&addr) {
                Ok(listener) => {
                    log::info!("Using specified port: {}", port);
                    listener
                }
                Err(e) => {
                    log::error!("Failed to bind to port {}: {}", port, e);
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Determine the transport type to use
fn determine_transport(args: &Args) -> String {
    if let Some(ref transport) = args.transport {
        match transport.as_str() {
            "tcp" | "socket" | "named-pipe" => transport.clone(),
            _ => {
                log::error!("Invalid transport type '{}'. Valid values are 'tcp', 'socket', and 'named-pipe'", transport);
                std::process::exit(1);
            }
        }
    } else if args.connection_file.is_some() {
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

/// Generate a socket path for Unix domain sockets
#[cfg(unix)]
fn generate_socket_path(socket_dir: Option<&String>) -> String {
    use std::env;

    let socket_directory = if let Some(dir) = socket_dir {
        // Use the explicitly provided socket directory
        std::path::PathBuf::from(dir)
    } else {
        // Try XDG runtime directory first, then fall back to temp directory
        if let Ok(xdg_runtime_dir) = env::var("XDG_RUNTIME_DIR") {
            std::path::PathBuf::from(xdg_runtime_dir)
        } else {
            env::temp_dir()
        }
    };

    let socket_name = format!("kallichore-{}.sock", std::process::id());
    socket_directory
        .join(socket_name)
        .to_string_lossy()
        .to_string()
}

/// Generate a named pipe name for Windows
#[cfg(windows)]
fn generate_named_pipe() -> String {
    format!(r"\\.\pipe\kallichore-{}", std::process::id())
}

/// Information about the server connection that gets written to the connection file
#[derive(Debug, Clone)]
enum ServerConnectionType {
    Tcp {
        port: u16,
        base_path: String,
    },
    #[cfg(unix)]
    Socket {
        socket_path: String,
        /// Whether this socket was created by the server (true) or provided by user (false)
        server_created: bool,
    },
    #[cfg(windows)]
    NamedPipe {
        pipe_name: String,
    },
}

/// Create the appropriate listener based on transport type and arguments
fn create_listener(args: &Args, transport_type: &str) -> (ListenerType, ServerConnectionType) {
    // If --unix-socket is explicitly provided, use that (overrides transport type)
    #[cfg(unix)]
    if let Some(ref socket_path) = args.unix_socket {
        return create_unix_listener(socket_path);
    }

    match transport_type {
        "tcp" => create_tcp_listener_with_info(args.port),
        #[cfg(unix)]
        "socket" => {
            let socket_path = generate_socket_path(args.socket_dir.as_ref());
            create_unix_listener_with_created_flag(&socket_path, true)
        }
        #[cfg(windows)]
        "named-pipe" => {
            let pipe_name = generate_named_pipe();
            create_named_pipe_listener(&pipe_name)
        }
        #[cfg(unix)]
        "named-pipe" => {
            log::error!("Named pipes are not supported on Unix systems");
            std::process::exit(1);
        }
        #[cfg(windows)]
        "socket" => {
            log::error!("Unix domain sockets are not supported on Windows");
            std::process::exit(1);
        }
        _ => {
            log::error!("Unsupported transport type: {}", transport_type);
            std::process::exit(1);
        }
    }
}

/// Create TCP listener with connection info
fn create_tcp_listener_with_info(port: u16) -> (ListenerType, ServerConnectionType) {
    let tcp_listener = create_tcp_listener(port);
    let actual_port = tcp_listener.local_addr().unwrap().port();
    log::info!("Using TCP port: {}", actual_port);

    let connection_info = ServerConnectionType::Tcp {
        port: actual_port,
        base_path: format!("http://127.0.0.1:{}", actual_port),
    };

    (ListenerType::Tcp(tcp_listener), connection_info)
}

/// Create Unix domain socket listener
#[cfg(unix)]
fn create_unix_listener(socket_path: &str) -> (ListenerType, ServerConnectionType) {
    create_unix_listener_with_created_flag(socket_path, false)
}

/// Create Unix domain socket listener with server_created flag
#[cfg(unix)]
fn create_unix_listener_with_created_flag(
    socket_path: &str,
    server_created: bool,
) -> (ListenerType, ServerConnectionType) {
    use std::os::unix::net::UnixListener as StdUnixListener;
    use tokio::net::UnixListener;

    // Remove existing socket file if it exists
    if std::path::Path::new(&socket_path).exists() {
        if let Err(e) = std::fs::remove_file(&socket_path) {
            log::warn!(
                "Failed to remove existing socket file '{}': {}",
                socket_path,
                e
            );
        }
    }

    match StdUnixListener::bind(&socket_path) {
        Ok(std_listener) => {
            std_listener
                .set_nonblocking(true)
                .expect("Failed to set Unix listener to non-blocking");
            let listener = UnixListener::from_std(std_listener)
                .expect("Failed to convert to tokio UnixListener");
            log::info!("Using Unix domain socket: {}", socket_path);

            let connection_info = ServerConnectionType::Socket {
                socket_path: socket_path.to_string(),
                server_created,
            };

            (ListenerType::Unix(listener), connection_info)
        }
        Err(e) => {
            log::error!("Failed to bind to Unix socket '{}': {}", socket_path, e);
            std::process::exit(1);
        }
    }
}

/// Create named pipe listener (Windows)
#[cfg(windows)]
fn create_named_pipe_listener(pipe_name: &str) -> (ListenerType, ServerConnectionType) {
    // For now, we'll store the pipe name and create the actual listener in the server
    // This is because Windows named pipes require different handling
    log::info!("Using named pipe: {}", pipe_name);

    let connection_info = ServerConnectionType::NamedPipe {
        pipe_name: pipe_name.to_string(),
    };

    (
        ListenerType::NamedPipe(pipe_name.to_string()),
        connection_info,
    )
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

    /// The path to a connection file. If specified, the server will select a
    /// port and authentication token itself, and write them to the given file.
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

    // Create the appropriate listener based on transport type and arguments
    let (listener, connection_info) = create_listener(&args, &transport_type);

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

    // Display connection information
    match &connection_info {
        ServerConnectionType::Tcp { port, .. } => {
            println!("Listening at 127.0.0.1:{}", port);
        }
        #[cfg(unix)]
        ServerConnectionType::Socket { socket_path, .. } => {
            println!("Listening on Unix socket: {}", socket_path);
        }
        #[cfg(windows)]
        ServerConnectionType::NamedPipe { pipe_name } => {
            println!("Listening on named pipe: {}", pipe_name);
        }
    }

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

    // Extract the main server socket path if we created it
    #[cfg(unix)]
    let main_server_socket = match &connection_info {
        ServerConnectionType::Socket {
            socket_path,
            server_created,
        } if *server_created => Some(socket_path.clone()),
        _ => None,
    };
    #[cfg(not(unix))]
    let main_server_socket: Option<String> = None;

    // Convert the listener type and pass to the server
    let server_listener = match listener {
        ListenerType::Tcp(tcp_listener) => {
            tcp_listener
                .set_nonblocking(true)
                .expect("Failed to set TCP listener to non-blocking");
            let tokio_listener = tokio::net::TcpListener::from_std(tcp_listener)
                .expect("Failed to convert to tokio TcpListener");
            server::ServerListener::Tcp(tokio_listener)
        }
        #[cfg(unix)]
        ListenerType::Unix(unix_listener) => server::ServerListener::Unix(unix_listener),
        #[cfg(windows)]
        ListenerType::NamedPipe(pipe_name) => server::ServerListener::NamedPipe(pipe_name),
    };

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
