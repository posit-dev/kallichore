use futures::{SinkExt, StreamExt};
use kallichore_api::{
    models::{InterruptMode, NewSession, VarAction, VarActionType},
    Api, ApiNoContext, Client, ContextWrapperExt, GetSessionResponse, KillSessionResponse,
    ListSessionsResponse, NewSessionResponse, StartSessionResponse,
};
use kcshared::{
    jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader},
    kernel_message::KernelMessage,
    websocket_message::WebsocketMessage,
};
use std::{
    path::PathBuf,
    process::{Child, Command},
    time::Duration,
};
use tokio::{net::TcpStream, time::sleep};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tungstenite::Message;
use uuid::Uuid;

struct TestServer {
    process: Child,
    port: u16,
    base_url: String,
    client: Client,
}

impl TestServer {
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
            base_url,
            client,
        }
    }

    // Connect to a session's websocket
    async fn connect_to_session(
        &self,
        session_id: String,
    ) -> WebSocketStream<MaybeTlsStream<TcpStream>> {
        let ws_url = format!(
            "ws://127.0.0.1:{}/sessions/{}/channels",
            self.port, session_id
        );
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .expect("Failed to connect to WebSocket");

        ws_stream
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

async fn execute_code(
    ws_stream: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    code: &str,
) -> Vec<JupyterMessage> {
    let msg_id = Uuid::new_v4().to_string();

    // Create execution request
    let header = JupyterMessageHeader {
        msg_id: msg_id.clone(),
        username: "test".to_string(),
        session: Uuid::new_v4().to_string(),
        date: chrono::Utc::now().to_rfc3339(),
        msg_type: "execute_request".to_string(),
        version: "5.3".to_string(),
    };

    let content = serde_json::json!({
        "code": code,
        "silent": false,
        "store_history": true,
        "user_expressions": {},
        "allow_stdin": false,
        "stop_on_error": true
    });

    let msg = JupyterMessage {
        header: header.clone(),
        parent_header: None,
        metadata: serde_json::json!({}),
        content,
        buffers: vec![],
        channel: JupyterChannel::Shell,
    };

    let ws_msg = WebsocketMessage::Jupyter(msg);
    let json = serde_json::to_string(&ws_msg).expect("Failed to serialize message");

    ws_stream
        .send(Message::Text(json))
        .await
        .expect("Failed to send message");

    // Collect all response messages until we get an idle status
    let mut results = Vec::new();
    let mut idle_received = false;

    // Wait for responses with a timeout
    let start = std::time::Instant::now();
    while !idle_received && start.elapsed() < Duration::from_secs(30) {
        // Add a small delay to avoid tight loop
        sleep(Duration::from_millis(10)).await;

        // Check for new messages
        if let Some(msg_result) = ws_stream.next().await {
            match msg_result {
                Ok(msg) => {
                    if let Message::Text(text) = msg {
                        if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(&text) {
                            match ws_msg {
                                WebsocketMessage::Jupyter(jupyter_msg) => {
                                    // Check if this is a status message with idle state
                                    if jupyter_msg.header.msg_type == "status" {
                                        if let Some(status) =
                                            jupyter_msg.content.get("execution_state")
                                        {
                                            if status == "idle" {
                                                idle_received = true;
                                            }
                                        }
                                    }
                                    results.push(jupyter_msg);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("Error receiving message: {:?}", e);
                    break;
                }
            }
        }
    }

    results
}

#[tokio::test]
async fn test_kernel_execution() {
    // Start the server
    let server = TestServer::start().await;

    // List sessions to ensure we start with an empty list
    let result = server
        .client
        .list_sessions()
        .await
        .expect("Failed to list sessions");
    if let ListSessionsResponse::ListOfActiveSessions(sessions) = result {
        assert_eq!(sessions.total, 0, "Server should start with no sessions");
    }

    // Create a new session
    let session_id = Uuid::new_v4().to_string();
    let new_session = NewSession {
        session_id: session_id.clone(),
        display_name: format!("Test Session - {}", session_id),
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

    // Create and start the session
    let result = server
        .client
        .new_session(new_session)
        .await
        .expect("Failed to create session");
    assert!(
        matches!(result, NewSessionResponse::TheSessionID(_)),
        "Failed to create session"
    );

    let start_result = server
        .client
        .start_session(session_id.clone())
        .await
        .expect("Failed to start session");
    assert!(
        matches!(start_result, StartSessionResponse::Started(_)),
        "Failed to start kernel"
    );

    // Connect to the websocket for the session
    let mut ws_stream = server.connect_to_session(session_id.clone()).await;

    // Give the kernel a moment to initialize
    sleep(Duration::from_secs(2)).await;

    // Execute some code
    let code = "2 + 3";
    let messages = execute_code(&mut ws_stream, code).await;

    // Verify we got an execute_result with the correct value
    let results: Vec<&JupyterMessage> = messages
        .iter()
        .filter(|msg| msg.header.msg_type == "execute_result")
        .collect();

    assert!(
        !results.is_empty(),
        "Should have received an execute_result message"
    );

    // Check for the expected result value (5)
    let result_msg = &results[0];
    let data = result_msg.content.get("data").expect("No data in result");
    let text_plain = data.get("text/plain").expect("No text/plain in data");
    assert_eq!(text_plain, "5", "Expected result of 2 + 3 to be 5");

    // Kill the session
    let kill_result = server
        .client
        .kill_session(session_id.clone())
        .await
        .expect("Failed to kill session");
    assert!(
        matches!(kill_result, KillSessionResponse::Killed(_)),
        "Failed to kill session"
    );

    // Verify the session is no longer active
    sleep(Duration::from_secs(1)).await;
    let result = server
        .client
        .get_session(session_id.clone())
        .await
        .expect("Failed to get session");
    if let GetSessionResponse::TheRequestedSession(session) = result {
        assert_eq!(
            session.status.to_string(),
            "exited",
            "Session should be in exited state"
        );
    }

    // Stop the server
    server.stop().await;
}

#[tokio::test]
async fn test_kernel_interrupt() {
    // Start the server
    let server = TestServer::start().await;

    // Create a new session
    let session_id = Uuid::new_v4().to_string();
    let new_session = NewSession {
        session_id: session_id.clone(),
        display_name: format!("Test Session - {}", session_id),
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

    // Create and start the session
    let _ = server
        .client
        .new_session(new_session)
        .await
        .expect("Failed to create session");
    let _ = server
        .client
        .start_session(session_id.clone())
        .await
        .expect("Failed to start session");

    // Connect to the websocket for the session
    let mut ws_stream = server.connect_to_session(session_id.clone()).await;

    // Give the kernel a moment to initialize
    sleep(Duration::from_secs(2)).await;

    // Execute a long-running piece of code that we'll interrupt
    let code = "import time\nfor i in range(10):\n    print(f'Iteration {i}')\n    time.sleep(1)";

    // Start execution in the background
    tokio::spawn({
        let mut ws_stream_clone = ws_stream.clone();
        async move {
            execute_code(&mut ws_stream_clone, code).await;
        }
    });

    // Give execution a moment to start
    sleep(Duration::from_secs(1)).await;

    // Send interrupt request
    let interrupt_result = server
        .client
        .interrupt_session(session_id.clone())
        .await
        .expect("Failed to interrupt session");

    // Collect messages after interrupt
    let mut interrupt_confirmed = false;
    let mut timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();

    while !interrupt_confirmed && start.elapsed() < timeout {
        if let Some(Ok(Message::Text(text))) = ws_stream.next().await {
            if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(&text) {
                match ws_msg {
                    WebsocketMessage::Jupyter(jupyter_msg) => {
                        // Look for interrupt notification messages
                        if jupyter_msg.header.msg_type == "status"
                            || jupyter_msg.header.msg_type == "interrupt_reply"
                        {
                            interrupt_confirmed = true;
                            break;
                        }
                    }
                    WebsocketMessage::Kernel(kernel_msg) => match kernel_msg {
                        KernelMessage::Interrupted => {
                            interrupt_confirmed = true;
                            break;
                        }
                        _ => {}
                    },
                }
            }
        }
        sleep(Duration::from_millis(100)).await;
    }

    assert!(
        interrupt_confirmed,
        "Should have received interrupt confirmation"
    );

    // Kill the session
    let _ = server.client.kill_session(session_id).await;

    // Stop the server
    server.stop().await;
}
