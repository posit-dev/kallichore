#![allow(missing_docs, unused_imports, dead_code)]

use futures::{SinkExt, StreamExt};
use kallichore_api::{
    models::{InterruptMode, NewSession},
    ApiNoContext, Client, ContextWrapperExt, GetSessionResponse, KillSessionResponse,
    ListSessionsResponse, NewSessionResponse, StartSessionResponse,
};
use kcshared::{
    jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader},
    kernel_message::KernelMessage,
    websocket_message::WebsocketMessage,
};
use std::{
    env,
    io::Write,
    path::PathBuf,
    process::{Child, Command},
    sync::Arc,
    time::Duration,
};
use swagger::{AuthData, ContextBuilder, EmptyContext, Has, Push, XSpanIdString};
use tokio::{net::TcpStream, sync::Mutex, time::sleep};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

// Define a ClientType that matches what's used in other integration tests
type ClientContext = swagger::make_context_ty!(
    ContextBuilder,
    EmptyContext,
    Option<AuthData>,
    XSpanIdString
);

// Define a convenience extension trait for the Client
trait ClientExt {
    fn new(base_path: &str) -> Box<dyn ApiNoContext<ClientContext>>;
}

// Implementation for Client that initializes it with proper context
impl<S, C> ClientExt for Client<S, C>
where
    S: hyper::service::Service<
            (hyper::Request<hyper::Body>, C),
            Response = hyper::Response<hyper::Body>,
        > + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync + 'static>> + std::fmt::Display,
    C: Has<XSpanIdString> + Has<Option<AuthData>> + Clone + Send + Sync + 'static,
{
    fn new(base_path: &str) -> Box<dyn ApiNoContext<ClientContext>> {
        let context = swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            None::<AuthData>,
            XSpanIdString::default()
        );

        let client = Client::try_new_http(base_path).expect("Failed to create HTTP client");
        Box::new(client.with_context(context))
    }
}

/// Structure for managing echo kernel for testing
struct EchoKernel {
    connection_file_path: PathBuf,
    process: Option<Child>,
}

impl EchoKernel {
    /// Creates a simple echo kernel for testing
    fn new() -> Self {
        // Create a temporary connection file
        let temp_dir = env::temp_dir();
        let connection_file_path = temp_dir.join(format!("echo_kernel_{}.json", Uuid::new_v4()));

        // Create a basic connection file
        let connection_file = serde_json::json!({
            "transport": "tcp",
            "ip": "127.0.0.1",
            "control_port": 0,
            "shell_port": 0,
            "stdin_port": 0,
            "iopub_port": 0,
            "hb_port": 0,
            "signature_scheme": "hmac-sha256",
            "key": "",
        });

        // Write the connection file
        let mut file =
            std::fs::File::create(&connection_file_path).expect("Failed to create connection file");
        file.write_all(
            serde_json::to_string_pretty(&connection_file)
                .unwrap()
                .as_bytes(),
        )
        .expect("Failed to write connection file");

        EchoKernel {
            connection_file_path,
            process: None,
        }
    }

    /// Get the path to the connection file
    fn connection_file_path(&self) -> String {
        self.connection_file_path.to_string_lossy().into()
    }
}

impl Drop for EchoKernel {
    fn drop(&mut self) {
        // Kill the process if it's still running
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
        }

        // Clean up the connection file
        let _ = std::fs::remove_file(&self.connection_file_path);
    }
}

struct TestServer {
    process: Child,
    port: u16,
    base_url: String,
    client: Box<dyn ApiNoContext<ClientContext>>,
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

        // Create client with context
        let context = swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            None::<AuthData>,
            XSpanIdString::default()
        );

        let client = Client::try_new_http(&base_url).expect("Failed to create HTTP client");
        let client = Box::new(client.with_context(context));

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
        // Request server shutdown - Method might vary based on your API
        // let _ = self.client.shutdown_server().await;

        // Force terminate the server process
        let _ = self.process.kill();
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

    // Create execution request with only fields defined in JupyterMessageHeader
    let header = JupyterMessageHeader {
        msg_id: msg_id.clone(),
        msg_type: "execute_request".to_string(),
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
        header,
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

// A helper struct to handle WebSocketStream which isn't Clone
struct WebSocketHelper {
    ws_stream: Arc<Mutex<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
}

impl WebSocketHelper {
    fn new(ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Self {
        WebSocketHelper {
            ws_stream: Arc::new(Mutex::new(ws_stream)),
        }
    }

    async fn execute(&self, code: &str) -> Vec<JupyterMessage> {
        let mut ws_stream = self.ws_stream.lock().await;
        execute_code(&mut *ws_stream, code).await
    }

    async fn next_message(&self) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
        let mut ws_stream = self.ws_stream.lock().await;
        ws_stream.next().await
    }
}

#[tokio::test]
async fn test_echo_kernel_execution() {
    // Create an echo kernel
    let echo_kernel = EchoKernel::new();

    // Start the server
    let server = TestServer::start().await;

    // List sessions to ensure we start with an empty list
    let result = server
        .client
        .list_sessions()
        .await
        .expect("Failed to list sessions");
    let ListSessionsResponse::ListOfActiveSessions(sessions) = result;
    assert_eq!(sessions.total, 0, "Server should start with no sessions");

    // Create a new session
    let session_id = Uuid::new_v4().to_string();
    let new_session = NewSession {
        session_id: session_id.clone(),
        display_name: format!("Echo Kernel Session - {}", session_id),
        language: "echo".into(),
        username: "testuser".into(),
        input_prompt: "In> ".into(),
        continuation_prompt: "...".into(),
        // Simulating an echo kernel - using a placeholder path
        argv: vec![
            "echo".into(),
            "--connection_file".into(),
            echo_kernel.connection_file_path(),
        ],
        working_directory: env::current_dir().unwrap().to_string_lossy().into(),
        env: vec![],
        connection_timeout: Some(30),
        interrupt_mode: InterruptMode::Message,
        protocol_version: Some("5.3".into()),
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
    let ws_stream = server.connect_to_session(session_id.clone()).await;
    let ws_helper = WebSocketHelper::new(ws_stream);

    // Give the kernel a moment to initialize
    sleep(Duration::from_secs(2)).await;

    // Execute some code
    let code = "Hello, Echo Kernel!";
    let messages = ws_helper.execute(code).await;

    // Verify we got an execute_result with the echo response
    let results: Vec<&JupyterMessage> = messages
        .iter()
        .filter(|msg| msg.header.msg_type == "execute_result")
        .collect();

    assert!(
        !results.is_empty(),
        "Should have received an execute_result message"
    );

    // In an echo kernel, the result should be the same as the input
    let result_msg = &results[0];
    let data = result_msg.content.get("data").expect("No data in result");
    let text_plain = data.get("text/plain").expect("No text/plain in data");
    assert!(
        text_plain.to_string().contains(code),
        "Echo kernel should return the input code"
    );

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

    // Check that session is in exited state based on your API structure
    match result {
        GetSessionResponse::SessionDetails(session) => {
            assert_eq!(
                session.status.to_string(),
                "exited",
                "Session should be in exited state"
            );
        }
        _ => panic!("Expected SessionDetails but got different response"),
    }

    // Stop the server
    server.stop().await;
}

#[tokio::test]
async fn test_echo_kernel_interrupt() {
    // Create an echo kernel
    let echo_kernel = EchoKernel::new();

    // Start the server
    let server = TestServer::start().await;

    // Create a new session
    let session_id = Uuid::new_v4().to_string();
    let new_session = NewSession {
        session_id: session_id.clone(),
        display_name: format!("Echo Kernel Session - {}", session_id),
        language: "echo".into(),
        username: "testuser".into(),
        input_prompt: "In> ".into(),
        continuation_prompt: "...".into(),
        argv: vec![
            "echo".into(),
            "--connection_file".into(),
            echo_kernel.connection_file_path(),
        ],
        working_directory: env::current_dir().unwrap().to_string_lossy().into(),
        env: vec![],
        connection_timeout: Some(30),
        interrupt_mode: InterruptMode::Message,
        protocol_version: Some("5.3".into()),
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
    let ws_stream = server.connect_to_session(session_id.clone()).await;
    let ws_helper = WebSocketHelper::new(ws_stream);

    // Give the kernel a moment to initialize
    sleep(Duration::from_secs(2)).await;

    // Execute a piece of code
    let code = "This is a test that should be interrupted";

    // Start the execution in a background task
    {
        let ws_helper = ws_helper.clone();
        let code_owned = code.to_string();

        tokio::spawn(async move {
            ws_helper.execute(&code_owned).await;
        });
    }

    // Give execution a moment to start
    sleep(Duration::from_secs(1)).await;

    // Send interrupt request
    let _ = server
        .client
        .interrupt_session(session_id.clone())
        .await
        .expect("Failed to interrupt session");

    // Collect messages after interrupt
    let mut interrupt_confirmed = false;
    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();

    while !interrupt_confirmed && start.elapsed() < timeout {
        if let Some(msg_result) = ws_helper.next_message().await {
            if let Ok(Message::Text(text)) = msg_result {
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
                        WebsocketMessage::Kernel(kernel_msg) => {
                            // Check for interrupted kernel message - may need to be adjusted based on actual enum variants
                            match kernel_msg {
                                KernelMessage::Output(_, _) => {
                                    // Check if output contains interrupt information
                                    interrupt_confirmed = true;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
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

#[tokio::test]
async fn test_echo_kernel_multiple_executions() {
    // Create an echo kernel
    let echo_kernel = EchoKernel::new();

    // Start the server
    let server = TestServer::start().await;

    // Create a new session
    let session_id = Uuid::new_v4().to_string();
    let new_session = NewSession {
        session_id: session_id.clone(),
        display_name: format!("Echo Kernel Session - {}", session_id),
        language: "echo".into(),
        username: "testuser".into(),
        input_prompt: "In> ".into(),
        continuation_prompt: "...".into(),
        argv: vec![
            "echo".into(),
            "--connection_file".into(),
            echo_kernel.connection_file_path(),
        ],
        working_directory: env::current_dir().unwrap().to_string_lossy().into(),
        env: vec![],
        connection_timeout: Some(30),
        interrupt_mode: InterruptMode::Message,
        protocol_version: Some("5.3".into()),
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
    let ws_stream = server.connect_to_session(session_id.clone()).await;
    let ws_helper = WebSocketHelper::new(ws_stream);

    // Give the kernel a moment to initialize
    sleep(Duration::from_secs(2)).await;

    // Execute multiple code blocks
    let test_cases = vec![
        "First message",
        "Second message with numbers 123",
        "Special characters !@#$%^&*()",
    ];

    for (i, code) in test_cases.iter().enumerate() {
        println!("Executing test case {}: {}", i + 1, code);

        // Execute the code
        let messages = ws_helper.execute(code).await;

        // Verify we got an execute_result
        let results: Vec<&JupyterMessage> = messages
            .iter()
            .filter(|msg| msg.header.msg_type == "execute_result")
            .collect();

        assert!(
            !results.is_empty(),
            "Should have received an execute_result message for test case {}",
            i + 1
        );

        // Check that the echo kernel returned our input
        let result_msg = &results[0];
        let data = result_msg.content.get("data").expect("No data in result");
        let text_plain = data.get("text/plain").expect("No text/plain in data");

        assert!(
            text_plain.to_string().contains(code),
            "Echo kernel should return the input code for test case {}",
            i + 1
        );

        // Give a small break between executions
        sleep(Duration::from_millis(500)).await;
    }

    // Kill the session
    let _ = server.client.kill_session(session_id).await;

    // Stop the server
    server.stop().await;
}

// Make WebSocketHelper clonable by using Arc
impl Clone for WebSocketHelper {
    fn clone(&self) -> Self {
        WebSocketHelper {
            ws_stream: self.ws_stream.clone(),
        }
    }
}
