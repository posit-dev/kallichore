//
// lib.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Library crate for kcserver modules

#![allow(missing_docs)]

pub mod client_session;
pub mod connection_file;
pub mod error;
pub mod execution_queue;
pub mod heartbeat;
pub mod jupyter_messages;
pub mod kernel_connection;
pub mod kernel_session;
pub mod kernel_state;
pub mod registration_file;
pub mod registration_socket;
pub mod server;
pub mod startup_status;
pub mod transport;
pub mod websocket_service;
pub mod wire_message;
pub mod wire_message_header;
pub mod working_dir;
pub mod zmq_ws_proxy;