//
// kernel_spec.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// From the Jupyter documentation for [Kernel Specs](https://jupyter-client.readthedocs.io/en/stable/kernels.html#kernel-specs).
#[derive(Serialize, Deserialize)]
pub struct KernelSpec {
    /// List of command line arguments to be used to start the kernel
    pub argv: Vec<String>,

    // The kernel name as it should be displayed in the UI
    pub display_name: String,

    // The kernel's language
    pub language: String,

    // Environment variables to set for the kernel
    pub env: serde_json::Map<String, Value>,
}
