//
// environment.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Environment variable resolution for kernel processes.

use kallichore_api::models::VarAction;
use std::collections::HashMap;

/// Resolves environment variables for a kernel process.
///
/// Takes the current process environment and applies a series of variable
/// actions (replace, append, prepend) to produce the final environment
/// that will be used when starting the kernel.
pub struct EnvironmentResolver {
    /// The base environment to start from
    initial_env: HashMap<String, String>,

    /// Actions to apply to modify the environment
    var_actions: Vec<VarAction>,

    /// Optional interrupt event handle for Windows
    #[cfg(windows)]
    interrupt_handle: Option<isize>,

    /// Optional parent process handle for Windows
    #[cfg(windows)]
    parent_handle: Option<u64>,
}

impl EnvironmentResolver {
    /// Create a new environment resolver.
    pub fn new(var_actions: Vec<VarAction>) -> Self {
        // Start with a copy of the process environment
        let initial_env = std::env::vars()
            .map(|(key, value)| {
                // Environment variables are case-insensitive on Windows
                #[cfg(target_os = "windows")]
                let key = key.to_uppercase();
                #[cfg(not(target_os = "windows"))]
                let key = key;
                (key, value)
            })
            .collect();

        Self {
            initial_env,
            var_actions,
            #[cfg(windows)]
            interrupt_handle: None,
            #[cfg(windows)]
            parent_handle: None,
        }
    }

    /// Set the interrupt event handle (Windows only).
    #[cfg(windows)]
    pub fn with_interrupt_handle(mut self, handle: Option<isize>) -> Self {
        self.interrupt_handle = handle;
        self
    }

    /// Set the parent process handle (Windows only).
    #[cfg(windows)]
    pub fn with_parent_handle(mut self, handle: Option<u64>) -> Self {
        self.parent_handle = handle;
        self
    }

    /// Resolve the environment by applying all variable actions.
    pub fn resolve(&self) -> HashMap<String, String> {
        let mut resolved_env = self.initial_env.clone();

        // On Windows, add interrupt and parent process handles
        #[cfg(windows)]
        self.add_windows_handles(&mut resolved_env);

        // Apply variable actions
        self.apply_var_actions(&mut resolved_env);

        resolved_env
    }

    /// Add Windows-specific handles to the environment.
    #[cfg(windows)]
    fn add_windows_handles(&self, env: &mut HashMap<String, String>) {
        // Add interrupt event handle if present
        if let Some(handle) = self.interrupt_handle {
            log::trace!("Adding interrupt event handle to environment: {}", handle);
            env.insert("JPY_INTERRUPT_EVENT".to_string(), format!("{}", handle));
            // Also set the legacy environment variable for backward compatibility
            env.insert("IPY_INTERRUPT_EVENT".to_string(), format!("{}", handle));
        }

        // Add parent process handle if present
        if let Some(handle) = self.parent_handle {
            log::trace!("Adding parent process handle to environment: {}", handle);
            env.insert("JPY_PARENT_PID".to_string(), format!("{}", handle));
        }
    }

    /// Apply variable actions to the environment.
    fn apply_var_actions(&self, env: &mut HashMap<String, String>) {
        use kallichore_api::models::VarActionType;

        for action in &self.var_actions {
            // Normalize key case on Windows
            #[cfg(target_os = "windows")]
            let env_key = action.name.to_uppercase();
            #[cfg(not(target_os = "windows"))]
            let env_key = action.name.clone();

            match action.action {
                VarActionType::Replace => {
                    env.insert(env_key, action.value.clone());
                }
                VarActionType::Append => {
                    let mut value = env.get(&env_key).unwrap_or(&String::new()).clone();
                    value.push_str(&action.value);
                    env.insert(env_key, value);
                }
                VarActionType::Prepend => {
                    let mut value = env.get(&env_key).unwrap_or(&String::new()).clone();
                    value.insert_str(0, &action.value);
                    env.insert(env_key, value);
                }
            }
        }
    }
}

/// Get the parent process handle on Windows for process monitoring.
#[cfg(windows)]
pub fn get_parent_process_handle() -> Option<u64> {
    use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows::Win32::System::Threading::GetCurrentProcess;

    #[allow(unsafe_code)]
    unsafe {
        let current_process = GetCurrentProcess();
        let mut target_handle = HANDLE::default();
        match DuplicateHandle(
            current_process,                     // Source process handle
            current_process,                     // Source handle to duplicate (current process)
            current_process,                     // Target process handle
            &mut target_handle,                  // Destination handle (out)
            0,                                   // Desired access
            windows::Win32::Foundation::BOOL(1), // Inheritable
            DUPLICATE_SAME_ACCESS,
        ) {
            Ok(_) => {
                let handle = target_handle.0 as u64;
                Some(handle)
            }
            Err(e) => {
                log::error!("Failed to duplicate parent process handle: {}", e);
                None
            }
        }
    }
}

/// Create an interrupt event on Windows.
#[cfg(windows)]
pub fn create_interrupt_event() -> Result<isize, anyhow::Error> {
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::System::Threading::CreateEventA;

    #[allow(unsafe_code)]
    unsafe {
        // Create a security attributes struct that permits inheritance of
        // the handle by new processes.
        let sa = SECURITY_ATTRIBUTES {
            #[allow(unused_qualifications)]
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: windows::Win32::Foundation::BOOL(1),
        };
        let event = CreateEventA(
            Some(&sa),
            windows::Win32::Foundation::BOOL(0), // bManualReset: false (auto-reset event)
            windows::Win32::Foundation::BOOL(0), // bInitialState: false (non-signaled)
            windows::core::PCSTR::null(),        // lpName: unnamed event
        );
        match event {
            Ok(handle) => {
                let handle_value = handle.0 as isize;
                log::debug!("Created interrupt event with handle: {}", handle_value);
                Ok(handle_value)
            }
            Err(e) => Err(anyhow::anyhow!("Failed to create interrupt event: {}", e)),
        }
    }
}
