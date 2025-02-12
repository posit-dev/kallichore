//
// client_session.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
//
//
use std::sync::atomic;
use std::sync::Arc;

use async_channel::Receiver;
use async_channel::Sender;
use kcshared::jupyter_message::JupyterMessage;
use kcshared::websocket_message::WebsocketMessage;
use once_cell::sync::Lazy;
use tokio::sync::RwLock;

use crate::kernel_connection::KernelConnection;
use crate::kernel_state::KernelState;
use event_listener::Event;
#[derive(Clone)]
pub struct ClientSession {
    /// Metadata about the connection to the Jupyter side of the kernel
    pub connection: KernelConnection,

    /// The unique ID of the client session
    pub client_id: String,

    /// The receiver for messages to be sent to the websocket
    pub ws_json_rx: Receiver<WebsocketMessage>,

    /// The sender for messages to be sent to the Jupyter kernel
    pub ws_zmq_tx: Sender<JupyterMessage>,

    /// The current state of the kernel
    pub state: Arc<RwLock<KernelState>>,

    /// An event that can be triggered to disconnect the session; used when we
    /// need to reconnect a new client to the same kernel.
    pub disconnect: Arc<Event>,
}

// An atomic counter for generating unique client IDs
static SESSION_COUNTER: Lazy<atomic::AtomicU32> = Lazy::new(|| atomic::AtomicU32::new(0));

impl ClientSession {
    pub fn new(
        connection: KernelConnection,
        ws_json_rx: Receiver<WebsocketMessage>,
        ws_zmq_tx: Sender<JupyterMessage>,
        state: Arc<RwLock<KernelState>>,
    ) -> Self {
        // Derive a unique client ID for this connection by combining the
        // session ID and a counter
        #[allow(unsafe_code)]
        let session_id = format!(
            "{}-{}",
            connection.session_id.clone(),
            SESSION_COUNTER.fetch_add(1, atomic::Ordering::SeqCst)
        );
        Self {
            connection,
            ws_json_rx,
            ws_zmq_tx,
            client_id: session_id,
            state,
            disconnect: Arc::new(Event::new()),
        }
    }
}
