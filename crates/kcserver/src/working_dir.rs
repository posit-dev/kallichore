//
// working-dir.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

// This file contains three platform-specific implementations of the function
// `get_process_cwd`, which returns the current working directory of a process
// given its PID.

use std::path::PathBuf;

/// Given a process ID, returns its current working directory.
///
/// The Linux implementation uses the `/proc` filesystem.
#[cfg(target_os = "linux")]
pub fn get_process_cwd(pid: u32) -> Result<PathBuf, anyhow::Error> {
    let cwd_link = format!("/proc/{}/cwd", pid);
    let path = std::fs::read_link(cwd_link)?;
    Ok(path)
}

/// Given a process ID, returns its current working directory.
///
/// The macOS implementation uses the `proc_pidinfo` system call.
#[cfg(target_os = "macos")]
pub fn get_process_cwd(pid: u32) -> Result<PathBuf, anyhow::Error> {
    use errno::errno;
    use libc::{proc_pidinfo, PROC_PIDVNODEPATHINFO};

    const PROC_PIDVNODEPATHINFO_SIZE: usize = size_of::<libc::proc_vnodepathinfo>();

    #[allow(unsafe_code)]
    unsafe {
        let mut vpi: libc::proc_vnodepathinfo = std::mem::zeroed();

        #[allow(trivial_casts)]
        let ret = proc_pidinfo(
            pid as i32,
            PROC_PIDVNODEPATHINFO,
            0,
            &mut vpi as *mut _ as *mut libc::c_void,
            PROC_PIDVNODEPATHINFO_SIZE as i32,
        );

        if ret <= 0 {
            return Err(anyhow::anyhow!("proc_pidinfo failed: {}", errno()));
        }

        let cwd = std::ffi::CStr::from_ptr(vpi.pvi_cdir.vip_path.as_ptr() as *const i8)
            .to_string_lossy()
            .into_owned();

        Ok(PathBuf::from(cwd))
    }
}
