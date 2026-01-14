//
// process_tree.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! OS-specific efficient process tree enumeration.
//!
//! This module provides efficient ways to enumerate child processes of a given PID
//! without scanning the entire process table on the system.

// Allow unsafe code for FFI calls on macOS
#![allow(unsafe_code)]

use std::collections::HashSet;

/// Get all descendant PIDs of the given root PID.
///
/// This function returns a set containing the root PID and all its descendants.
/// The implementation is OS-specific for efficiency:
/// - macOS: Uses `proc_listchildpids()` to directly query child processes
/// - Linux: Enumerates only processes with the same PGID as the root
/// - Windows: Uses cached process tree with periodic full scans
pub fn get_process_tree(root_pid: u32) -> HashSet<u32> {
    #[cfg(target_os = "macos")]
    {
        macos::get_process_tree(root_pid)
    }

    #[cfg(target_os = "linux")]
    {
        linux::get_process_tree(root_pid)
    }

    #[cfg(target_os = "windows")]
    {
        windows::get_process_tree(root_pid)
    }
}

/// Notify the process tree cache that a tick has occurred (Windows only).
/// On other platforms, this is a no-op.
#[allow(unused_variables)]
pub fn tick_process_cache(root_pid: u32) {
    #[cfg(target_os = "windows")]
    {
        windows::tick_process_cache(root_pid);
    }
}

/// Clear the process tree cache for a given root PID.
/// Called when a kernel session is terminated.
#[allow(unused_variables, dead_code)]
pub fn clear_process_cache(root_pid: u32) {
    #[cfg(target_os = "windows")]
    {
        windows::clear_process_cache(root_pid);
    }
}

// =============================================================================
// macOS implementation using proc_listchildpids()
// =============================================================================

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashSet;

    // FFI bindings for libproc
    #[link(name = "proc", kind = "dylib")]
    extern "C" {
        fn proc_listchildpids(ppid: libc::c_int, buffer: *mut libc::c_int, buffersize: libc::c_int)
            -> libc::c_int;
    }

    /// Get child PIDs of a process using proc_listchildpids()
    fn get_child_pids(pid: u32) -> Vec<u32> {
        // First call with null buffer to get the count
        let count =
            unsafe { proc_listchildpids(pid as libc::c_int, std::ptr::null_mut(), 0) };

        if count <= 0 {
            return Vec::new();
        }

        // Allocate buffer for PIDs
        let buffer_size = count as usize;
        let mut buffer: Vec<libc::c_int> = vec![0; buffer_size];

        let result = unsafe {
            proc_listchildpids(
                pid as libc::c_int,
                buffer.as_mut_ptr(),
                (buffer_size * size_of::<libc::c_int>()) as libc::c_int,
            )
        };

        if result <= 0 {
            return Vec::new();
        }

        // Convert to u32 and filter out any zeros
        let num_pids = result as usize / size_of::<libc::c_int>();
        buffer
            .into_iter()
            .take(num_pids)
            .filter(|&pid| pid > 0)
            .map(|pid| pid as u32)
            .collect()
    }

    pub fn get_process_tree(root_pid: u32) -> HashSet<u32> {
        let mut visited = HashSet::new();
        let mut to_visit = vec![root_pid];

        while let Some(pid) = to_visit.pop() {
            if !visited.insert(pid) {
                continue;
            }

            // Get children of this process
            let children = get_child_pids(pid);
            for child in children {
                if !visited.contains(&child) {
                    to_visit.push(child);
                }
            }
        }

        visited
    }
}

// =============================================================================
// Linux implementation using PGID filtering
// =============================================================================

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashSet;
    use std::fs;

    /// Get the PGID of a process by reading /proc/[pid]/stat
    fn get_pgid(pid: u32) -> Option<u32> {
        let stat_path = format!("/proc/{}/stat", pid);
        let stat_content = fs::read_to_string(stat_path).ok()?;

        // The stat file format is: pid (comm) state ppid pgrp ...
        // We need the 5th field (pgrp/pgid), but comm can contain spaces and parens
        // So we find the last ')' and parse from there
        let last_paren = stat_content.rfind(')')?;
        let fields_after_comm = &stat_content[last_paren + 2..]; // Skip ") "
        let fields: Vec<&str> = fields_after_comm.split_whitespace().collect();

        // fields[0] = state, fields[1] = ppid, fields[2] = pgrp
        if fields.len() < 3 {
            return None;
        }

        fields[2].parse().ok()
    }

    /// Get the PPID (parent PID) of a process
    fn get_ppid(pid: u32) -> Option<u32> {
        let stat_path = format!("/proc/{}/stat", pid);
        let stat_content = fs::read_to_string(stat_path).ok()?;

        let last_paren = stat_content.rfind(')')?;
        let fields_after_comm = &stat_content[last_paren + 2..];
        let fields: Vec<&str> = fields_after_comm.split_whitespace().collect();

        // fields[0] = state, fields[1] = ppid
        if fields.len() < 2 {
            return None;
        }

        fields[1].parse().ok()
    }

    pub fn get_process_tree(root_pid: u32) -> HashSet<u32> {
        let mut tree = HashSet::new();
        tree.insert(root_pid);

        // Get the PGID of the root process
        let root_pgid = match get_pgid(root_pid) {
            Some(pgid) => pgid,
            None => return tree, // Process doesn't exist, return just the root
        };

        // Read /proc to find all processes
        let proc_dir = match fs::read_dir("/proc") {
            Ok(dir) => dir,
            Err(_) => return tree,
        };

        // Collect all PIDs that have the same PGID
        let mut same_pgid_pids = Vec::new();
        for entry in proc_dir.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Only look at numeric directories (PIDs)
            if let Ok(pid) = name_str.parse::<u32>() {
                if let Some(pgid) = get_pgid(pid) {
                    if pgid == root_pgid {
                        same_pgid_pids.push(pid);
                    }
                }
            }
        }

        // Now filter to only include descendants of root_pid
        // Build a parent map for these processes
        let mut parent_map: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for &pid in &same_pgid_pids {
            if let Some(ppid) = get_ppid(pid) {
                parent_map.insert(pid, ppid);
            }
        }

        // Check if each process is a descendant of root_pid
        for &pid in &same_pgid_pids {
            if is_descendant_of(pid, root_pid, &parent_map) {
                tree.insert(pid);
            }
        }

        tree
    }

    /// Check if `pid` is a descendant of `ancestor` using the parent map
    fn is_descendant_of(
        pid: u32,
        ancestor: u32,
        parent_map: &std::collections::HashMap<u32, u32>,
    ) -> bool {
        if pid == ancestor {
            return true;
        }

        let mut current = pid;
        let mut visited = HashSet::new();

        while let Some(&parent) = parent_map.get(&current) {
            if parent == ancestor {
                return true;
            }
            if parent == 0 || parent == 1 || !visited.insert(current) {
                // Reached init or a cycle, not a descendant
                return false;
            }
            current = parent;
        }

        false
    }
}

// =============================================================================
// Windows implementation with cached process tree
// =============================================================================

#[cfg(target_os = "windows")]
mod windows {
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    use once_cell::sync::Lazy;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS,
    };

    /// Cache entry for a process tree
    struct CacheEntry {
        pids: HashSet<u32>,
        tick_count: u32,
    }

    /// Global cache for process trees, keyed by root PID
    static PROCESS_CACHE: Lazy<Mutex<HashMap<u32, CacheEntry>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    /// How often to refresh the cache (in ticks)
    const CACHE_REFRESH_INTERVAL: u32 = 10;

    /// Build the complete process tree by scanning all processes
    fn scan_process_tree(root_pid: u32) -> HashSet<u32> {
        let mut tree = HashSet::new();
        tree.insert(root_pid);

        // Create a snapshot of all processes
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        let snapshot = match snapshot {
            Ok(handle) => handle,
            Err(_) => return tree,
        };

        // Build a parent-child map
        let mut parent_map: HashMap<u32, u32> = HashMap::new();

        let mut entry = PROCESSENTRY32 {
            dwSize: std::mem::size_of::<PROCESSENTRY32>() as u32,
            ..Default::default()
        };

        unsafe {
            if Process32First(snapshot, &mut entry).is_ok() {
                loop {
                    let pid = entry.th32ProcessID;
                    let ppid = entry.th32ParentProcessID;
                    parent_map.insert(pid, ppid);

                    if Process32Next(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
        }

        // Find all descendants using BFS
        let mut to_visit = vec![root_pid];
        while let Some(pid) = to_visit.pop() {
            // Find all children of this PID
            for (&child_pid, &parent_pid) in &parent_map {
                if parent_pid == pid && !tree.contains(&child_pid) {
                    tree.insert(child_pid);
                    to_visit.push(child_pid);
                }
            }
        }

        tree
    }

    pub fn get_process_tree(root_pid: u32) -> HashSet<u32> {
        let mut cache = PROCESS_CACHE.lock().unwrap();

        if let Some(entry) = cache.get(&root_pid) {
            // Return cached result
            return entry.pids.clone();
        }

        // No cache entry, do a full scan
        let pids = scan_process_tree(root_pid);
        cache.insert(
            root_pid,
            CacheEntry {
                pids: pids.clone(),
                tick_count: 0,
            },
        );
        pids
    }

    pub fn tick_process_cache(root_pid: u32) {
        let mut cache = PROCESS_CACHE.lock().unwrap();

        if let Some(entry) = cache.get_mut(&root_pid) {
            entry.tick_count += 1;

            if entry.tick_count >= CACHE_REFRESH_INTERVAL {
                // Time to refresh the cache
                entry.pids = scan_process_tree(root_pid);
                entry.tick_count = 0;
            }
        }
    }

    pub fn clear_process_cache(root_pid: u32) {
        let mut cache = PROCESS_CACHE.lock().unwrap();
        cache.remove(&root_pid);
    }
}
