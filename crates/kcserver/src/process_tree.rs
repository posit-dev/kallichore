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

use std::collections::HashSet;

/// How often to refresh the cache (in ticks)
/// This is used on platforms that cache process trees (Linux, Windows)
/// to limit the frequency of full process table scans.
#[cfg(any(target_os = "linux", target_os = "windows"))]
const CACHE_REFRESH_INTERVAL: u32 = 5;

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

/// Notify the process tree cache that a tick has occurred.
/// This triggers periodic cache refresh on platforms that use caching.
#[allow(unused_variables)]
pub fn tick_process_cache(root_pid: u32) {
    #[cfg(target_os = "linux")]
    {
        linux::tick_process_cache(root_pid);
    }

    #[cfg(target_os = "windows")]
    {
        windows::tick_process_cache(root_pid);
    }
}

/// Clear the process tree cache for a given root PID.
/// Called when a kernel session is terminated.
#[allow(unused_variables)]
pub fn clear_process_cache(root_pid: u32) {
    #[cfg(target_os = "linux")]
    {
        linux::clear_process_cache(root_pid);
    }

    #[cfg(target_os = "windows")]
    {
        windows::clear_process_cache(root_pid);
    }
}

// =============================================================================
// macOS implementation using proc_listchildpids()
// =============================================================================

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
    use std::collections::HashSet;

    // FFI bindings for libproc
    #[link(name = "proc", kind = "dylib")]
    extern "C" {
        fn proc_listchildpids(
            ppid: libc::c_int,
            buffer: *mut libc::c_int,
            buffersize: libc::c_int,
        ) -> libc::c_int;
    }

    /// Get child PIDs of a process using proc_listchildpids()
    fn get_child_pids(pid: u32) -> Vec<u32> {
        // First call with null buffer to get the required buffer size in bytes
        // SAFETY: proc_listchildpids is a well-documented macOS API that safely handles
        // null buffer pointers by returning the required buffer size in bytes.
        let bytes_needed =
            unsafe { proc_listchildpids(pid as libc::c_int, std::ptr::null_mut(), 0) };

        if bytes_needed <= 0 {
            return Vec::new();
        }

        // Allocate buffer for PIDs (convert bytes to element count)
        let count = bytes_needed as usize / size_of::<libc::c_int>();
        let mut buffer: Vec<libc::c_int> = vec![0; count];

        // SAFETY: We've allocated a buffer of sufficient size (as returned by the first call).
        // proc_listchildpids writes at most buffersize bytes to the buffer.
        let result =
            unsafe { proc_listchildpids(pid as libc::c_int, buffer.as_mut_ptr(), bytes_needed) };

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
// Linux implementation using PGID filtering with caching
// =============================================================================

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::sync::Mutex;

    use once_cell::sync::Lazy;

    use super::CACHE_REFRESH_INTERVAL;
    use crate::proc_stat;

    /// Global cache shared across all kernels
    struct GlobalCache {
        /// Parent map from the last /proc scan (pid -> ppid)
        parent_map: HashMap<u32, u32>,
        /// PGID map from the last /proc scan (pid -> pgid)
        pgid_map: HashMap<u32, u32>,
        /// Per-kernel cached process trees
        kernel_caches: HashMap<u32, HashSet<u32>>,
        /// Global tick counter (all kernels share the same clock)
        tick_count: u32,
    }

    static GLOBAL_CACHE: Lazy<Mutex<GlobalCache>> = Lazy::new(|| {
        Mutex::new(GlobalCache {
            parent_map: HashMap::new(),
            pgid_map: HashMap::new(),
            kernel_caches: HashMap::new(),
            tick_count: 0,
        })
    });

    /// Scan /proc once and build parent_map and pgid_map for all processes
    fn scan_proc() -> (HashMap<u32, u32>, HashMap<u32, u32>) {
        let mut parent_map = HashMap::new();
        let mut pgid_map = HashMap::new();

        let proc_dir = match fs::read_dir("/proc") {
            Ok(dir) => dir,
            Err(_) => return (parent_map, pgid_map),
        };

        for entry in proc_dir.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Only look at numeric directories (PIDs)
            if let Ok(pid) = name_str.parse::<u32>() {
                if let Some(stat) = proc_stat::parse_proc_stat(pid) {
                    parent_map.insert(pid, stat.ppid);
                    pgid_map.insert(pid, stat.pgid);
                }
            }
        }

        (parent_map, pgid_map)
    }

    /// Build a process tree for a root PID using the cached parent/pgid maps
    fn build_tree_from_cache(
        root_pid: u32,
        parent_map: &HashMap<u32, u32>,
        pgid_map: &HashMap<u32, u32>,
    ) -> HashSet<u32> {
        let mut tree = HashSet::new();
        tree.insert(root_pid);

        // Get the PGID of the root process
        let root_pgid = match pgid_map.get(&root_pid) {
            Some(&pgid) => pgid,
            None => return tree, // Process doesn't exist
        };

        // Collect PIDs with the same PGID
        let same_pgid_pids: Vec<u32> = pgid_map
            .iter()
            .filter(|(_, &pgid)| pgid == root_pgid)
            .map(|(&pid, _)| pid)
            .collect();

        // Check if each process is a descendant of root_pid
        for pid in same_pgid_pids {
            if is_descendant_of(pid, root_pid, parent_map) {
                tree.insert(pid);
            }
        }

        tree
    }

    /// Check if `pid` is a descendant of `ancestor` using the parent map
    fn is_descendant_of(pid: u32, ancestor: u32, parent_map: &HashMap<u32, u32>) -> bool {
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

    pub fn get_process_tree(root_pid: u32) -> HashSet<u32> {
        let mut cache = GLOBAL_CACHE.lock().unwrap();

        // Check if we have a cached result
        if let Some(pids) = cache.kernel_caches.get(&root_pid) {
            return pids.clone();
        }

        // No cache entry - need to build one
        // If we have no /proc data yet, do an initial scan
        if cache.parent_map.is_empty() {
            let (parent_map, pgid_map) = scan_proc();
            cache.parent_map = parent_map;
            cache.pgid_map = pgid_map;
        }

        let pids = build_tree_from_cache(root_pid, &cache.parent_map, &cache.pgid_map);
        cache.kernel_caches.insert(root_pid, pids.clone());
        pids
    }

    pub fn tick_process_cache(_root_pid: u32) {
        let mut cache = GLOBAL_CACHE.lock().unwrap();

        cache.tick_count += 1;

        if cache.tick_count >= CACHE_REFRESH_INTERVAL {
            // Do ONE /proc scan for all kernels
            let (parent_map, pgid_map) = scan_proc();
            cache.parent_map = parent_map;
            cache.pgid_map = pgid_map;

            // Rebuild all kernel caches
            let root_pids: Vec<u32> = cache.kernel_caches.keys().cloned().collect();
            for root in root_pids {
                let pids = build_tree_from_cache(root, &cache.parent_map, &cache.pgid_map);
                cache.kernel_caches.insert(root, pids);
            }

            cache.tick_count = 0;
        }
    }

    pub fn clear_process_cache(root_pid: u32) {
        let mut cache = GLOBAL_CACHE.lock().unwrap();
        cache.kernel_caches.remove(&root_pid);
    }
}

// =============================================================================
// Windows implementation with cached process tree
// =============================================================================

#[cfg(target_os = "windows")]
mod windows {
    use std::collections::{HashMap, HashSet};
    use std::mem::size_of;
    use std::sync::Mutex;

    use once_cell::sync::Lazy;
    use windows::Win32::Foundation::CloseHandle;

    use super::CACHE_REFRESH_INTERVAL;
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

    /// Build the complete process tree by scanning all processes
    #[allow(unsafe_code)]
    fn scan_process_tree(root_pid: u32) -> HashSet<u32> {
        let mut tree = HashSet::new();
        tree.insert(root_pid);

        // Create a snapshot of all processes
        // SAFETY: CreateToolhelp32Snapshot is a well-documented Windows API.
        // TH32CS_SNAPPROCESS with 0 requests a snapshot of all processes.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        let snapshot = match snapshot {
            Ok(handle) => handle,
            Err(_) => return tree,
        };

        // Build a parent-child map
        let mut parent_map: HashMap<u32, u32> = HashMap::new();

        let mut entry = PROCESSENTRY32 {
            dwSize: size_of::<PROCESSENTRY32>() as u32,
            ..Default::default()
        };

        // SAFETY: We have a valid snapshot handle and properly initialized PROCESSENTRY32
        // with dwSize set. Process32First/Next read process info into the entry struct.
        // CloseHandle releases the snapshot handle when done.
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
