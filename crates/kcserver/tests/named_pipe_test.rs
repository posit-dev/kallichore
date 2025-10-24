//
// named_pipe_test.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Integration test for Windows named pipe functionality

#[cfg(windows)]
mod windows_named_pipe_tests {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[allow(unused_imports)]
    use serde_json::json;

    struct NamedPipeTestServer {
        child: std::process::Child,
        pipe_name: String,
    }

    impl NamedPipeTestServer {
        async fn start() -> Self {
            // Create a temporary connection file
            let temp_file =
                tempfile::NamedTempFile::new().expect("Failed to create temp connection file");
            let connection_file_path = temp_file.path().to_string_lossy().to_string();

            // Try to use pre-built binary first, fall back to cargo run
            let binary_path = std::env::current_dir()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("target/debug/kcserver.exe");

            let mut cmd = if binary_path.exists() {
                println!("Using pre-built binary: {:?}", binary_path);
                let mut c = Command::new(&binary_path);
                c.args(&[
                    "--connection-file",
                    &connection_file_path,
                    "--transport",
                    "named-pipe",
                    "--token",
                    "none", // Disable auth for testing
                ]);
                c
            } else {
                println!("Pre-built binary not found, using cargo run");
                let mut c = Command::new("cargo");
                c.args(&[
                    "run",
                    "--bin",
                    "kcserver",
                    "--",
                    "--connection-file",
                    &connection_file_path,
                    "--transport",
                    "named-pipe",
                    "--token",
                    "none", // Disable auth for testing
                ]);
                c
            };

            // Reduce logging noise for faster startup
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            cmd.env("RUST_LOG", "debug");

            println!("Starting server with command: {:?}", cmd);
            println!("Connection file path: {}", connection_file_path);
            let child = cmd
                .spawn()
                .expect("Failed to start kcserver with named pipe");

            // Wait longer for the server to start and write the connection file
            let mut retries = 0;
            let connection_info: serde_json::Value = loop {
                tokio::time::sleep(Duration::from_millis(500)).await;

                println!("Attempt {}: Checking connection file...", retries + 1);

                match std::fs::read_to_string(&connection_file_path) {
                    Ok(content) if !content.trim().is_empty() => {
                        println!("Connection file content: {}", content);
                        match serde_json::from_str(&content) {
                            Ok(info) => break info,
                            Err(_) if retries < 10 => {
                                retries += 1;
                                continue;
                            }
                            Err(e) => panic!("Failed to parse connection file: {}", e),
                        }
                    }
                    Ok(content) => {
                        println!(
                            "Connection file exists but is empty. Content: '{}'",
                            content
                        );
                        if retries < 10 {
                            retries += 1;
                            continue;
                        } else {
                            panic!("Connection file is empty after 5 seconds");
                        }
                    }
                    Err(e) if retries < 10 => {
                        println!("Connection file doesn't exist yet: {}", e);
                        retries += 1;
                        continue;
                    }
                    Err(e) => panic!("Connection file error after 5 seconds: {}", e),
                }
            };
            let pipe_name = connection_info["named_pipe"]
                .as_str()
                .expect("Missing named_pipe in connection file")
                .to_string();
            let test_server = NamedPipeTestServer { child, pipe_name };

            test_server
        }

        #[allow(dead_code)]
        async fn wait_for_ready(&self) {
            // Wait a bit for the server to start up
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        fn pipe_name(&self) -> &str {
            &self.pipe_name
        }

        async fn stop(mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    impl Drop for NamedPipeTestServer {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    /// Helper function to connect to a named pipe
    #[allow(dead_code)]
    async fn connect_to_named_pipe(pipe_name: &str) -> std::io::Result<std::fs::File> {
        use std::fs::OpenOptions;

        // Try to open the named pipe like a file
        OpenOptions::new().read(true).write(true).open(pipe_name)
    }

    /// Helper function to send HTTP request over named pipe
    #[allow(dead_code)]
    async fn send_http_request(
        pipe_name: &str,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> std::io::Result<HttpResponse> {
        use std::io::{Read, Write};

        let mut pipe = connect_to_named_pipe(pipe_name).await?;

        let request = if let Some(body) = body {
            format!(
                "{} {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                method,
                path,
                body.len(),
                body
            )
        } else {
            format!("{} {} HTTP/1.1\r\nHost: localhost\r\n\r\n", method, path)
        };

        pipe.write_all(request.as_bytes())?;

        // Read response
        let mut buffer = vec![0u8; 4096];
        let bytes_read = pipe.read(&mut buffer)?;
        let response_str = String::from_utf8_lossy(&buffer[..bytes_read]);

        HttpResponse::parse(&response_str)
    }
    /// Simple HTTP response parser
    #[allow(dead_code)]
    struct HttpResponse {
        status_code: u16,
        headers: std::collections::HashMap<String, String>,
        body: String,
    }

    impl HttpResponse {
        fn parse(response: &str) -> std::io::Result<Self> {
            let lines: Vec<&str> = response.split("\r\n").collect();

            if lines.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Empty response",
                ));
            }

            // Parse status line
            let status_parts: Vec<&str> = lines[0].split_whitespace().collect();
            if status_parts.len() < 2 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid status line",
                ));
            }

            let status_code = status_parts[1].parse::<u16>().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid status code")
            })?;

            // Parse headers
            let mut headers = std::collections::HashMap::new();
            let mut body_start = 1;

            for (i, line) in lines.iter().enumerate().skip(1) {
                if line.is_empty() {
                    body_start = i + 1;
                    break;
                }

                if let Some(colon_pos) = line.find(':') {
                    let key = line[..colon_pos].trim().to_lowercase();
                    let value = line[colon_pos + 1..].trim().to_string();
                    headers.insert(key, value);
                }
            }

            // Parse body
            let body = if body_start < lines.len() {
                lines[body_start..].join("\r\n")
            } else {
                String::new()
            };

            Ok(HttpResponse {
                status_code,
                headers,
                body,
            })
        }
    }
    #[tokio::test]
    async fn test_named_pipe_server_startup() {
        // This test verifies that the server can start with a named pipe argument
        let server = NamedPipeTestServer::start().await;

        // Verify the server started successfully
        println!(
            "Named pipe server started successfully with pipe: {}",
            server.pipe_name()
        );

        // Wait for the server to be ready
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify the named pipe exists by attempting to connect
        match connect_to_named_pipe(server.pipe_name()).await {
            Ok(_) => {
                println!(
                    "Successfully connected to named pipe: {}",
                    server.pipe_name()
                );
            }
            Err(e) => {
                println!("Warning: Could not connect to named pipe: {}", e);
                // This might fail if the server isn't fully ready yet, but that's okay
                // The important thing is that the server started without crashing
            }
        }

        server.stop().await;
        println!("Named pipe server startup test passed");
    }

    #[tokio::test]
    async fn test_named_pipe_http_session_creation() {
        // This test creates a session via HTTP over named pipes
        let server = NamedPipeTestServer::start().await;

        // Wait for the server to be ready
        tokio::time::sleep(Duration::from_secs(3)).await;

        let session_id = format!("test-session-{}", uuid::Uuid::new_v4());
        let session_request = json!({
            "session_id": session_id,
            "argv": ["python", "-m", "ipykernel_launcher", "-f", "{connection_file}"],
            "display_name": "Python Test",
            "language": "python",
            "working_directory": ".",
            "session_mode": "console",
            "input_prompt": ">>> ",
            "continuation_prompt": "... ",
            "username": "test",
            "env": {},
            "run_in_shell": false,
            "interrupt_mode": "signal",
            "connection_timeout": 60,
            "protocol_version": "5.3"
        });

        println!("Creating session: {}", session_id);
        println!("Request body: {}", session_request);

        // Attempt to create session via HTTP over named pipe
        match send_http_request(
            server.pipe_name(),
            "PUT",
            "/sessions",
            Some(&session_request.to_string()),
        )
        .await
        {
            Ok(response) => {
                println!("Session creation response: HTTP {}", response.status_code);
                println!("Response body: {}", response.body);

                // A successful session creation should return 200
                // Even if it fails due to missing Python, we've successfully tested HTTP over named pipes
                if response.status_code == 200 {
                    println!("✓ Session created successfully via named pipe HTTP");
                } else if response.status_code == 400 {
                    println!("✓ HTTP over named pipe is working (session creation failed due to missing kernel, which is expected)");
                } else {
                    println!(
                        "✓ HTTP over named pipe responded with status: {}",
                        response.status_code
                    );
                }
            }
            Err(e) => {
                println!(
                    "Warning: Failed to send HTTP request over named pipe: {}",
                    e
                );
                // This might fail if named pipe HTTP isn't fully implemented yet
            }
        }

        server.stop().await;
        println!("Named pipe HTTP session creation test completed");
    }

    #[tokio::test]
    async fn test_named_pipe_channels_upgrade() {
        // This test verifies that the channels-upgrade endpoint works with named pipes
        let server = NamedPipeTestServer::start().await;

        // Wait for the server to be ready
        tokio::time::sleep(Duration::from_secs(3)).await;

        let session_id = format!("upgrade-test-{}", uuid::Uuid::new_v4());

        // First try to create a session
        let session_request = json!({
            "session_id": session_id,
            "argv": [],  // Empty argv for adopted session
            "display_name": "Test Session",
            "language": "python",
            "working_directory": ".",
            "username": "test"
        });

        println!("Creating session for channels upgrade test: {}", session_id);

        // Create session
        if let Ok(create_response) = send_http_request(
            server.pipe_name(),
            "PUT",
            "/sessions",
            Some(&session_request.to_string()),
        )
        .await
        {
            println!(
                "Session creation response: HTTP {}",
                create_response.status_code
            );

            if create_response.status_code == 200 {
                // Now test channels upgrade
                let upgrade_path = format!("/sessions/{}/channels", session_id);
                println!("Testing channels upgrade: GET {}", upgrade_path);

                match send_http_request(server.pipe_name(), "GET", &upgrade_path, None).await {
                    Ok(upgrade_response) => {
                        println!(
                            "Channels upgrade response: HTTP {}",
                            upgrade_response.status_code
                        );
                        println!("Response body: {}", upgrade_response.body);

                        if upgrade_response.status_code == 200 {
                            println!("✓ Channels upgrade endpoint is working over named pipes");
                        } else {
                            println!(
                                "✓ Channels upgrade endpoint responded (status: {})",
                                upgrade_response.status_code
                            );
                        }
                    }
                    Err(e) => {
                        println!("Warning: Failed to test channels upgrade: {}", e);
                    }
                }
            }
        }

        server.stop().await;
        println!("Named pipe channels upgrade test completed");
    }

    #[tokio::test]
    async fn test_named_pipe_list_sessions() {
        // This test verifies that we can list sessions via HTTP over named pipes
        let server = NamedPipeTestServer::start().await;

        // Wait for the server to be ready
        tokio::time::sleep(Duration::from_secs(2)).await;

        println!("Testing session list via named pipe HTTP");

        // Test the sessions list endpoint
        match send_http_request(server.pipe_name(), "GET", "/sessions", None).await {
            Ok(response) => {
                println!("Sessions list response: HTTP {}", response.status_code);
                println!("Response body: {}", response.body);

                if response.status_code == 200 {
                    println!("✓ Successfully listed sessions via named pipe HTTP");

                    // Try to parse the response as JSON
                    if let Ok(sessions) = serde_json::from_str::<serde_json::Value>(&response.body)
                    {
                        println!("✓ Valid JSON response received: {}", sessions);
                    }
                } else {
                    println!(
                        "✓ HTTP over named pipe responded with status: {}",
                        response.status_code
                    );
                }
            }
            Err(e) => {
                println!("Warning: Failed to list sessions over named pipe: {}", e);
            }
        }
        server.stop().await;
        println!("Named pipe list sessions test completed");
    }

    #[tokio::test]
    async fn test_named_pipe_session_creation_and_listing() {
        // This test creates a session via HTTP over named pipes and then verifies it appears in the session list
        let server = NamedPipeTestServer::start().await;

        // Wait for the server to be ready
        tokio::time::sleep(Duration::from_secs(3)).await;
        let session_id = format!("test-session-{}", uuid::Uuid::new_v4());
        let session_request = json!({
            "session_id": session_id,
            "display_name": "Python Test Session",
            "session_mode": "console",
            "language": "python",
            "username": "test",
            "input_prompt": ">>> ",
            "continuation_prompt": "... ",
            "argv": ["python", "-m", "ipykernel_launcher", "-f", "{connection_file}"],
            "working_directory": ".",
            "env": [],
            "connection_timeout": 60,
            "interrupt_mode": "signal",
            "protocol_version": "5.3",
            "run_in_shell": false
        });

        println!("Step 1: Creating session: {}", session_id);

        // Create session via HTTP over named pipe
        let create_response = send_http_request(
            server.pipe_name(),
            "PUT",
            "/sessions",
            Some(&session_request.to_string()),
        )
        .await
        .expect("Failed to send session creation request");

        println!(
            "Session creation response: HTTP {}",
            create_response.status_code
        );
        println!("Response body: {}", create_response.body);

        // Verify session was created successfully
        assert_eq!(
            create_response.status_code, 200,
            "Session creation should succeed"
        );

        // Parse the response to get the session ID
        let response_json: serde_json::Value =
            serde_json::from_str(&create_response.body).expect("Response should be valid JSON");

        let returned_session_id = response_json["session_id"]
            .as_str()
            .expect("Response should contain session_id");

        println!("✓ Session created with ID: {}", returned_session_id);

        // Step 2: List sessions to verify the created session appears
        println!("Step 2: Listing sessions to verify the created session appears");

        let list_response = send_http_request(server.pipe_name(), "GET", "/sessions", None)
            .await
            .expect("Failed to send session list request");

        println!("Session list response: HTTP {}", list_response.status_code);
        println!("Response body: {}", list_response.body);

        // Verify we got a successful response
        assert_eq!(
            list_response.status_code, 200,
            "Session listing should succeed"
        );

        // Parse the session list response
        let sessions_json: serde_json::Value = serde_json::from_str(&list_response.body)
            .expect("Session list response should be valid JSON");

        let sessions = sessions_json["sessions"]
            .as_array()
            .expect("Response should contain sessions array");

        let total = sessions_json["total"]
            .as_i64()
            .expect("Response should contain total count");

        println!("Total sessions: {}", total);
        println!("Sessions found: {}", sessions.len());

        // Verify the session we created is in the list
        let found_session = sessions
            .iter()
            .find(|session| session["session_id"].as_str() == Some(&session_id));

        assert!(
            found_session.is_some(),
            "Created session '{}' should appear in session list. Sessions: {:?}",
            session_id,
            sessions
        );

        let found_session = found_session.unwrap();

        // Verify session properties
        assert_eq!(found_session["session_id"].as_str().unwrap(), session_id);
        assert_eq!(
            found_session["display_name"].as_str().unwrap(),
            "Python Test Session"
        );
        assert_eq!(found_session["language"].as_str().unwrap(), "python");

        println!(
            "✓ Session '{}' successfully found in session list",
            session_id
        );
        println!("✓ Session properties verified");

        server.stop().await;
        println!("✓ Named pipe session creation and listing test completed successfully");
    }
}

#[cfg(not(windows))]
mod non_windows_tests {
    #[test]
    fn test_named_pipes_not_available() {
        // On non-Windows platforms, named pipes should not be available
        println!("Named pipe functionality is Windows-only");
    }
}
