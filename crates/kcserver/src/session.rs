//
// session.rs
//
// Copyright (C) 2024 Posit Software, PBC. All rights reserved.
//
//

//! Wraps Jupyter kernel sessions.

use kallichore_api::models;

pub struct KernelSession {
    pub session_id: String,
    pub argv: Vec<String>,
    pub process_id: Option<u32>,
}

impl KernelSession {
    /// Create a new kernel session.
    pub fn new(session: models::Session) -> Self {
        // Start the session in a new thread
        let argv = session.argv.clone();

        let child = tokio::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(session.working_directory)
            .spawn()
            .expect("Failed to start child process");

        // Get the process ID of the child process
        let pid = child.id();
        // let _ = child.wait().await;

        KernelSession {
            session_id: session.session_id,
            argv: session.argv,
            process_id: pid,
        }
    }
}
