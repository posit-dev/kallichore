//
// mod.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

#![allow(dead_code)]

pub mod test_utils;
pub mod transport;

use kallichore_api::{ApiNoContext, Client, ContextWrapperExt};
use kcshared::port_picker::pick_unused_tcp_port;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use swagger::{AuthData, ContextBuilder, EmptyContext, Push, XSpanIdString};
use tokio::time::timeout;

type ClientContext = swagger::make_context_ty!(
    ContextBuilder,
    EmptyContext,
    Option<AuthData>,
    XSpanIdString
);

#[derive(Clone)]
pub enum TestServerMode {
    Http,
    #[cfg(windows)]
    NamedPipe,
    #[cfg(unix)]
    DomainSocket,
}

// Common server configuration for reducing duplication
struct ServerConfig {
    args: Vec<String>,
    expected_output_pattern: Option<String>, // For extracting info from server output
}

impl ServerConfig {
    fn tcp(port: u16) -> Self {
        Self {
            args: vec![
                "--port".to_string(),
                port.to_string(),
                "--token".to_string(),
                "none".to_string(),
            ],
            expected_output_pattern: None,
        }
    }

    #[cfg(windows)]
    fn named_pipe(connection_file_path: &str) -> Self {
        Self {
            args: vec![
                "--connection-file".to_string(),
                connection_file_path.to_string(),
                "--transport".to_string(),
                "named-pipe".to_string(),
                "--token".to_string(),
                "none".to_string(),
            ],
            expected_output_pattern: None,
        }
    }

    #[cfg(unix)]
    fn unix_socket(socket_path: &str) -> Self {
        Self {
            args: vec![
                "--unix-socket".to_string(),
                socket_path.to_string(),
                "--token".to_string(),
                "none".to_string(),
            ],
            expected_output_pattern: None,
        }
    }
}

// Common server creation logic to reduce duplication
async fn create_server_process(config: ServerConfig) -> Child {
    // Try to use pre-built binary first, fall back to cargo run
    let binary_path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/kcserver")
        .with_extension(if cfg!(windows) { "exe" } else { "" });

    let mut cmd = if binary_path.exists() {
        println!("Using pre-built binary at: {:?}", binary_path);
        let mut c = Command::new(&binary_path);
        c.args(&config.args);
        c
    } else {
        println!("Pre-built binary not found, using cargo run");
        let mut c = Command::new("cargo");
        let mut args = vec!["run".to_string(), "--bin".to_string(), "kcserver".to_string(), "--".to_string()];
        args.extend(config.args);
        c.args(&args);
        c
    };

    // Capture output for debugging if needed
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Reduce log level for faster startup but still capture errors
    cmd.env("RUST_LOG", "warn");

    cmd.spawn().expect("Failed to start kcserver")
}

#[allow(dead_code)]
pub struct TestServer {
    child: Child,
    base_url: String,
    port: u16,
    mode: TestServerMode,
    #[cfg(windows)]
    pipe_name: Option<String>,
    #[cfg(unix)]
    socket_path: Option<String>,
}

impl TestServer {
    pub async fn start() -> Self {
        Self::start_with_mode(TestServerMode::Http).await
    }

    pub async fn start_with_mode(mode: TestServerMode) -> Self {
        match mode {
            TestServerMode::Http => Self::start_http_server().await,
            #[cfg(windows)]
            TestServerMode::NamedPipe => Self::start_named_pipe_server().await,
            #[cfg(unix)]
            TestServerMode::DomainSocket => Self::start_domain_socket_server().await,
        }
    }

    async fn start_http_server() -> Self {
        let port = pick_unused_tcp_port().expect("Failed to pick unused port");
        let config = ServerConfig::tcp(port);
        let child = create_server_process(config).await;
        let base_url = format!("http://localhost:{}", port);

        let test_server = TestServer {
            child,
            base_url,
            port,
            mode: TestServerMode::Http,
            #[cfg(windows)]
            pipe_name: None,
            #[cfg(unix)]
            socket_path: None,
        };

        test_server.wait_for_ready().await;
        test_server
    }

    #[cfg(windows)]
    async fn start_named_pipe_server() -> Self {
        use tempfile::NamedTempFile;

        // Create a temporary connection file
        let temp_file = NamedTempFile::new().expect("Failed to create temp connection file");
        let connection_file_path = temp_file.path().to_string_lossy().to_string();

        let config = ServerConfig::named_pipe(&connection_file_path);
        let child = create_server_process(config).await;

        // Wait for the connection file to be created and read the pipe name from it
        let mut pipe_name = None;
        for _attempt in 0..100 {
            if std::path::Path::new(&connection_file_path).exists() {
                if let Ok(content) = std::fs::read_to_string(&connection_file_path) {
                    if !content.trim().is_empty() {
                        if let Ok(connection_info) =
                            serde_json::from_str::<serde_json::Value>(&content)
                        {
                            if let Some(pipe_path) =
                                connection_info.get("named_pipe").and_then(|v| v.as_str())
                            {
                                pipe_name = Some(pipe_path.to_string());
                                break;
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let pipe_name = pipe_name.expect("Failed to get pipe name from connection file");
        println!("Named pipe server started with pipe: {}", pipe_name);

        // For named pipe mode, we need to communicate via the pipe
        let base_url = format!("pipe://{}", pipe_name);

        let test_server = TestServer {
            child,
            base_url,
            port: 0, // Not used for named pipe mode
            mode: TestServerMode::NamedPipe,
            pipe_name: Some(pipe_name),
            #[cfg(unix)]
            socket_path: None,
        };

        // Wait a bit for the server to be ready
        tokio::time::sleep(Duration::from_secs(2)).await;

        test_server
    }

    #[cfg(unix)]
    async fn start_domain_socket_server() -> Self {
        use tempfile::tempdir;
        use uuid::Uuid;

        // Create a temporary directory for the socket
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let socket_path = temp_dir
            .path()
            .join(format!("kallichore-test-{}.sock", Uuid::new_v4().simple()));

        let config = ServerConfig::unix_socket(socket_path.to_str().unwrap());
        let child = create_server_process(config).await;

        // Wait for the socket file to be created
        for _attempt in 0..100 {
            if socket_path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let base_url = format!("unix://{}", socket_path.to_string_lossy());

        let test_server = TestServer {
            child,
            base_url,
            port: 0, // Not used for domain socket mode
            mode: TestServerMode::DomainSocket,
            #[cfg(windows)]
            pipe_name: None,
            socket_path: Some(socket_path.to_string_lossy().to_string()),
        };

        // Wait a bit for the server to be ready
        tokio::time::sleep(Duration::from_secs(2)).await;

        test_server
    }
    async fn wait_for_ready(&self) {
        match self.mode {
            TestServerMode::Http => {
                let client = self.create_client().await;

                // Increased timeout for Windows and debug builds
                for attempt in 0..60 {
                    match timeout(Duration::from_millis(500), client.server_status()).await {
                        Ok(Ok(_)) => {
                            println!("Server ready after {} attempts", attempt + 1);
                            return;
                        }
                        Ok(Err(e)) => {
                            if attempt > 45 {
                                println!("Server status error on attempt {}: {:?}", attempt, e);
                            }
                        }
                        Err(_) => {
                            if attempt > 45 {
                                println!("Server status timeout on attempt {}", attempt);
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                panic!("HTTP server failed to start within timeout");
            }
            #[cfg(windows)]
            TestServerMode::NamedPipe => {
                // For named pipe servers, we just wait a bit since they don't have HTTP endpoints
                tokio::time::sleep(Duration::from_secs(1)).await;
                println!("Named pipe server should be ready");
            }
            #[cfg(unix)]
            TestServerMode::DomainSocket => {
                // For domain socket servers, we just wait a bit since they don't have HTTP endpoints
                tokio::time::sleep(Duration::from_secs(1)).await;
                println!("Domain socket server should be ready");
            }
        }
    }

    pub async fn create_client(&self) -> Box<dyn ApiNoContext<ClientContext> + Send + Sync> {
        match self.mode {
            TestServerMode::Http => {
                #[allow(trivial_casts)]
                let context: ClientContext = swagger::make_context!(
                    ContextBuilder,
                    EmptyContext,
                    None as Option<AuthData>,
                    XSpanIdString::default()
                );

                let client =
                    Client::try_new_http(&self.base_url).expect("Failed to create HTTP client");

                Box::new(client.with_context(context))
            }
            #[cfg(windows)]
            TestServerMode::NamedPipe => {
                // For named pipe mode, we need a special client that can communicate over named pipes
                // For now, we'll create a dummy client that won't be used for HTTP operations
                // The actual communication will be done directly via named pipe
                #[allow(trivial_casts)]
                let context: ClientContext = swagger::make_context!(
                    ContextBuilder,
                    EmptyContext,
                    None as Option<AuthData>,
                    XSpanIdString::default()
                );

                // Create a placeholder HTTP client - this won't work for actual requests
                // but the named pipe tests should use direct pipe communication
                let client = Client::try_new_http("http://localhost:1")
                    .expect("Failed to create placeholder client");
                Box::new(client.with_context(context))
            }
            #[cfg(unix)]
            TestServerMode::DomainSocket => {
                // Similar to named pipe mode
                #[allow(trivial_casts)]
                let context: ClientContext = swagger::make_context!(
                    ContextBuilder,
                    EmptyContext,
                    None as Option<AuthData>,
                    XSpanIdString::default()
                );

                let client = Client::try_new_http("http://localhost:1")
                    .expect("Failed to create placeholder client");
                Box::new(client.with_context(context))
            }
        }
    }

    #[allow(dead_code)]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }

    #[cfg(windows)]
    #[allow(dead_code)]
    pub fn pipe_name(&self) -> Option<&str> {
        self.pipe_name.as_deref()
    }

    #[cfg(unix)]
    #[allow(dead_code)]
    pub fn socket_path(&self) -> Option<&str> {
        self.socket_path.as_deref()
    }

    #[allow(dead_code)]
    pub fn mode(&self) -> &TestServerMode {
        &self.mode
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        println!("Cleaning up test server (PID: {})", self.child.id());

        // Use kill() which sends SIGTERM on Unix (gentler than SIGKILL)
        // and terminates gracefully on Windows
        if let Err(e) = self.child.kill() {
            println!("Warning: Failed to terminate test server process: {}", e);
        }

        // Wait for the process to terminate
        match self.child.wait() {
            Ok(status) => {
                println!("Test server process terminated with status: {}", status);
            }
            Err(e) => {
                println!("Warning: Failed to wait for test server process: {}", e);
            }
        }
    }
}
