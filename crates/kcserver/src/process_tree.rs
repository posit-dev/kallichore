//
// process_tree.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Enumeration of the processes belonging to a kernel session.
//!
//! Each implementation walks only the session's own process tree; none of them
//! enumerate the system process table.
//!
//! - Linux: reads `/proc/[pid]/task/[tid]/children`, which the kernel maintains
//! - macOS: `proc_listchildpids()`
//! - Windows: the membership list of the session's job object

use std::collections::HashSet;

/// Get the root PID and all of its descendants.
///
/// `session_id` identifies the kernel session the root process belongs to; it
/// is how Windows finds the session's job object.
#[allow(unused_variables)]
pub fn get_process_tree(session_id: &str, root_pid: u32) -> HashSet<u32> {
    // On Windows, job object membership is inherited by child processes, so
    // the job already knows the whole tree and there is nothing to walk.
    #[cfg(target_os = "windows")]
    {
        let mut tree = crate::kernel_session::job_object::session_process_ids(session_id)
            .unwrap_or_default();
        tree.insert(root_pid);
        tree
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut visited = HashSet::new();
        let mut to_visit = vec![root_pid];

        while let Some(pid) = to_visit.pop() {
            if !visited.insert(pid) {
                continue;
            }
            to_visit.extend(child_pids(pid).into_iter().filter(|c| !visited.contains(c)));
        }

        visited
    }
}

/// Read the direct children of a process from `/proc/[pid]/task/[tid]/children`.
///
/// The kernel tracks children per thread, so we union the lists across the
/// process's threads. This needs `CONFIG_PROC_CHILDREN`, on by default since
/// Linux 3.5; if the file is missing we report no children.
#[cfg(target_os = "linux")]
fn child_pids(pid: u32) -> Vec<u32> {
    use std::fs;

    let task_dir = format!("/proc/{}/task", pid);
    let Ok(entries) = fs::read_dir(&task_dir) else {
        return Vec::new();
    };

    let mut children = Vec::new();
    for entry in entries.flatten() {
        let Ok(tid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(content) = fs::read_to_string(format!("{}/{}/children", task_dir, tid)) else {
            continue;
        };
        children.extend(
            content
                .split_whitespace()
                .filter_map(|pid| pid.parse::<u32>().ok()),
        );
    }

    children
}

/// Read the direct children of a process with `proc_listchildpids()`.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn child_pids(pid: u32) -> Vec<u32> {
    /// Ceiling on how many children we will ask for, so a pathological process
    /// tree can't make us allocate without bound.
    const MAX_CHILDREN: usize = 4096;

    // proc_listchildpids() returns the number of PIDs it wrote, and silently
    // truncates if the buffer is too small, so we grow until a call comes back
    // short. Asking it to size the buffer is no help: given a null buffer it
    // reports the number of processes on the whole system.
    let mut capacity = 16usize;

    loop {
        let mut buffer: Vec<libc::c_int> = vec![0; capacity];
        // SAFETY: we tell the call the size of a buffer we just allocated, and
        // it writes at most that many entries.
        let written = unsafe {
            libc::proc_listchildpids(
                pid as libc::pid_t,
                buffer.as_mut_ptr() as *mut libc::c_void,
                (capacity * size_of::<libc::c_int>()) as libc::c_int,
            )
        };

        if written <= 0 {
            return Vec::new();
        }

        let written = written as usize;
        if written >= capacity && capacity < MAX_CHILDREN {
            // The list may have been truncated; retry with more room.
            capacity = (capacity * 2).min(MAX_CHILDREN);
            continue;
        }

        buffer.truncate(written);
        return buffer
            .into_iter()
            .filter(|&pid| pid > 0)
            .map(|pid| pid as u32)
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_contains_root() {
        let pid = std::process::id();
        assert!(get_process_tree("test", pid).contains(&pid));
    }

    /// Windows discovers the tree through a job object that only kernel
    /// sessions get, so this covers the platforms that walk the tree live.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn child_process_is_in_tree() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("failed to spawn test child");

        let tree = get_process_tree("test", std::process::id());
        let child_pid = child.id();

        let _ = child.kill();
        let _ = child.wait();

        assert!(
            tree.contains(&child_pid),
            "child {} missing from tree {:?}",
            child_pid,
            tree
        );
    }
}
