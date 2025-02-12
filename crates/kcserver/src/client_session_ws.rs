//
// client_session_ws.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//

use futures::SinkExt;
use futures::StreamExt;
use hyper::header::{HeaderValue, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, UPGRADE};
use hyper::upgrade::Upgraded;
use hyper::{Body, Response, StatusCode};
use kcshared::jupyter_message::JupyterMessage;
use swagger::ApiError;
use tokio::select;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::client_session::ClientSession;

async fn handle_ws_message(session: &ClientSession, data: String) {
    // parse the message into a JupyterMessage
    let channel_message = serde_json::from_str::<JupyterMessage>(&data);

    // if the message is not a Jupyter message, log an error and return
    let channel_message = match channel_message {
        Ok(channel_message) => channel_message,
        Err(e) => {
            log::error!(
                "Failed to parse Jupyter message: {}. Raw message: {:?}",
                e,
                data
            );
            return;
        }
    };

    // Log the message ID and type
    log::info!(
        "[client {}] Got message {} of type {}; sending to Jupyter socket {:?}",
        session.client_id,
        channel_message.header.msg_id.clone(),
        channel_message.header.msg_type.clone(),
        channel_message.channel
    );

    match session.ws_zmq_tx.send(channel_message).await {
        Ok(_) => {
            log::trace!("Sent message to Jupyter");
        }
        Err(e) => {
            log::error!("Failed to send message to Jupyter: {}", e);
        }
    }
}

pub async fn channels_websocket_request(
    mut request: hyper::Request<Body>,
    session: ClientSession,
) -> Result<Response<Body>, ApiError> {
    let derived = {
        let headers = request.headers();
        let key = headers.get(SEC_WEBSOCKET_KEY);
        key.map(|k| derive_accept_key(k.as_bytes()))
    };
    let version = request.version();

    tokio::task::spawn(async move {
        match hyper::upgrade::on(&mut request).await {
            Ok(upgraded) => {
                log::debug!("Creating session for websocket connection");

                from_upgraded(&session, upgraded).await;
            }
            Err(e) => {
                log::error!("Failed to upgrade channel connection to websocket: {}", e);
            }
        }
    });

    let upgrade = HeaderValue::from_static("Upgrade");
    let websocket = HeaderValue::from_static("websocket");
    let mut response = Response::new(Body::default());
    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    *response.version_mut() = version;
    response.headers_mut().append(CONNECTION, upgrade);
    response.headers_mut().append(UPGRADE, websocket);
    response
        .headers_mut()
        .append(SEC_WEBSOCKET_ACCEPT, derived.unwrap().parse().unwrap());
    Ok(response)
}

pub async fn from_upgraded(session: &ClientSession, upgraded: Upgraded) {
    let stream = WebSocketStream::from_raw_socket(
        upgraded,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;
    handle_channel_ws(session, stream).await;
}

pub async fn handle_channel_ws(session: &ClientSession, mut ws_stream: WebSocketStream<Upgraded>) {
    // Mark the session as connected
    {
        let mut state = session.state.write().await;

        // We should never receive a connection request for an
        // already-connected session.
        if state.connected {
            log::warn!(
                "[client {}] Received connection request for already-connected session.",
                session.client_id
            );
        } else {
            log::info!("[client {}] Connecting to websocket", session.client_id);
        }
        state.set_connected(true).await
    }

    // Interval timer for client pings
    let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(10));

    // Ping counters
    let mut ping_outbound: u64 = 0;
    let mut pong_inbound: u64 = 0;

    // Clone the client ID for use in the loop
    let client_id = session.client_id.clone();

    // Loop to handle messages from the websocket and the ZMQ channel
    loop {
        select! {
            from_socket = ws_stream.next() => {
                let message = match from_socket {
                    Some(out) => match out {
                        Ok(m) => m,
                        Err(e) => {
                            log::error!("[client {}] Failed to read data from websocket: {}", client_id, e);
                            break;
                        }
                    },
                    None => {
                        log::info!("[client {}] No data from websocket; closing", client_id);
                        break;
                    }
                };
                match message {
                    Message::Text(data) => {
                        if data.is_empty() {
                            log::info!("[client {}] Empty message from websocket; closing", client_id);
                            break;
                        }
                        handle_ws_message(session, data).await;
                    },
                    Message::Ping(data) => {
                        // Tungstenite should handle the pong response, so
                        // we just log the ping
                        log::trace!("[client {}] Got ping from websocket ({} bytes)", client_id, data.len());
                    },
                    Message::Pong(data) => {
                        // Sanity check data size
                        if data.len() != 8 {
                            log::warn!("[client {}] Got pong with invalid data size ({} bytes); ignoring", client_id, data.len());
                            continue;
                        }

                        // Log the pong and update the counter
                        let last_pong = pong_inbound;
                        pong_inbound = u64::from_be_bytes(data.as_slice().try_into().unwrap());

                        // We expect the pong to be one more than the last pong
                        if pong_inbound != last_pong + 1 {
                            log::warn!("[client {}] Got pong {} from websocket; expected {}", client_id, pong_inbound, last_pong + 1);
                        }

                        log::trace!("[client {}] Got pong {} from websocket", client_id, pong_inbound);
                    },
                    Message::Binary(data) => {
                        // Ignore binary messages for now
                        log::warn!("[client {}] Got binary message from websocket ({} bytes); ignoring", client_id, data.len());
                    },
                    Message::Frame(_) => {
                        // Ignore frame messages; these are not received by socket reads.
                    },
                    Message::Close(_) => {
                        log::info!("[client {}] Websocket closed by client", client_id);
                        break;
                    },
                }
            },
            json = session.ws_json_rx.recv() => {
                match json {
                    Ok(json) => {
                        let json = serde_json::to_string(&json).unwrap();
                        match ws_stream.send(Message::text(json)).await {
                            Ok(_) => {}
                            Err(e) => {
                                log::error!("Failed to send message to websocket: {}", e);
                            }
                        }
                    },
                    Err(e) => {
                        log::error!("Failed to receive websocket message: {}", e);
                    }
                }
            },
            _ = tick.tick() => {
                // Check to see how far behind the pong counter is
                let diff = ping_outbound - pong_inbound;
                if diff > 3 {
                    log::warn!("[client {}] Lost connection with client; websocket pong counter is behind by {} pings", client_id, diff);
                    break;
                }

                // Send a ping
                ping_outbound += 1;
                let ping_data = ping_outbound.to_be_bytes().to_vec();
                match ws_stream.send(Message::Ping(ping_data)).await {
                    Ok(_) => {
                        log::trace!("[client {}] Ping {} / Pong {}", client_id, ping_outbound, pong_inbound);
                    }
                    Err(e) => {
                        log::error!("[client {}] Failed to send ping to websocket: {}", client_id, e);
                        break;
                    }
                }
            },
            _ = session.disconnect.listen() => {
                log::info!("[client {}] Disconnecting", client_id);
                break;
            }
        }
    }

    // Mark the session as disconnected
    {
        let mut state = session.state.write().await;
        state.connected = false;
    }
}
