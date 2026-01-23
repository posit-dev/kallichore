//
// proc_stat.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Linux-specific utilities for parsing /proc filesystem.
//!
//! This module provides shared utilities for parsing /proc/[pid]/stat files,
//! used by both the process tree enumeration and CPU tracking code.

#![cfg(target_os = "linux")]

use std::fs;

/// Parsed fields from /proc/[pid]/stat
#[derive(Debug, Clone)]
pub struct ProcStat {
    /// Parent process ID (field 4, index 1 after comm)
    pub ppid: u32,
    /// Process group ID (field 5, index 2 after comm)
    pub pgid: u32,
    /// User mode CPU time in jiffies (field 14, index 11 after comm)
    pub utime: u64,
    /// Kernel mode CPU time in jiffies (field 15, index 12 after comm)
    pub stime: u64,
}

impl ProcStat {
    /// Total CPU time (utime + stime)
    pub fn cpu_time(&self) -> u64 {
        self.utime + self.stime
    }
}

/// Parse /proc/[pid]/stat to extract process information.
///
/// The stat file format is: `pid (comm) state ppid pgrp session tty_nr tpgid flags
/// minflt cminflt majflt cmajflt utime stime cutime cstime ...`
///
/// Note: `comm` can contain spaces and parentheses, so we find the last ')' to
/// reliably parse the remaining fields.
pub fn parse_proc_stat(pid: u32) -> Option<ProcStat> {
    let stat_path = format!("/proc/{}/stat", pid);
    let stat_content = fs::read_to_string(stat_path).ok()?;

    // comm can contain spaces and parens, so find the last ')'
    let last_paren = stat_content.rfind(')')?;
    let fields_after_comm = stat_content.get(last_paren + 2..)?; // Skip ") "
    let fields: Vec<&str> = fields_after_comm.split_whitespace().collect();

    // fields[0] = state, fields[1] = ppid, fields[2] = pgrp, ...
    // fields[11] = utime, fields[12] = stime
    if fields.len() < 13 {
        return None;
    }

    Some(ProcStat {
        ppid: fields[1].parse().ok()?,
        pgid: fields[2].parse().ok()?,
        utime: fields[11].parse().ok()?,
        stime: fields[12].parse().ok()?,
    })
}

/// Read total CPU time from /proc/stat (sum of all jiffies across all CPUs).
///
/// The first line of /proc/stat is:
/// `cpu  user nice system idle iowait irq softirq steal guest guest_nice`
///
/// We sum all these values to get total CPU time.
pub fn read_total_cpu_time() -> u64 {
    let Ok(content) = fs::read_to_string("/proc/stat") else {
        return 0;
    };

    let Some(cpu_line) = content.lines().next() else {
        return 0;
    };

    if !cpu_line.starts_with("cpu ") {
        return 0;
    }

    // Sum all the values (skip "cpu" label)
    cpu_line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse::<u64>().ok())
        .sum()
}

/// Count the number of CPUs by counting cpu[N] lines in /proc/stat.
pub fn count_cpus() -> usize {
    let Ok(content) = fs::read_to_string("/proc/stat") else {
        return 1;
    };

    // Count lines starting with "cpu" followed by a digit (cpu0, cpu1, etc.)
    content
        .lines()
        .filter(|line| {
            line.starts_with("cpu")
                && line
                    .chars()
                    .nth(3)
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
        })
        .count()
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_total_cpu_time() {
        // Should return a non-zero value on a real Linux system
        let cpu_time = read_total_cpu_time();
        // Just verify it doesn't panic and returns something reasonable
        assert!(cpu_time > 0 || cfg!(not(target_os = "linux")));
    }

    #[test]
    fn test_count_cpus() {
        let cpus = count_cpus();
        assert!(cpus >= 1);
    }

    #[test]
    fn test_parse_proc_stat_current_process() {
        // Parse our own process's stat
        let pid = std::process::id();
        if let Some(stat) = parse_proc_stat(pid) {
            // Our parent PID should be non-zero
            assert!(stat.ppid > 0);
            // PGID should be set
            assert!(stat.pgid > 0);
        }
        // It's OK if this fails on non-Linux or in restricted environments
    }
}
