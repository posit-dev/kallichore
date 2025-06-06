//
// error.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::{fmt, process::ExitStatus};

use kallichore_api::models;
use log::error;

/// Errors that can occur in the Kallichore server.
///
/// Important: do not remove or reorder variants from this enum, as the discriminant
/// of each variant is used as an error code and we'd like to keep the error codes
/// stable across versions.
#[derive(Debug)]
pub enum KSError {
    SessionExists(String),
    SessionNotFound(String),
    SessionRunning(String),
    SessionNotRunning(String),
    ProcessNotFound(u32, String),
    ProcessStartFailed(anyhow::Error),
    SessionConnectionFailed(anyhow::Error),
    SessionConnectionTimeout(u32),
    ProcessAbnormalExit(ExitStatus),
    SessionCreateFailed(String, anyhow::Error),
    SessionInterruptFailed(String, anyhow::Error),
    ZmqProxyError(anyhow::Error),
    NoProcess(String),
    RestartFailed(anyhow::Error),
    ExitedBeforeConnection,
    NoKernelInfo(anyhow::Error),
    StartFailed(anyhow::Error),
    HandshakeFailed(String, anyhow::Error),
    NoConnectionInfo(String, anyhow::Error),
    KernelPathNotFound(String),
    NoKernelPath(String),
}

impl fmt::Display for KSError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            KSError::SessionExists(session_id) => {
                write!(f, "Session {} already exists", session_id)
            }
            KSError::SessionRunning(session_id) => {
                write!(f, "Session {} is running", session_id)
            }
            KSError::SessionNotRunning(session_id) => {
                write!(f, "Session {} is not running", session_id)
            }
            KSError::ProcessStartFailed(err) => {
                write!(f, "Failed to start process for session: {}", err)
            }
            KSError::ProcessAbnormalExit(status) => {
                write!(f, "The process exited abnormally ({})", status)
            }
            KSError::ProcessNotFound(pid, session_id) => {
                write!(f, "Can't find process {} for session {}", pid, session_id)
            }
            KSError::SessionNotFound(session_id) => {
                write!(f, "Session {} not found", session_id)
            }
            KSError::SessionConnectionFailed(err) => {
                write!(f, "Failed to connect to session's ZeroMQ sockets: {}", err)
            }
            KSError::SessionConnectionTimeout(seconds) => {
                write!(
                    f,
                    "Timed out waiting to connect to session's ZeroMQ sockets after {} seconds",
                    seconds
                )
            }
            KSError::SessionCreateFailed(session_id, err) => {
                write!(f, "Failed to create session {}: {}", session_id, err)
            }
            KSError::SessionInterruptFailed(session_id, err) => {
                write!(f, "Failed to interrupt session {}: {}", session_id, err)
            }
            KSError::NoProcess(session_id) => {
                write!(
                    f,
                    "There is no process associated with session {} (has it exited?)",
                    session_id
                )
            }
            KSError::ZmqProxyError(err) => {
                write!(f, "Error receiving or proxying ZeroMQ messages: {}", err)
            }
            KSError::RestartFailed(err) => {
                write!(f, "Failed to restart kernel: {}", err)
            }
            KSError::ExitedBeforeConnection => {
                write!(
                    f,
                    "The kernel exited before a connection could be established"
                )
            }
            KSError::NoKernelInfo(e) => {
                write!(
                    f,
                    "Failed to get kernel info from the kernel process: {}",
                    e
                )
            }
            KSError::StartFailed(e) => {
                write!(f, "Failed to start kernel: {}", e)
            }
            KSError::HandshakeFailed(session_id, e) => {
                write!(
                    f,
                    "JEP 66 handshake failed for session {}: {}",
                    session_id, e
                )
            }
            KSError::NoConnectionInfo(session_id, e) => {
                write!(
                    f,
                    "Connection information is not available for session {}: {}",
                    session_id, e
                )
            }
            KSError::KernelPathNotFound(kernel_path) => {
                write!(f, "Kernel path not found: {}", kernel_path)
            }
            KSError::NoKernelPath(session_id) => {
                write!(
                    f,
                    "Session {} is missing a kernel path; the 'argv' field in the session info must name a path to a kernel executable",
                    session_id
                )
            }
        }
    }
}

impl KSError {
    /// This method is used to match each error variant to a unique error code.
    /// The error codes are just the discriminants of the error variants, i.e.
    /// the index of the variant in the enum definition.
    #[allow(unsafe_code, trivial_casts)]
    fn discriminant(&self) -> u8 {
        unsafe { *(self as *const Self as *const u8) }
    }

    pub fn to_json(&self, details: Option<String>) -> models::Error {
        models::Error {
            code: format!("KS-{}", self.discriminant()),
            message: self.to_string(),
            details,
        }
    }

    pub fn log(&self) {
        error!("KS-{}: {}", self.discriminant(), self.to_string());
    }
}
