//
// error.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use std::fmt;

use kallichore_api::models;
use log::error;

pub enum KSError {
    SessionExists(String),
}

impl fmt::Display for KSError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error KS-{}: ", self.discriminant())?;
        match self {
            KSError::SessionExists(session_id) => {
                write!(f, "Session {} already exists", session_id)
            }
        }
    }
}

impl KSError {
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
