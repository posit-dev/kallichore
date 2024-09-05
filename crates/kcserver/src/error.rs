//
// error.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::fmt;

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
    SessionNotRunning(String),
    ProcessNotFound(String),
    SessionStartFailed(anyhow::Error),
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
            KSError::SessionNotRunning(session_id) => {
                write!(f, "Session {} is not running", session_id)
            }
            KSError::SessionStartFailed(err) => {
                write!(f, "Failed to start session: {}", err)
            }
            KSError::ProcessNotFound(session_id) => {
                write!(f, "Can't find a process for session {}", session_id)
            }
            KSError::SessionNotFound(session_id) => {
                write!(f, "Session {} not found", session_id)
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
