//
// working-dir.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

// This file contains two platform-specific implementations of the function
// `get_process_cwd`, which returns the current working directory of a process
// given its PID.

use std::{
    env,
    path::{Path, PathBuf},
};

/// Expand a path
pub fn expand_path<P: AsRef<Path>>(path: P) -> Result<PathBuf, anyhow::Error> {
    let path = path.as_ref();
    let path_str = path.to_string_lossy();

    // Handle home directory expansion
    if path_str.starts_with('~') {
        let home_dir = if cfg!(windows) {
            // On Windows, try USERPROFILE first, then HOME
            env::var("USERPROFILE").or_else(|_| env::var("HOME"))
        } else {
            // On Unix-like systems, try HOME first, then fallback to pwd entry
            env::var("HOME").or_else(|_| {
                #[cfg(unix)]
                {
                    use std::ffi::CStr;
                    use std::os::raw::c_char;
                    extern "C" {
                        fn getpwuid(uid: u32) -> *mut libc::passwd;
                        fn getuid() -> u32;
                    }

                    #[allow(unsafe_code)]
                    unsafe {
                        let passwd = getpwuid(getuid());
                        if !passwd.is_null() {
                            let dir = (*passwd).pw_dir as *const c_char;
                            let home = CStr::from_ptr(dir).to_string_lossy().into_owned();
                            return Ok(home);
                        }
                    }
                }
                Err(env::VarError::NotPresent)
            })
        }
        .map_err(|_| anyhow::anyhow!("Can't read HOME from environment to expand {:?}", path))?;

        // Replace ~ with home directory and handle ~/rest/of/path
        let remainder = &path_str[1..];
        let path = if remainder.is_empty() {
            PathBuf::from(home_dir)
        } else if remainder.starts_with('/') || remainder.starts_with('\\') {
            PathBuf::from(home_dir).join(&remainder[1..])
        } else {
            PathBuf::from(home_dir).join(remainder)
        };

        if cfg!(unix) {
            // On Unix-like systems, we can use canonicalize to resolve symlinks
            Ok(path.canonicalize()?)
        } else {
            // No need on Windows (plus `canonicalize` adds some extended path garbage)
            Ok(path.to_path_buf())
        }
    } else {
        if cfg!(unix) {
            Ok(path.canonicalize()?)
        } else {
            Ok(path.to_path_buf())
        }
    }
}

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
