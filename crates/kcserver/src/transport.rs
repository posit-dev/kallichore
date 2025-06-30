//
// transport.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

//! Transport trait and implementations for different connection types

use anyhow;
use async_trait::async_trait;
use std::fmt;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;

/// Transport trait defining common interface for all transport mechanisms
#[async_trait]
#[allow(dead_code)] // Some methods may not be used directly but are part of the trait interface
pub trait Transport: Send + Sync + fmt::Debug {
    /// The listener type this transport uses
    type Listener;
    
    /// The connection info type this transport produces
    type ConnectionInfo: Clone + fmt::Debug;
    
    /// Create a new transport instance with the given configuration
    async fn create(config: TransportConfig) -> Result<Self, TransportError>
    where
        Self: Sized;
    
    /// Get the listener for this transport
    fn listener(&self) -> &Self::Listener;
    
    /// Get connection information for this transport
    fn connection_info(&self) -> &Self::ConnectionInfo;
    
    /// Get the transport type as a string
    fn transport_type(&self) -> &'static str;
    
    /// Log connection information
    fn log_connection_info(&self);
}

/// Configuration for creating transports
#[derive(Debug, Clone)]
#[allow(dead_code)] // Some fields may not be used on all platforms
pub struct TransportConfig {
    /// TCP port (for TCP transport)
    pub port: u16,
    /// Unix socket path (for Unix socket transport)
    #[cfg(unix)]
    pub unix_socket_path: Option<String>,
    /// Socket directory (for Unix socket transport)
    #[cfg(unix)]
    pub socket_dir: Option<String>,
    /// Named pipe name (for Windows named pipe transport)
    #[cfg(windows)]
    pub named_pipe_name: Option<String>,
    /// Whether the server created the socket/pipe (vs user-provided)
    pub server_created: bool,
}

/// Error type for transport operations
pub type TransportError = anyhow::Error;

/// Connection information for TCP transport
#[derive(Debug, Clone)]
pub struct TcpConnectionInfo {
    pub port: u16,
    pub base_path: String,
}

/// TCP transport implementation
#[derive(Debug)]
pub struct TcpTransport {
    listener: TcpListener,
    connection_info: TcpConnectionInfo,
}

#[async_trait]
impl Transport for TcpTransport {
    type Listener = TcpListener;
    type ConnectionInfo = TcpConnectionInfo;
    
    async fn create(config: TransportConfig) -> Result<Self, TransportError> {
        let tcp_listener = create_tcp_listener(config.port)?;
        let actual_port = tcp_listener.local_addr()?.port();
        
        let connection_info = TcpConnectionInfo {
            port: actual_port,
            base_path: format!("http://127.0.0.1:{}", actual_port),
        };
        
        Ok(Self {
            listener: tcp_listener,
            connection_info,
        })
    }
    
    fn listener(&self) -> &Self::Listener {
        &self.listener
    }
    
    fn connection_info(&self) -> &Self::ConnectionInfo {
        &self.connection_info
    }
    
    fn transport_type(&self) -> &'static str {
        "tcp"
    }
    
    fn log_connection_info(&self) {
        log::info!("Using TCP transport on port: {}", self.connection_info.port);
        println!("Listening at 127.0.0.1:{}", self.connection_info.port);
    }
}

/// Helper function to create TCP listener
fn create_tcp_listener(port: u16) -> Result<TcpListener, TransportError> {
    let listener = match port {
        0 => {
            // If the port is 0, let the OS pick a random port
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            let port = listener.local_addr()?.port();
            log::info!("Using OS-assigned port: {}", port);
            listener
        }
        _ => {
            // If the port is specified, try to bind to it
            let addr = format!("127.0.0.1:{}", port);
            let listener = std::net::TcpListener::bind(&addr)?;
            log::info!("Using specified port: {}", port);
            listener
        }
    };
    
    listener.set_nonblocking(true)?;
    let tokio_listener = TcpListener::from_std(listener)?;
    Ok(tokio_listener)
}

/// Connection information for Unix socket transport
#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct UnixSocketConnectionInfo {
    pub socket_path: String,
    pub server_created: bool,
}

/// Unix socket transport implementation
#[cfg(unix)]
#[derive(Debug)]
pub struct UnixSocketTransport {
    listener: UnixListener,
    connection_info: UnixSocketConnectionInfo,
}

#[cfg(unix)]
#[async_trait]
impl Transport for UnixSocketTransport {
    type Listener = UnixListener;
    type ConnectionInfo = UnixSocketConnectionInfo;
    
    async fn create(config: TransportConfig) -> Result<Self, TransportError> {
        let socket_path = config.unix_socket_path.clone().unwrap_or_else(|| 
            generate_socket_path(config.socket_dir.as_ref())
        );
        
        let server_created = config.unix_socket_path.is_none();
        
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
        
        let std_listener = std::os::unix::net::UnixListener::bind(&socket_path)?;
        std_listener.set_nonblocking(true)?;
        let listener = UnixListener::from_std(std_listener)?;
        
        let connection_info = UnixSocketConnectionInfo {
            socket_path,
            server_created,
        };
        
        Ok(Self {
            listener,
            connection_info,
        })
    }
    
    fn listener(&self) -> &Self::Listener {
        &self.listener
    }
    
    fn connection_info(&self) -> &Self::ConnectionInfo {
        &self.connection_info
    }
    
    fn transport_type(&self) -> &'static str {
        "socket"
    }
    
    fn log_connection_info(&self) {
        log::info!("Using Unix domain socket: {}", self.connection_info.socket_path);
        println!("Listening on Unix socket: {}", self.connection_info.socket_path);
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

/// Connection information for named pipe transport
#[cfg(windows)]
#[derive(Debug, Clone)]
pub struct NamedPipeConnectionInfo {
    pub pipe_name: String,
}

/// Named pipe transport implementation
#[cfg(windows)]
#[derive(Debug)]
pub struct NamedPipeTransport {
    pipe_name: String,
    connection_info: NamedPipeConnectionInfo,
}

#[cfg(windows)]
#[async_trait]
impl Transport for NamedPipeTransport {
    type Listener = String; // Store pipe name as the "listener"
    type ConnectionInfo = NamedPipeConnectionInfo;
    
    async fn create(config: TransportConfig) -> Result<Self, TransportError> {
        let pipe_name = match config.named_pipe_name {
            Some(name) => name,
            None => generate_named_pipe(),
        };
        
        let connection_info = NamedPipeConnectionInfo {
            pipe_name: pipe_name.clone(),
        };
        
        Ok(Self {
            pipe_name: pipe_name.clone(),
            connection_info,
        })
    }
    
    fn listener(&self) -> &Self::Listener {
        &self.pipe_name
    }
    
    fn connection_info(&self) -> &Self::ConnectionInfo {
        &self.connection_info
    }
    
    fn transport_type(&self) -> &'static str {
        "named-pipe"
    }
    
    fn log_connection_info(&self) {
        log::info!("Using named pipe: {}", self.connection_info.pipe_name);
        println!("Listening on named pipe: {}", self.connection_info.pipe_name);
    }
}

/// Generate a named pipe name for Windows
#[cfg(windows)]
fn generate_named_pipe() -> String {
    format!(r"\\.\pipe\kallichore-{}", std::process::id())
}

/// Unified transport enum that can hold any transport type
#[derive(Debug)]
pub enum TransportType {
    Tcp(TcpTransport),
    #[cfg(unix)]
    UnixSocket(UnixSocketTransport),
    #[cfg(windows)]
    NamedPipe(NamedPipeTransport),
}

impl TransportType {
    /// Create a transport based on the transport type string and configuration
    pub async fn create(transport_type: &str, config: TransportConfig) -> Result<Self, TransportError> {
        match transport_type {
            "tcp" => Ok(TransportType::Tcp(TcpTransport::create(config).await?)),
            #[cfg(unix)]
            "socket" => Ok(TransportType::UnixSocket(UnixSocketTransport::create(config).await?)),
            #[cfg(windows)]
            "named-pipe" => Ok(TransportType::NamedPipe(NamedPipeTransport::create(config).await?)),
            #[cfg(unix)]
            "named-pipe" => Err(anyhow::anyhow!(
                "Named pipes are not supported on Unix systems"
            )),
            #[cfg(windows)]
            "socket" => Err(anyhow::anyhow!(
                "Unix domain sockets are not supported on Windows"
            )),
            _ => Err(anyhow::anyhow!(
                "Invalid transport type: {}", transport_type
            )),
        }
    }
    
    /// Get the transport type as a string
    #[allow(dead_code)] // May be used in future features
    pub fn transport_type(&self) -> &'static str {
        match self {
            TransportType::Tcp(transport) => transport.transport_type(),
            #[cfg(unix)]
            TransportType::UnixSocket(transport) => transport.transport_type(),
            #[cfg(windows)]
            TransportType::NamedPipe(transport) => transport.transport_type(),
        }
    }
    
    /// Log connection information
    pub fn log_connection_info(&self) {
        match self {
            TransportType::Tcp(transport) => transport.log_connection_info(),
            #[cfg(unix)]
            TransportType::UnixSocket(transport) => transport.log_connection_info(),
            #[cfg(windows)]
            TransportType::NamedPipe(transport) => transport.log_connection_info(),
        }
    }
    
    /// Convert transport to ServerConnectionType for connection file compatibility
    pub fn to_server_connection_type(&self) -> ServerConnectionType {
        match self {
            TransportType::Tcp(transport) => {
                let info = transport.connection_info();
                ServerConnectionType::Tcp {
                    port: info.port,
                    base_path: info.base_path.clone(),
                }
            }
            #[cfg(unix)]
            TransportType::UnixSocket(transport) => {
                let info = transport.connection_info();
                ServerConnectionType::Socket {
                    socket_path: info.socket_path.clone(),
                    server_created: info.server_created,
                }
            }
            #[cfg(windows)]
            TransportType::NamedPipe(transport) => {
                let info = transport.connection_info();
                ServerConnectionType::NamedPipe {
                    pipe_name: info.pipe_name.clone(),
                }
            }
        }
    }
    
    /// Extract the server listener, consuming the transport
    pub fn into_server_listener(self) -> crate::server::ServerListener {
        match self {
            TransportType::Tcp(tcp_transport) => {
                crate::server::ServerListener::Tcp(tcp_transport.listener)
            }
            #[cfg(unix)]
            TransportType::UnixSocket(unix_transport) => {
                crate::server::ServerListener::Unix(unix_transport.listener)
            }
            #[cfg(windows)]
            TransportType::NamedPipe(pipe_transport) => {
                crate::server::ServerListener::NamedPipe(pipe_transport.pipe_name)
            }
        }
    }
    
    /// Get main server socket path for cleanup (Unix only)
    #[cfg(unix)]
    pub fn main_server_socket(&self) -> Option<String> {
        match self {
            TransportType::UnixSocket(transport) => {
                let info = transport.connection_info();
                if info.server_created {
                    Some(info.socket_path.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    
    #[cfg(not(unix))]
    pub fn main_server_socket(&self) -> Option<String> {
        None
    }
}

/// Information about the server connection that gets written to the connection file
#[derive(Debug, Clone)]
#[allow(dead_code)] // Some fields may not be used on all platforms
pub enum ServerConnectionType {
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
