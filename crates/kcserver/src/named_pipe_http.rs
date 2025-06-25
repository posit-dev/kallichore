//
// named_pipe_http.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

//! HTTP server implementation over Windows named pipes

use crate::server::Server;
use kallichore_api::Api;
use std::sync::Arc;
use swagger::{AuthData, Authorization, EmptyContext, Push, XSpanIdString};

// Define the proper context type that implements all required traits
type NamedPipeContext =
    <<<EmptyContext as Push<XSpanIdString>>::Result as Push<Option<AuthData>>>::Result as Push<
        Option<Authorization>,
    >>::Result;

#[cfg(windows)]
pub async fn start_named_pipe_http_server(
    pipe_name: String,
    server: Server<NamedPipeContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::net::windows::named_pipe::ServerOptions;

    log::info!("Starting HTTP server over named pipe at: {}", pipe_name);

    let server = Arc::new(server);

    // Accept connections in a loop
    loop {
        // Create a new named pipe for each connection
        let pipe_server = match ServerOptions::new().create(&pipe_name) {
            Ok(pipe) => pipe,
            Err(e) => {
                log::error!("Failed to create named pipe server: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        // Accept a connection
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
    server: Arc<Server<NamedPipeContext>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use hyper::Method;
    use std::str::FromStr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    // Create a proper context for the API calls
    let context = EmptyContext::default()
        .push(XSpanIdString("named-pipe".to_string()))
        .push(None::<AuthData>)
        .push(None::<Authorization>);

    // Handle the request using the API trait methods
    let response = match (method.as_str(), path) {
        ("GET", "/sessions") => match server.list_sessions(&context).await {
            Ok(kallichore_api::ListSessionsResponse::ListOfActiveSessions(session_list)) => {
                let json = serde_json::to_string(&session_list)?;
                create_http_response(200, "OK", &json)
            }
            Err(e) => {
                log::error!("Failed to list sessions: {}", e);
                let error_obj = kallichore_api::models::Error {
                    code: "SESSION_LIST_FAILED".to_string(),
                    message: format!("Failed to list sessions: {}", e),
                    details: None,
                };
                let json = serde_json::to_string(&error_obj)?;
                create_http_response(500, "Internal Server Error", &json)
            }
        },
        ("PUT", "/sessions") => {
            // Handle session creation using the API trait
            match serde_json::from_str::<kallichore_api::models::NewSession>(&body.trim()) {
                Ok(new_session_request) => {
                    match server.new_session(new_session_request, &context).await {
                        Ok(kallichore_api::NewSessionResponse::TheSessionID(session_response)) => {
                            let json = serde_json::to_string(&session_response)?;
                            create_http_response(200, "OK", &json)
                        }
                        Ok(kallichore_api::NewSessionResponse::InvalidRequest(error)) => {
                            let json = serde_json::to_string(&error)?;
                            create_http_response(400, "Bad Request", &json)
                        }
                        Ok(kallichore_api::NewSessionResponse::Unauthorized) => {
                            let error_obj = kallichore_api::models::Error {
                                code: "UNAUTHORIZED".to_string(),
                                message: "Unauthorized".to_string(),
                                details: None,
                            };
                            let json = serde_json::to_string(&error_obj)?;
                            create_http_response(401, "Unauthorized", &json)
                        }
                        Err(e) => {
                            log::error!("Failed to create session: {}", e);
                            let error_obj = kallichore_api::models::Error {
                                code: "SESSION_CREATE_FAILED".to_string(),
                                message: format!("Failed to create session: {}", e),
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

#[cfg(not(windows))]
pub async fn start_named_pipe_http_server(
    _pipe_name: String,
    _server: Server<NamedPipeContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("Named pipes are only supported on Windows".into())
}
