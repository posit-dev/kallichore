//! Shared types and utilities for Kallichore client and server.

/// Jupyter message types
pub mod jupyter_message;

/// Kernel message types
pub mod kernel_message;

/// WebSocket message types
pub mod websocket_message;

/// Kernel info messages
pub mod kernel_info;

/// Jupyter Handshaking Protocol (JEP 66)
pub mod handshake_protocol;

/// Port picker for finding free TCP ports
pub mod port_picker;
