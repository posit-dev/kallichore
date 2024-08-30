//
// kernel_state.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use kallichore_api::models;

#[derive(Debug, Clone)]
/// The mutable state of the kernel.
pub struct KernelState {
    /// The kernel's current status.
    pub status: models::Status,
    // TODO: This is where other kernel state data should go -- e.g. current
    // working directory, when the current computation was started, etc.
}

impl KernelState {
    /// Create a new kernel state.
    pub fn new() -> Self {
        KernelState {
            status: models::Status::Idle,
        }
    }
}
