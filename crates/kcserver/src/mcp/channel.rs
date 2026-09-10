//
// channel.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Pumps the WebSocket that connects a registered frontend to the supervisor.
//!
//! Reconnection replaces the channel: the last window to connect owns command
//! brokering, matching how session WebSockets already behave.

use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use kcshared::mcp_frontend::{FrontendMessage, ServerFrontendMessage};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::McpState;

/// Read frontend frames and write brokered command requests until the socket
/// closes.
pub async fn run(
    state: Arc<McpState>,
    frontend_id: String,
    mut stream: WebSocketStream<TokioIo<Upgraded>>,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerFrontendMessage>();
    let Some(generation) = state.registry.attach_channel(&frontend_id, tx).await else {
        log::warn!(
            "MCP frontend channel opened for unregistered frontend '{}'",
            frontend_id
        );
        let _ = stream.close(None).await;
        return;
    };
    log::info!("MCP frontend channel connected for '{}'", frontend_id);

    loop {
        tokio::select! {
            outbound = rx.recv() => {
                let Some(message) = outbound else { break };
                let text = match serde_json::to_string(&message) {
                    Ok(text) => text,
                    Err(e) => {
                        log::error!("Failed to serialize MCP frontend message: {}", e);
                        continue;
                    }
                };
                if let Err(e) = stream.send(Message::Text(text)).await {
                    log::debug!(
                        "MCP frontend channel for '{}' failed to send: {}",
                        frontend_id,
                        e
                    );
                    break;
                }
            }
            inbound = stream.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<FrontendMessage>(&text) {
                            Ok(message) => {
                                state.registry.handle_message(&frontend_id, message).await
                            }
                            Err(e) => log::warn!(
                                "MCP frontend '{}' sent an unreadable frame: {}",
                                frontend_id,
                                e
                            ),
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        log::debug!(
                            "MCP frontend channel for '{}' ended: {}",
                            frontend_id,
                            e
                        );
                        break;
                    }
                }
            }
        }
    }

    state
        .registry
        .detach_channel(&frontend_id, generation)
        .await;
    log::info!("MCP frontend channel disconnected for '{}'", frontend_id);
}
