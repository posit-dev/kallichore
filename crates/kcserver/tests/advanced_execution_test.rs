use futures::{SinkExt, StreamExt};
use kallichore_api::{
    models::{InterruptMode, NewSession, VarAction, VarActionType},
    Api, ApiNoContext, Client, ContextWrapperExt, GetSessionResponse, NewSessionResponse,
    StartSessionResponse,
};
use kcshared::{
    jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader},
    websocket_message::WebsocketMessage,
};
use serde_json::json;
use std::{
    process::{Child, Command},
    time::Duration,
};
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tungstenite::Message;
use uuid::Uuid;

/// A test server instance
struct TestServer {
    process: Child,
    port: u16,
    base_url: String,
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
            base_url,
            client,
        }
    }

    /// Connect to a session's websocket
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

/// Helper function to create and start a Python kernel session
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

    // Create and start the session
    let create_result = server
        .client
        .new_session(new_session)
        .await
        .expect("Failed to create session");
    assert!(
        matches!(create_result, NewSessionResponse::TheSessionID(_)),
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

    // Give the kernel a moment to initialize
    sleep(Duration::from_secs(2)).await;

    session_id
}

/// Execute code and wait for results
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

    let content = json!({
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
        metadata: json!({}),
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
                                    // Check if this is related to our execution request
                                    if jupyter_msg
                                        .parent_header
                                        .as_ref()
                                        .map_or(false, |h| h.msg_id == msg_id)
                                    {
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

/// Extract the text result from execute_result messages
fn extract_result_text(messages: &[JupyterMessage]) -> Option<String> {
    for msg in messages {
        if msg.header.msg_type == "execute_result" {
            if let Some(data) = msg.content.get("data") {
                if let Some(text) = data.get("text/plain") {
                    if let Some(text_str) = text.as_str() {
                        return Some(text_str.to_string());
                    }
                }
            }
        }
    }
    None
}

#[tokio::test]
async fn test_multiple_executions() {
    // Start the server
    let server = TestServer::start().await;

    // Create a Python session
    let session_id = create_python_session(&server).await;

    // Connect to the websocket for the session
    let mut ws_stream = server.connect_to_session(session_id.clone()).await;

    // Test 1: Simple arithmetic
    let messages = execute_code(&mut ws_stream, "2 * 3 + 4").await;
    let result = extract_result_text(&messages).expect("Should have received a result");
    assert_eq!(result, "10", "Expected result of 2 * 3 + 4 to be 10");

    // Test 2: Variable assignment and retrieval
    let _ = execute_code(&mut ws_stream, "x = 42").await;
    let messages = execute_code(&mut ws_stream, "x").await;
    let result = extract_result_text(&messages).expect("Should have received a result");
    assert_eq!(result, "42", "Expected variable x to be 42");

    // Test 3: Function definition and execution
    let code = "
def add_numbers(a, b):
    return a + b
";
    let _ = execute_code(&mut ws_stream, code).await;

    let messages = execute_code(&mut ws_stream, "add_numbers(10, 20)").await;
    let result = extract_result_text(&messages).expect("Should have received a result");
    assert_eq!(result, "30", "Expected add_numbers(10, 20) to be 30");

    // Test 4: Using libraries
    let messages = execute_code(&mut ws_stream, "import math\nmath.sqrt(16)").await;
    let result = extract_result_text(&messages).expect("Should have received a result");
    assert_eq!(result, "4.0", "Expected math.sqrt(16) to be 4.0");

    // Test 5: More complex output (list)
    let messages = execute_code(&mut ws_stream, "[i**2 for i in range(5)]").await;
    let result = extract_result_text(&messages).expect("Should have received a result");
    assert_eq!(result, "[0, 1, 4, 9, 16]", "Expected correct list output");

    // Test 6: Error handling
    let messages = execute_code(&mut ws_stream, "1/0").await;

    // Verify we got an error message
    let error_messages: Vec<&JupyterMessage> = messages
        .iter()
        .filter(|msg| msg.header.msg_type == "error")
        .collect();

    assert!(
        !error_messages.is_empty(),
        "Should have received an error message for division by zero"
    );

    // Clean up
    let _ = server.client.kill_session(session_id).await;
    server.stop().await;
}
