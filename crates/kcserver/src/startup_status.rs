//
// startup_status.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use crate::error::KSError;

/// The status of the kernel startup.
pub enum StartupStatus {
    AbnormalExit(KSError),
    ConnectionFailed(KSError),
    Connected,
}
