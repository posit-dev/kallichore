//
// process_control.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Signalling kernel processes.
//!
//! These address a process by PID rather than looking it up first, so nothing
//! here enumerates the system process table.

/// Forcibly terminate a process.
///
/// Returns false if the process could not be signalled, most often because it
/// has already exited.
pub fn terminate(pid: u32) -> bool {
    #[cfg(unix)]
    {
        signal(pid, libc::SIGKILL)
    }

    #[cfg(windows)]
    {
        windows_impl::terminate(pid)
    }
}

/// Ask a process to interrupt whatever it is doing.
///
/// Unix only: on Windows an interrupt is delivered by setting a named event
/// the kernel waits on, which [`crate::kernel_session`] handles at startup
/// rather than by signalling the process.
#[cfg(unix)]
pub fn interrupt(pid: u32) -> bool {
    signal(pid, libc::SIGINT)
}

/// Report whether a process is still running.
pub fn is_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // Signal 0 runs kill()'s existence and permission checks without
        // sending anything. EPERM means the process is there but owned by
        // somebody else, which still counts as running.
        signal(pid, 0)
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[cfg(windows)]
    {
        windows_impl::is_running(pid)
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn signal(pid: u32, signal: libc::c_int) -> bool {
    // SAFETY: kill() reads only the pid and signal it is given, and reports
    // failure through its return value.
    unsafe { libc::kill(pid as libc::pid_t, signal) == 0 }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_impl {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE,
    };

    pub fn is_running(pid: u32) -> bool {
        // SAFETY: OpenProcess returns either a live handle or an error.
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return false;
        };

        // A handle can outlive the process it refers to, so ask for the exit
        // code rather than treating a successful open as proof of life.
        let mut exit_code = 0u32;
        // SAFETY: the handle is live and `exit_code` is a valid out pointer.
        let running = unsafe { GetExitCodeProcess(handle, &mut exit_code) }.is_ok()
            && exit_code == STILL_ACTIVE.0 as u32;

        // SAFETY: the handle came from OpenProcess and is not used again.
        unsafe {
            let _ = CloseHandle(handle);
        }

        running
    }

    pub fn terminate(pid: u32) -> bool {
        // SAFETY: OpenProcess returns either a live handle or an error.
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) }) else {
            return false;
        };

        // SAFETY: the handle was just opened with PROCESS_TERMINATE, and is
        // closed below whether or not the call succeeds.
        let terminated = unsafe { TerminateProcess(handle, 1) }.is_ok();
        unsafe {
            let _ = CloseHandle(handle);
        }

        terminated
    }
}

// These drive real processes through `sleep`, which Windows runners do not
// have.
#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn terminate_kills_a_live_process() {
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("failed to spawn test child");

        assert!(terminate(child.id()));
        assert!(child.wait().is_ok());
    }

    #[test]
    fn is_running_tracks_a_process_lifetime() {
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("failed to spawn test child");
        let pid = child.id();

        assert!(is_running(pid));

        let _ = child.kill();
        let _ = child.wait();

        assert!(!is_running(pid));
    }

    #[test]
    fn terminate_reports_failure_for_a_dead_process() {
        let mut child = std::process::Command::new("sleep")
            .arg("0")
            .spawn()
            .expect("failed to spawn test child");
        let pid = child.id();
        let _ = child.wait();

        // The child has been reaped, so its PID no longer names a process.
        assert!(!terminate(pid));
    }
}
