//
// startup_status.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use crate::error::KSError;

/// The status of the kernel startup.
#[derive(Debug)]
pub enum StartupStatus {
    /// The kernel exited abnormally before it finished starting up or
    /// connecting. The first value is the exit code, the second is the output,
    /// and the third is the underlying error.
    AbnormalExit(i32, String, KSError),

    /// The kernel started up successfully but the attempt to connect to its 0MQ
    /// sockets failed. The first value is the output, the second is the
    /// underlying error.
    ConnectionFailed(String, KSError),

    /// The kernel started up and connected to its 0MQ sockets successfully. The payload is the kernel information.
    Connected(serde_json::Value),
}
