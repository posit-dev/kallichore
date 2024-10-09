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
pub enum KSError {
    SessionExists(String),
    SessionNotFound(String),
    SessionConnected(String),
    SessionRunning(String),
    SessionNotRunning(String),
    ProcessNotFound(u32, String),
    ProcessStartFailed(anyhow::Error),
    SessionConnectionFailed(anyhow::Error),
    SessionConnectionTimeout(u32),
    ProcessAbnormalExit(ExitStatus, i32, String),
    SessionCreateFailed(String, anyhow::Error),
    SessionInterruptFailed(String, anyhow::Error),
    ZmqProxyError(anyhow::Error),
    NoProcess(String),
    RestartFailed(anyhow::Error),
}

impl fmt::Display for KSError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error KS-{}: ", self.discriminant())?;
        match self {
            KSError::SessionExists(session_id) => {
                write!(f, "Session {} already exists", session_id)
            }
            KSError::SessionConnected(session_id) => {
                write!(f, "Session {} is connected to another client", session_id)
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
            KSError::ProcessAbnormalExit(exit_status, exit_code, output) => {
                write!(
                    f,
                    "Process exited abnormally ({}, code {}).\n{}",
                    exit_status, exit_code, output
                )
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
                    "Timed out waiting to session's ZeroMQ sockets after {} seconds",
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
        error!("{}", self.to_string());
    }
}
