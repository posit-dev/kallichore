//
// transport.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//

//! Transport layer utilities for testing different communication channels

use futures::{SinkExt, StreamExt};
use kcshared::websocket_message::WebsocketMessage;
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Clone)]
#[allow(dead_code)]
pub enum TransportType {
    Websocket,
    #[cfg(unix)]
    DomainSocket,
    #[cfg(windows)]
    NamedPipe,
}

pub enum CommunicationChannel {
    Websocket {
        sender: futures::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
        receiver: futures::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
    },
    #[cfg(unix)]
    DomainSocket {
        sender: futures::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>,
            Message,
        >,
        receiver: futures::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>,
        >,
    },
    #[cfg(windows)]
    NamedPipe {
        pipe: tokio::net::windows::named_pipe::NamedPipeClient,
    },
}

impl CommunicationChannel {
    /// Send a message over the communication channel
    pub async fn send_message(
        &mut self,
        message: &WebsocketMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message_json = serde_json::to_string(message)?;
        match self {
            CommunicationChannel::Websocket { sender, .. } => {
                sender.send(Message::Text(message_json)).await?;
            }
            #[cfg(unix)]
            CommunicationChannel::DomainSocket { sender, .. } => {
                sender.send(Message::Text(message_json)).await?;
            }
            #[cfg(windows)]
            CommunicationChannel::NamedPipe { pipe } => {
                use tokio::io::AsyncWriteExt;
                let message_with_newline = format!("{}\n", message_json);
                pipe.write_all(message_with_newline.as_bytes()).await?;
                pipe.flush().await?;
            }
        }
        Ok(())
    }

    /// Receive a message from the communication channel
    pub async fn receive_message(
        &mut self,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            CommunicationChannel::Websocket { receiver, .. } => match receiver.next().await {
                Some(Ok(Message::Text(text))) => Ok(Some(text)),
                Some(Ok(Message::Binary(data))) => {
                    Ok(Some(format!("binary({} bytes)", data.len())))
                }
                Some(Ok(Message::Ping(_))) => Ok(Some("ping".to_string())),
                Some(Ok(Message::Pong(_))) => Ok(Some("pong".to_string())),
                Some(Ok(Message::Close(_))) => Ok(None),
                Some(Ok(Message::Frame(_))) => Ok(Some("frame".to_string())),
                Some(Err(e)) => Err(Box::new(e)),
                None => Ok(None),
            },
            #[cfg(unix)]
            CommunicationChannel::DomainSocket { receiver, .. } => match receiver.next().await {
                Some(Ok(Message::Text(text))) => Ok(Some(text)),
                Some(Ok(Message::Binary(data))) => {
                    Ok(Some(format!("binary({} bytes)", data.len())))
                }
                Some(Ok(Message::Ping(_))) => Ok(Some("ping".to_string())),
                Some(Ok(Message::Pong(_))) => Ok(Some("pong".to_string())),
                Some(Ok(Message::Close(_))) => Ok(None),
                Some(Ok(Message::Frame(_))) => Ok(Some("frame".to_string())),
                Some(Err(e)) => Err(Box::new(e)),
                None => Ok(None),
            },
            #[cfg(windows)]
            CommunicationChannel::NamedPipe { pipe } => {
                use tokio::io::AsyncReadExt;
                let mut buffer = vec![0; 4096];
                match pipe.read(&mut buffer).await {
                    Ok(0) => Ok(None), // EOF
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buffer[..n]).trim().to_string();
                        Ok(Some(text))
                    }
                    Err(e) => Err(Box::new(e)),
                }
            }
        }
    }

    /// Close the communication channel
    pub async fn close(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match self {
            CommunicationChannel::Websocket { sender, .. } => {
                let _ = sender.send(Message::Close(None)).await;
            }
            #[cfg(unix)]
            CommunicationChannel::DomainSocket { sender, .. } => {
                let _ = sender.send(Message::Close(None)).await;
            }
            #[cfg(windows)]
            CommunicationChannel::NamedPipe { pipe: _ } => {
                // Named pipe will close when dropped
            }
        }
        Ok(())
    }

    /// Create a websocket communication channel
    pub async fn create_websocket(
        ws_url: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (ws_stream, _) = connect_async(ws_url).await?;
        let (ws_sender, ws_receiver) = ws_stream.split();
        Ok(CommunicationChannel::Websocket {
            sender: ws_sender,
            receiver: ws_receiver,
        })
    }

    #[cfg(unix)]
    /// Create a domain socket communication channel
    pub async fn create_domain_socket(
        socket_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let stream = tokio::net::UnixStream::connect(socket_path).await?;
        let ws_stream = tokio_tungstenite::WebSocketStream::from_raw_socket(
            stream,
            tokio_tungstenite::tungstenite::protocol::Role::Client,
            None,
        )
        .await;
        let (sender, receiver) = ws_stream.split();
        Ok(CommunicationChannel::DomainSocket { sender, receiver })
    }

    #[cfg(windows)]
    /// Create a named pipe communication channel
    pub async fn create_named_pipe(
        pipe_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let pipe = tokio::net::windows::named_pipe::ClientOptions::new().open(pipe_path)?;
        Ok(CommunicationChannel::NamedPipe { pipe })
    }
}

/// Communication test results tracking
#[derive(Default, Debug)]
pub struct CommunicationTestResults {
    pub message_count: u32,
    pub received_jupyter_messages: u32,
    pub received_kernel_messages: u32,
    pub kernel_info_reply_received: bool,
    pub execute_reply_received: bool,
    pub stream_output_received: bool,
    pub expected_output_found: bool,
    pub collected_output: String,
}

impl CommunicationTestResults {
    /// Process a received message and update test results
    pub fn process_message(&mut self, text: &str) {
        self.message_count += 1;
        if text == "ping" || text == "pong" || text == "timeout" || text == "empty" {
            return;
        }
        if text.starts_with("binary(") || text.starts_with("other:") {
            println!("Received {}", text);
            return;
        }

        if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(text) {
            match ws_msg {
                WebsocketMessage::Jupyter(jupyter_msg) => {
                    self.received_jupyter_messages += 1;
                    println!("  -> Jupyter: {}", jupyter_msg.header.msg_type);
                    match jupyter_msg.header.msg_type.as_str() {
                        "kernel_info_reply" => {
                            self.kernel_info_reply_received = true;
                        }
                        "execute_reply" => {
                            self.execute_reply_received = true;
                        }
                        "stream" => {
                            self.stream_output_received = true;
                            if let Some(text_content) = jupyter_msg.content.get("text") {
                                let output_text = text_content.as_str().unwrap_or("");
                                self.collected_output.push_str(output_text);
                                println!("    Stream output: {}", output_text);
                            }
                        }
                        _ => {}
                    }
                }
                WebsocketMessage::Kernel(_kernel_msg) => {
                    self.received_kernel_messages += 1;
                    println!("  -> Kernel message received");
                }
            }
        } else if text.contains("\"type\":\"ping\"") || text.contains("\"type\":\"disconnect\"") {
            println!("  -> Server control message: {}", text);
        } else {
            println!("  -> Unknown message: {}", text);
        }

        // Check for expected output
        if self
            .collected_output
            .contains("Hello from Kallichore test!")
            && self.collected_output.contains("2 + 3 = 5")
        {
            self.expected_output_found = true;
        }
    }

    /// Assert that the test results meet the minimum requirements
    pub fn assert_success(&self) {
        assert!(
            self.received_jupyter_messages > 0,
            "Expected to receive Jupyter messages from the Python kernel, but got {}. The kernel may not be starting or communicating properly.",
            self.received_jupyter_messages
        );

        assert!(
            self.kernel_info_reply_received,
            "Expected to receive kernel_info_reply from Python kernel, but didn't get one. The kernel is not responding to basic requests."
        );

        assert!(
            self.execute_reply_received,
            "Expected to receive execute_reply from Python kernel, but didn't get one. The kernel is not executing code properly."
        );

        assert!(
            self.stream_output_received,
            "Expected to receive stream output from Python kernel, but didn't get any. The kernel is not producing stdout output."
        );

        assert!(
            self.expected_output_found,
            "Expected to find 'Hello from Kallichore test!' and '2 + 3 = 5' in the kernel output, but didn't find both. The kernel executed but produced unexpected output. Actual collected output: {:?}",
            self.collected_output
        );
    }

    /// Print a summary of the test results
    pub fn print_summary(&self) {
        println!("Communication test completed:");
        println!("  - Total messages: {}", self.message_count);
        println!("  - Jupyter messages: {}", self.received_jupyter_messages);
        println!("  - Kernel messages: {}", self.received_kernel_messages);
        println!("  - Kernel info reply: {}", self.kernel_info_reply_received);
        println!("  - Execute reply: {}", self.execute_reply_received);
        println!("  - Stream output: {}", self.stream_output_received);
        println!("  - Expected output found: {}", self.expected_output_found);
        println!("  - Collected output: {:?}", self.collected_output);
    }
}

/// Run a communication test with timeout and message processing
pub async fn run_communication_test(
    comm: &mut CommunicationChannel,
    timeout: Duration,
    max_messages: u32,
) -> CommunicationTestResults {
    let mut results = CommunicationTestResults::default();
    let start_time = std::time::Instant::now();

    println!("Listening for kernel responses...");

    while start_time.elapsed() < timeout && results.message_count < max_messages {
        println!(
            "Waiting for message... (elapsed: {:.1}s)",
            start_time.elapsed().as_secs_f32()
        );

        match comm.receive_message().await {
            Ok(Some(text)) => {
                results.process_message(&text);
            }
            Ok(None) => {
                println!("Communication channel closed");
                break;
            }
            Err(e) => {
                println!("Communication error: {}", e);
                break;
            }
        }
    }

    results
}
