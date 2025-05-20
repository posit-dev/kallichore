use kallichore_api::{
    models::{InterruptMode, NewSession, ServerConfiguration, VarAction, VarActionType},
    Api, Client, ClientHeartbeatResponse, DeleteSessionResponse, GetServerConfigurationResponse,
    GetSessionResponse, ListSessionsResponse, NewSessionResponse, ServerStatusResponse,
    SetServerConfigurationResponse, StartSessionResponse,
};
use std::{
    process::{Child, Command},
    time::Duration,
};
use tokio::time::sleep;
use uuid::Uuid;

/// A test server instance for API testing
struct TestServer {
    process: Child,
    port: u16,
    client: Client,
}

impl TestServer {
    /// Start a new test server on a random port
    async fn start() -> Self {
        // Find an available port
        let port = portpicker::pick_unused_port().expect("No available ports");

        // Start server process
        let process = Command::new("cargo")
            .args([
                "run",
                "--bin",
                "kcserver",
                "--",
                "--port",
                &port.to_string(),
                "--token",
                "none",
            ])
            .spawn()
            .expect("Failed to start server");

        // Give the server time to start
        sleep(Duration::from_secs(2)).await;

        // Create API client
        let base_url = format!("http://127.0.0.1:{}", port);
        let client = Client::new(&base_url);

        println!("Server started on port {}", port);

        TestServer {
            process,
            port,
            client,
        }
    }

    async fn stop(mut self) {
        // Request server shutdown
        let _ = self.client.shutdown_server().await;

        // Wait for process to exit
        let _ = self.process.wait();
        println!("Server stopped");
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Force kill if not already stopped
        let _ = self.process.kill();
    }
}

/// Helper function to create a test Python session
async fn create_python_session(server: &TestServer) -> String {
    let session_id = Uuid::new_v4().to_string();

    let new_session = NewSession {
        session_id: session_id.clone(),
        display_name: format!("Test Python Session - {}", session_id),
        language: "python".to_string(),
        username: "testuser".to_string(),
        input_prompt: "In> ".to_string(),
        continuation_prompt: "...".to_string(),
        argv: vec![
            "python".to_string(),
            "-m".to_string(),
            "ipykernel_launcher".to_string(),
        ],
        working_directory: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        env: vec![],
        connection_timeout: Some(30),
        interrupt_mode: InterruptMode::Message,
        protocol_version: Some("5.3".to_string()),
        run_in_shell: Some(false),
    };

    // Create the session
    let create_result = server
        .client
        .new_session(new_session)
        .await
        .expect("Failed to create session");
    assert!(
        matches!(create_result, NewSessionResponse::TheSessionID(_)),
        "Failed to create session"
    );

    session_id
}

#[tokio::test]
async fn test_api_endpoints() {
    // Start the server
    let server = TestServer::start().await;

    // Test 1: Check server status
    let status_result = server
        .client
        .server_status()
        .await
        .expect("Failed to get server status");
    if let ServerStatusResponse::ServerStatusAndInformation(status) = status_result {
        assert_eq!(status.sessions, 0, "Server should start with no sessions");
        assert_eq!(
            status.active, 0,
            "Server should start with no active sessions"
        );
        assert!(!status.busy, "Server should not be busy initially");
    } else {
        panic!("Unexpected response from server_status endpoint");
    }

    // Test 2: Check server configuration
    let config_result = server
        .client
        .get_server_configuration()
        .await
        .expect("Failed to get server configuration");
    if let GetServerConfigurationResponse::TheCurrentServerConfiguration(config) = config_result {
        println!(
            "Server configuration: idle_shutdown_hours={:?}",
            config.idle_shutdown_hours
        );
    } else {
        panic!("Unexpected response from get_server_configuration endpoint");
    }

    // Test 3: Send heartbeat
    let heartbeat_result = server
        .client
        .client_heartbeat()
        .await
        .expect("Failed to send heartbeat");
    assert!(
        matches!(
            heartbeat_result,
            ClientHeartbeatResponse::HeartbeatReceived(_)
        ),
        "Heartbeat should be acknowledged"
    );

    // Test 4: Create a session
    let session_id = create_python_session(&server).await;

    // Test 5: List sessions
    let list_result = server
        .client
        .list_sessions()
        .await
        .expect("Failed to list sessions");
    if let ListSessionsResponse::ListOfActiveSessions(sessions) = list_result {
        assert_eq!(sessions.total, 1, "Server should have one session");
        assert_eq!(
            sessions.sessions[0].session_id, session_id,
            "Session ID should match"
        );
    } else {
        panic!("Unexpected response from list_sessions endpoint");
    }

    // Test 6: Get session details
    let get_result = server
        .client
        .get_session(session_id.clone())
        .await
        .expect("Failed to get session");
    if let GetSessionResponse::TheRequestedSession(session) = get_result {
        assert_eq!(session.session_id, session_id, "Session ID should match");
        assert_eq!(session.language, "python", "Language should be Python");
    } else {
        panic!("Unexpected response from get_session endpoint");
    }

    // Test 7: Start the session
    let start_result = server
        .client
        .start_session(session_id.clone())
        .await
        .expect("Failed to start session");
    assert!(
        matches!(start_result, StartSessionResponse::Started(_)),
        "Session should start successfully"
    );

    // Give the kernel time to initialize
    sleep(Duration::from_secs(2)).await;

    // Test 8: Get connection info
    let conn_result = server
        .client
        .connection_info(session_id.clone())
        .await
        .expect("Failed to get connection info");
    assert!(
        conn_result.to_string().contains("ConnectionInfoResponse"),
        "Should receive connection info"
    );

    // Test 9: Interrupt the session
    let interrupt_result = server
        .client
        .interrupt_session(session_id.clone())
        .await
        .expect("Failed to interrupt session");
    assert!(
        interrupt_result.to_string().contains("Interrupted"),
        "Session should be interruptible"
    );

    // Test 10: Update server configuration
    let new_config = ServerConfiguration {
        idle_shutdown_hours: Some(5),
        log_level: Some("debug".to_string()),
    };

    let set_config_result = server
        .client
        .set_server_configuration(new_config)
        .await
        .expect("Failed to set server configuration");
    assert!(
        matches!(
            set_config_result,
            SetServerConfigurationResponse::ConfigurationUpdated(_)
        ),
        "Configuration should update successfully"
    );

    // Verify the configuration was updated
    let config_result = server
        .client
        .get_server_configuration()
        .await
        .expect("Failed to get server configuration");
    if let GetServerConfigurationResponse::TheCurrentServerConfiguration(config) = config_result {
        assert_eq!(
            config.idle_shutdown_hours,
            Some(5),
            "idle_shutdown_hours should be updated to 5"
        );
    }

    // Test 11: Delete the session (first kill it)
    // First kill it so it's in exited state
    let _ = server.client.kill_session(session_id.clone()).await;

    // Wait for the session to exit
    sleep(Duration::from_secs(1)).await;

    // Then delete it
    let delete_result = server
        .client
        .delete_session(session_id.clone())
        .await
        .expect("Failed to delete session");
    assert!(
        matches!(delete_result, DeleteSessionResponse::Deleted(_)),
        "Session should be deleted successfully"
    );

    // Clean up
    server.stop().await;
}
