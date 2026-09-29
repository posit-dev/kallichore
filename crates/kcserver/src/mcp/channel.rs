//
// channel.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Pumps the WebSocket that connects a Positron window to the supervisor.
//!
//! Several windows may share a workspace record, so channels accumulate rather
//! than replacing each other; the registry brokers commands to whichever
//! window the user last focused.

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
    workspace_id: String,
    mut stream: WebSocketStream<TokioIo<Upgraded>>,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerFrontendMessage>();
    let Some(generation) = state.registry.attach_channel(&workspace_id, tx).await else {
        log::warn!(
            "MCP frontend channel opened for unregistered workspace '{}'",
            workspace_id
        );
        let _ = stream.close(None).await;
        return;
    };
    log::info!("MCP frontend channel connected for '{}'", workspace_id);

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
                        workspace_id,
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
                                state
                                    .registry
                                    .handle_message(&workspace_id, generation, message)
                                    .await
                            }
                            Err(e) => log::warn!(
                                "MCP frontend channel for '{}' sent an unreadable frame: {}",
                                workspace_id,
                                e
                            ),
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        log::debug!(
                            "MCP frontend channel for '{}' ended: {}",
                            workspace_id,
                            e
                        );
                        break;
                    }
                }
            }
        }
    }

    // Deregistering the workspace also ends the loop, by dropping the sender;
    // close properly so the window sees a normal closure rather than a
    // dropped connection.
    let _ = stream.close(None).await;
    state
        .registry
        .detach_channel(&workspace_id, generation)
        .await;
    log::info!("MCP frontend channel disconnected for '{}'", workspace_id);
}
