//
// named_pipe_http.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

//! HTTP server implementation over Windows named pipes

use crate::server::Server;
use std::sync::Arc;

#[cfg(windows)]
pub struct NamedPipeServer {
    pipe_name: String,
}

#[cfg(windows)]
impl NamedPipeServer {
    #[allow(dead_code)]
    pub fn bind(pipe_name: &str) -> std::io::Result<Self> {
        // Validate pipe name format
        if !pipe_name.starts_with("\\\\.\\pipe\\") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid pipe name format: {}", pipe_name),
            ));
        }

        log::info!("Creating named pipe server at: {}", pipe_name);

        Ok(NamedPipeServer {
            pipe_name: pipe_name.to_string(),
        })
    }

    pub async fn accept(&self) -> std::io::Result<NamedPipeStream> {
        // For now, simulate accepting a connection
        // In a full implementation, this would create actual named pipe connections
        log::info!("Simulating named pipe connection on: {}", self.pipe_name);
        
        // Simulate some async work
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        Ok(NamedPipeStream::new())
    }
}

/// A Windows named pipe stream
#[cfg(windows)]
pub struct NamedPipeStream {
    // For the simplified implementation, we'll just track that it exists
    _marker: std::marker::PhantomData<()>,
}

#[cfg(windows)]
impl NamedPipeStream {
    fn new() -> Self {
        NamedPipeStream {
            _marker: std::marker::PhantomData,
        }
    }
}

#[cfg(windows)]
impl Drop for NamedPipeStream {
    fn drop(&mut self) {
        // Cleanup would go here in a full implementation
    }
}

#[cfg(windows)]
impl tokio::io::AsyncRead for NamedPipeStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        // For the simplified implementation, simulate reading HTTP data
        let http_response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
        let len = std::cmp::min(buf.remaining(), http_response.len());
        if len > 0 {
            buf.put_slice(&http_response[..len]);
        }
        std::task::Poll::Ready(Ok(()))
    }
}

#[cfg(windows)]
impl tokio::io::AsyncWrite for NamedPipeStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        // For the simplified implementation, just accept all writes
        log::debug!("Named pipe received {} bytes: {}", buf.len(), String::from_utf8_lossy(buf));
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

/// Create a hyper service that accepts connections from a named pipe server
#[cfg(windows)]
pub struct NamedPipeIncoming {
    server: NamedPipeServer,
}

#[cfg(windows)]
impl NamedPipeIncoming {
    #[allow(dead_code)]
    pub fn new(pipe_name: &str) -> std::io::Result<Self> {
        let server = NamedPipeServer::bind(pipe_name)?;
        Ok(NamedPipeIncoming { server })
    }
}

#[cfg(windows)]
impl hyper::server::accept::Accept for NamedPipeIncoming {
    type Conn = NamedPipeStream;
    type Error = std::io::Error;

    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Self::Conn, Self::Error>>> {
        // For simplified implementation, immediately return a connection
        // In production, this should properly poll for new connections
        log::debug!("Named pipe accepting new connection");
        match futures::executor::block_on(self.server.accept()) {
            Ok(stream) => std::task::Poll::Ready(Some(Ok(stream))),
            Err(e) => std::task::Poll::Ready(Some(Err(e))),
        }
    }
}

#[cfg(windows)]
pub async fn start_named_pipe_http_server(
    pipe_name: String,
    server: Server<()>,
) -> Result<(), Box<dyn std::error::Error>> {    use tokio::net::windows::named_pipe::ServerOptions;

    let server = Arc::new(server);
    
    log::info!("Starting HTTP server over named pipe at: {}", pipe_name);    // Accept connections in a loop
    loop {
        // Create a new named pipe for each connection
        let pipe_server = match ServerOptions::new().create(&pipe_name) {
            Ok(pipe) => pipe,
            Err(e) => {
                log::error!("Failed to create named pipe server: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                continue;
            }
        };        // Accept a connection
        match pipe_server.connect().await {
            Ok(_) => {
                log::info!("Client connected to named pipe: {}", pipe_name);
                let server_clone = Arc::clone(&server);
                
                // Handle this connection
                tokio::spawn(async move {
                    if let Err(e) = handle_http_request(pipe_server, server_clone).await {
                        log::error!("Error handling named pipe HTTP request: {}", e);
                    }
                });
            }
            Err(e) => {
                log::error!("Failed to accept named pipe connection: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}

#[cfg(windows)]
async fn handle_http_request(
    mut stream: tokio::net::windows::named_pipe::NamedPipeServer,
    server: Arc<Server<()>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use hyper::Method;
    use std::str::FromStr;
    
    // Read the HTTP request
    let mut buffer = vec![0u8; 4096];
    let bytes_read = stream.read(&mut buffer).await?;
    let request_str = String::from_utf8_lossy(&buffer[..bytes_read]);
    
    log::debug!("Named pipe received HTTP request: {}", request_str);
    
    // Parse the request
    let lines: Vec<&str> = request_str.split("\r\n").collect();
    if lines.is_empty() {
        return Err("No HTTP request received".into());
    }
    
    // Parse the request line
    let request_line = lines[0];
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 3 {
        return Err("Invalid HTTP request line".into());
    }
    
    let method = Method::from_str(parts[0])?;
    let path = parts[1];
    
    // Find the body (after empty line)
    let mut body = String::new();
    let mut found_empty = false;
    for line in lines.iter().skip(1) {
        if found_empty {
            body.push_str(line);
            body.push_str("\r\n");
        } else if line.trim().is_empty() {
            found_empty = true;
        }
    }
    
    log::info!("Named pipe HTTP request: {} {}", method, path);
    
    // Handle the request based on the path using real server methods
    let response = match (method.as_str(), path) {
        ("GET", "/sessions") => {
            // Get the actual session list from the server
            let session_list = server.get_sessions_list().await;
            let json = serde_json::to_string(&session_list)?;
            create_http_response(200, "OK", &json)
        }
        ("PUT", "/sessions") => {
            // Handle session creation with real session creation
            match serde_json::from_str::<kallichore_api::models::NewSession>(&body.trim()) {
                Ok(new_session_request) => {
                    // Call the server's session creation method
                    match server.create_session(new_session_request).await {
                        Ok(session_id) => {
                            let response_obj = kallichore_api::models::NewSession200Response {
                                session_id,
                            };
                            let json = serde_json::to_string(&response_obj)?;
                            create_http_response(200, "OK", &json)
                        }
                        Err(e) => {
                            log::error!("Failed to create session: {}", e);
                            let error_obj = kallichore_api::models::Error {
                                code: "SESSION_CREATE_FAILED".to_string(),
                                message: e,
                                details: None,
                            };
                            let json = serde_json::to_string(&error_obj)?;
                            create_http_response(500, "Internal Server Error", &json)
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to parse session request: {}", e);
                    let error_obj = kallichore_api::models::Error {
                        code: "INVALID_REQUEST".to_string(),
                        message: format!("Invalid request: {}", e),
                        details: None,
                    };
                    let json = serde_json::to_string(&error_obj)?;
                    create_http_response(400, "Bad Request", &json)
                }
            }
        }
        _ => {
            // Unknown endpoint
            let error_obj = kallichore_api::models::Error {
                code: "NOT_FOUND".to_string(),
                message: format!("Not found: {} {}", method, path),
                details: None,
            };
            let json = serde_json::to_string(&error_obj)?;
            create_http_response(404, "Not Found", &json)
        }
    };
    
    // Send the response
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    
    log::info!("Named pipe HTTP response sent successfully");
    Ok(())
}

fn create_http_response(status_code: u16, status_text: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        status_code,
        status_text,
        body.len(),
        body
    )
}
