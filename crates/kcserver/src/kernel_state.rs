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

    /// Whether the kernel is connected to a client.
    pub connected: bool,

    /// The current working directory of the kernel.
    pub working_directory: String,
    // TODO: This is where other kernel state data should go -- e.g. current
    // working directory, when the current computation was started, etc.
}

impl KernelState {
    /// Create a new kernel state.
    pub fn new(working_directory: String) -> Self {
        KernelState {
            status: models::Status::Idle,
            working_directory,
            connected: false,
        }
    }
}
