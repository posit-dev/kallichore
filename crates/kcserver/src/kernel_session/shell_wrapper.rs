//
// shell_wrapper.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Shell wrapper functionality for kernel startup.

use kallichore_api::models::{self, StartupError};
use std::collections::HashMap;
#[cfg(not(target_os = "windows"))]
use std::fs;

#[cfg(not(target_os = "windows"))]
use super::utils::escape_for_shell;
#[cfg(not(target_os = "windows"))]
use crate::error::KSError;

/// Information about the shell command that was built.
pub struct ShellCommandInfo {
    /// The tokio Command to execute
    pub command: tokio::process::Command,

    /// The shell that will be used (e.g., "/bin/bash")
    pub shell_used: Option<String>,

    /// The startup command or script path
    pub startup_arg: Option<String>,
}

/// Builds shell commands for starting kernels with various startup environments.
pub struct ShellCommandBuilder {
    /// Session ID for logging
    session_id: String,

    /// Startup environment mode
    startup_env: models::StartupEnvironment,

    /// Optional argument for Command or Script modes
    #[cfg(not(target_os = "windows"))]
    startup_arg: Option<String>,

    /// Working directory for resolving relative paths
    #[cfg(not(target_os = "windows"))]
    working_directory: String,
}

impl ShellCommandBuilder {
    /// Create a new shell command builder.
    pub fn new(
        session_id: String,
        startup_env: models::StartupEnvironment,
        #[cfg(not(target_os = "windows"))] startup_arg: Option<String>,
        #[cfg(not(target_os = "windows"))] working_directory: String,
    ) -> Self {
        Self {
            session_id,
            startup_env,
            #[cfg(not(target_os = "windows"))]
            startup_arg,
            #[cfg(not(target_os = "windows"))]
            working_directory,
        }
    }

    /// Build a command to start the kernel, optionally wrapped in a shell.
    ///
    /// Returns `None` if no shell wrapper is needed (StartupEnvironment::None),
    /// or `Some(ShellCommandInfo)` with the shell wrapper configured and metadata.
    pub fn build_command(
        &self,
        argv: &[String],
        resolved_env: &HashMap<String, String>,
    ) -> Result<Option<ShellCommandInfo>, StartupError> {
        match self.startup_env {
            models::StartupEnvironment::None => Ok(None),

            models::StartupEnvironment::Shell
            | models::StartupEnvironment::Command
            | models::StartupEnvironment::Script => {
                self.build_login_shell_command(argv, resolved_env)
            }
        }
    }

    /// Build a command that runs the kernel in a login shell.
    #[cfg(not(target_os = "windows"))]
    fn build_login_shell_command(
        &self,
        argv: &[String],
        #[cfg_attr(not(target_os = "macos"), allow(unused_variables))] resolved_env: &HashMap<
            String,
            String,
        >,
    ) -> Result<Option<ShellCommandInfo>, StartupError> {
        // Find a suitable login shell
        let login_shell = match self.find_login_shell() {
            Some(shell) => shell,
            None => {
                log::warn!(
                    "[session {}] No valid login shell found; running kernel without a login shell",
                    self.session_id
                );
                return Ok(None);
            }
        };

        log::debug!(
            "[session {}] Running kernel in login shell: {}",
            self.session_id,
            login_shell
        );

        // Build the base kernel command with escaping
        let mut kernel_command = argv
            .iter()
            .map(|arg| escape_for_shell(arg))
            .collect::<Vec<_>>()
            .join(" ");

        // On macOS, if the DYLD_LIBRARY_PATH environment variable was
        // requested, set it explicitly; it is not inherited by default
        // in login shells due to SIP.
        #[cfg(target_os = "macos")]
        if let Some(dyld_path) = resolved_env.get("DYLD_LIBRARY_PATH") {
            log::debug!(
                "[session {}] Explicitly forwarding DYLD_LIBRARY_PATH: {}",
                self.session_id,
                dyld_path
            );
            kernel_command = format!(
                "DYLD_LIBRARY_PATH={} {}",
                escape_for_shell(dyld_path),
                kernel_command
            );
        }

        // Add startup command or script if specified
        kernel_command = match self.startup_env {
            models::StartupEnvironment::Command => {
                self.wrap_with_startup_command(kernel_command)?
            }
            models::StartupEnvironment::Script => {
                self.wrap_with_startup_script(kernel_command, &login_shell)?
            }
            _ => kernel_command, // Shell mode - no prefix
        };

        // Determine login argument based on shell type
        let login_arg = self.get_login_arg(&login_shell);

        // Create the shell command
        let mut cmd = tokio::process::Command::new(&login_shell);
        cmd.args(&[login_arg, "-c", &kernel_command]);

        Ok(Some(ShellCommandInfo {
            command: cmd,
            shell_used: Some(login_shell),
            startup_arg: self.startup_arg.clone(),
        }))
    }

    /// On Windows, startup environment settings are ignored.
    #[cfg(target_os = "windows")]
    fn build_login_shell_command(
        &self,
        _argv: &[String],
        _resolved_env: &HashMap<String, String>,
    ) -> Result<Option<ShellCommandInfo>, StartupError> {
        log::debug!(
            "[session {}] startup_environment parameter ignored on Windows",
            self.session_id
        );
        Ok(None)
    }

    /// Find a suitable login shell.
    #[cfg(not(target_os = "windows"))]
    fn find_login_shell(&self) -> Option<String> {
        let candidates = vec![
            std::env::var("SHELL").unwrap_or_else(|_| String::from("")),
            String::from("/bin/bash"),
            String::from("/bin/sh"),
        ];

        for (i, shell_path) in candidates.iter().enumerate() {
            // Ignore if empty (happens if SHELL is not set)
            if shell_path.is_empty() {
                continue;
            }

            // Check if the shell exists
            if fs::metadata(shell_path).is_ok() {
                return Some(shell_path.clone());
            } else if i == 0 {
                // The first candidate comes from $SHELL. If it doesn't exist,
                // log a warning but continue to try the others.
                log::warn!(
                    "[session {}] Shell path specified in $SHELL '{}' does not exist",
                    self.session_id,
                    shell_path
                );
            }
        }

        None
    }

    /// Wrap the kernel command with a startup command.
    #[cfg(not(target_os = "windows"))]
    fn wrap_with_startup_command(&self, kernel_command: String) -> Result<String, StartupError> {
        if let Some(cmd) = &self.startup_arg {
            log::debug!(
                "[session {}] Executing startup command: {}",
                self.session_id,
                cmd
            );
            // Wrap command in markers to identify failure point
            Ok(format!(
                "echo 'KALLICHORE_STARTUP_BEGIN' && {{ {}; }} && echo 'KALLICHORE_STARTUP_SUCCESS' && {}",
                cmd,
                kernel_command
            ))
        } else {
            log::warn!(
                "[session {}] StartupEnvironment::Command specified but no command provided",
                self.session_id
            );
            Ok(kernel_command)
        }
    }

    /// Wrap the kernel command with a startup script.
    #[cfg(not(target_os = "windows"))]
    fn wrap_with_startup_script(
        &self,
        kernel_command: String,
        login_shell: &str,
    ) -> Result<String, StartupError> {
        let script_path_str = match &self.startup_arg {
            Some(path) => path,
            None => {
                log::warn!(
                    "[session {}] StartupEnvironment::Script specified but no script path provided",
                    self.session_id
                );
                return Ok(kernel_command);
            }
        };

        // Resolve script path
        let script_path = if std::path::Path::new(script_path_str).is_absolute() {
            std::path::PathBuf::from(script_path_str)
        } else {
            // Resolve relative to working directory
            std::path::PathBuf::from(&self.working_directory).join(script_path_str)
        };

        // Validate script exists and is a file
        match fs::metadata(&script_path) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    let err = KSError::ProcessStartFailed(anyhow::anyhow!(
                        "Startup script path '{}' exists but is not a file",
                        script_path.display()
                    ));
                    return Err(StartupError {
                        exit_code: None,
                        output: None,
                        error: err.to_json(None),
                    });
                }
            }
            Err(e) => {
                let err = KSError::ProcessStartFailed(anyhow::anyhow!(
                    "Startup script not found or cannot be read: '{}' ({})",
                    script_path.display(),
                    e
                ));
                return Err(StartupError {
                    exit_code: None,
                    output: None,
                    error: err.to_json(None),
                });
            }
        }

        log::debug!(
            "[session {}] Sourcing startup script: {}",
            self.session_id,
            script_path.display()
        );

        // Determine shell type for proper source command
        let source_cmd = match login_shell.split('/').last() {
            Some("csh") | Some("tcsh") => "source", // csh uses 'source'
            _ => ".", // sh/bash/zsh use '.' or 'source', '.' is more portable
        };

        // Wrap script in markers to identify failure point
        Ok(format!(
            "echo 'KALLICHORE_STARTUP_BEGIN' && {{ {} {}; }} && echo 'KALLICHORE_STARTUP_SUCCESS' && {}",
            source_cmd,
            escape_for_shell(&script_path.to_string_lossy()),
            kernel_command
        ))
    }

    /// Get the login argument for a specific shell.
    #[cfg(not(target_os = "windows"))]
    fn get_login_arg(&self, login_shell: &str) -> &'static str {
        match login_shell.split('/').last() {
            None => "-l", // Unknown shell, presume bash-alike
            Some(shell) => match shell {
                // csh-like shells don't support -c for login shells.
                // Instead, we emulate a login shell by asking it to load
                // the directory stack (-d)
                "csh" | "tcsh" => "-d",

                // Bash and zsh support the long-form --login option
                "bash" | "zsh" => "--login",

                // Sh and dash only support -l
                "dash" | "sh" => "-l",

                // For all other shells, presume -l
                _ => "-l",
            },
        }
    }
}
