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
    pub status: models::Status,
}

impl KernelSession {
    /// Create a new kernel session.
    pub fn new(session: models::Session) -> Self {
        // Start the session in a new thread
        let argv = session.argv.clone();

        let mut child = tokio::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(session.working_directory)
            .spawn()
            .expect("Failed to start child process");

        // Get the process ID of the child process
        let pid = child.id();

        let mut kernel_session = KernelSession {
            session_id: session.session_id.clone(),
            argv: session.argv,
            process_id: pid,
            status: models::Status::Idle,
        };

        tokio::spawn(async move {
            let status = child.wait().await.expect("Failed to wait on child process");
            // update the status of the session
            kernel_session.status = models::Status::Exited;
            log::info!(
                "Child process for session {} exited with status: {}",
                session.session_id,
                status
            );
        });

        kernel_session
    }
}
