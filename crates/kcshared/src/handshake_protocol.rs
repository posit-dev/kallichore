//
// handshake_protocol.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

// This file contains the protocol for negotiating ports for ZeroMQ sockets
// between the kernel and the supervisor.
//
// In the traditional Jupyter protocol, the client (in this case, the
// supervisor) selects a set of ports for the shell, IOPub, stdin, control, and
// heartbeat channels, then write a connection file for the kernel to read. The
// kernel reads this file at startup and binds to the specified ports. This has
// a race condition because other processes can bind to the same ports before
// the kernel starts.
//
// Using the Jupyter Enhanced Proposal (JEP) 66, the kernel uses a REQ/REP
// registration socket to negotiate ports. It sends the kernel the address of
// the registration socket, and the kernel replies with the ports it has
// selected.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents the handshake version supported by a kernel
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeVersion {
    /// Major version number
    pub major: u32,

    /// Minor version number
    pub minor: u32,
}

/// Protocol version indicating support for JEP 66 handshaking
pub const JEP66_PROTOCOL_VERSION: &str = "5.5";

/// Status returned in the handshake reply
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HandshakeStatus {
    /// The handshake was successful
    #[serde(rename = "ok")]
    Ok,

    /// The handshake failed
    #[serde(rename = "error")]
    Error,
}

/// The information sent from the kernel to the supervisor after it has
/// selected a set of ports
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeRequest {
    /// The port for the shell channel
    pub shell_port: u16,

    /// The port for the IOPub channel
    pub iopub_port: u16,

    /// The port for the stdin channel
    pub stdin_port: u16,

    /// The port for the control channel
    pub control_port: u16,

    /// The port for the heartbeat channel
    pub hb_port: u16,
}

/// The response sent by the kernel to accept or reject the connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeReply {
    /// Status of the handshake (ok or error)
    pub status: HandshakeStatus,

    /// Optional error message if status is Error
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Additional capabilities supported by the kernel
    #[serde(default)]
    pub capabilities: HashMap<String, serde_json::Value>,
}

/// Information needed for the registration process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationInfo {
    /// The transport protocol (e.g. "tcp")
    pub transport: String,

    /// The signature scheme (e.g. "hmac-sha256")
    pub signature_scheme: String,

    /// The IP address
    pub ip: String,

    /// The key used for message signing
    pub key: String,

    /// The registration port
    pub registration_port: u16,
}

impl HandshakeVersion {
    /// Create a new handshake version
    pub fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Returns the current version supported by the supervisor
    pub fn current() -> Self {
        Self { major: 5, minor: 5 }
    }

    /// Check if the kernel's protocol version supports handshaking (>= 5.5)
    pub fn supports_handshaking(protocol_version: &str) -> bool {
        // Parse the protocol version string (e.g., "5.5")
        if let Some((major, minor)) = protocol_version.split_once('.') {
            if let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) {
                // Check if protocol version is >= 5.5
                return major > 5 || (major == 5 && minor >= 5);
            }
        }
        // If we can't parse the version, assume it doesn't support handshaking
        false
    }
}
