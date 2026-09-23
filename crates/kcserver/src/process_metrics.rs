//
// process_metrics.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Per-process resource sampling.
//!
//! Every implementation here reads only the PIDs it is handed; none of them
//! enumerate the system process table.

use std::collections::HashSet;

/// A point-in-time sample of one process's resource usage.
pub struct ProcessSample {
    pub pid: u32,
    /// CPU time consumed over the process's lifetime, in nanoseconds.
    pub cpu_time_ns: u64,
    pub memory_bytes: u64,
    pub thread_count: u64,
}

/// Sample the given processes.
///
/// Processes that have exited, or that we aren't allowed to query, are
/// skipped, so the result may be shorter than `pids`.
pub fn sample(pids: &HashSet<u32>) -> Vec<ProcessSample> {
    pids.iter().filter_map(|&pid| sample_one(pid)).collect()
}

#[cfg(target_os = "linux")]
use linux::sample_one;

#[cfg(target_os = "macos")]
use macos::sample_one;

#[cfg(target_os = "windows")]
use windows_impl::sample_one;

// =============================================================================
// Linux: /proc/[pid]/stat parsing
//
// The parser is compiled into test builds on every platform so that the field
// offsets, which are the easy thing to get wrong here, stay covered wherever
// the tests are run.
// =============================================================================

#[cfg(any(target_os = "linux", test))]
mod proc_stat {
    /// The fields of `/proc/[pid]/stat` that we care about.
    pub struct ProcStat {
        /// `utime` + `stime`, in clock ticks.
        pub cpu_ticks: u64,
        pub num_threads: u64,
        pub rss_pages: u64,
    }

    /// Parse the contents of a `/proc/[pid]/stat` file.
    ///
    /// Counting from just after the parenthesized `comm` field, the fields we
    /// want are 11 (`utime`), 12 (`stime`), 17 (`num_threads`) and 21 (`rss`,
    /// in pages).
    pub fn parse(content: &str) -> Option<ProcStat> {
        // `comm` is parenthesized and may itself contain spaces and
        // parentheses, so the fields we want start after the last ')'.
        let after_comm = content.get(content.rfind(')')? + 2..)?;
        let fields: Vec<&str> = after_comm.split_whitespace().collect();
        if fields.len() < 22 {
            return None;
        }

        let utime: u64 = fields[11].parse().ok()?;
        let stime: u64 = fields[12].parse().ok()?;

        Some(ProcStat {
            cpu_ticks: utime.saturating_add(stime),
            num_threads: fields[17].parse::<i64>().ok()?.max(1) as u64,
            rss_pages: fields[21].parse().ok()?,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// utime 42, stime 17, num_threads 5, rss 987 pages.
        const SAMPLE: &str = "1234 (bash) S 1200 1234 1234 34816 1234 4194304 1500 200 0 0 \
42 17 3 1 20 0 5 0 987654 12345678 987 18446744073709551615";

        #[test]
        fn reads_the_expected_fields() {
            let stat = parse(SAMPLE).expect("sample should parse");
            assert_eq!(stat.cpu_ticks, 59);
            assert_eq!(stat.num_threads, 5);
            assert_eq!(stat.rss_pages, 987);
        }

        #[test]
        fn handles_spaces_and_parens_in_comm() {
            let stat = parse(&SAMPLE.replace("(bash)", "(od d) name)")).expect("should parse");
            assert_eq!(stat.cpu_ticks, 59);
            assert_eq!(stat.rss_pages, 987);
        }

        #[test]
        fn rejects_truncated_lines() {
            assert!(parse("1234 (bash) S 1200 1234").is_none());
            assert!(parse("nonsense").is_none());
        }
    }
}

// =============================================================================
// Linux: a single read of /proc/[pid]/stat
// =============================================================================

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod linux {
    use std::fs;
    use std::sync::OnceLock;

    use super::ProcessSample;

    /// Clock ticks per second, used to convert the jiffy counts in
    /// `/proc/[pid]/stat` into nanoseconds.
    fn clock_ticks_per_sec() -> u64 {
        static TICKS: OnceLock<u64> = OnceLock::new();
        *TICKS.get_or_init(|| {
            // SAFETY: sysconf has no preconditions beyond a valid name.
            let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
            if ticks > 0 {
                ticks as u64
            } else {
                100
            }
        })
    }

    fn page_size() -> u64 {
        static SIZE: OnceLock<u64> = OnceLock::new();
        *SIZE.get_or_init(|| {
            // SAFETY: sysconf has no preconditions beyond a valid name.
            let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            if size > 0 {
                size as u64
            } else {
                4096
            }
        })
    }

    pub fn sample_one(pid: u32) -> Option<ProcessSample> {
        let content = fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
        let stat = super::proc_stat::parse(&content)?;

        Some(ProcessSample {
            pid,
            cpu_time_ns: stat.cpu_ticks.saturating_mul(1_000_000_000) / clock_ticks_per_sec(),
            memory_bytes: stat.rss_pages.saturating_mul(page_size()),
            thread_count: stat.num_threads,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn sample_current_process() {
            let sample = sample_one(std::process::id()).expect("our own process is sampleable");
            assert!(sample.memory_bytes > 0);
            assert!(sample.thread_count >= 1);
        }

        #[test]
        fn missing_process_is_none() {
            // PID 0 is never a real process in /proc.
            assert!(sample_one(0).is_none());
        }
    }
}

// =============================================================================
// macOS: libproc
// =============================================================================

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
    use std::sync::OnceLock;

    use super::ProcessSample;

    /// The ratio `mach_timebase_info` reports, declared here because the
    /// `libc` binding for it is deprecated in favour of a crate we don't need.
    #[repr(C)]
    struct MachTimebaseInfo {
        numer: u32,
        denom: u32,
    }

    extern "C" {
        fn mach_timebase_info(info: *mut MachTimebaseInfo) -> libc::c_int;
    }

    /// Convert a Mach absolute time duration to nanoseconds.
    ///
    /// libproc reports CPU times in Mach absolute time units, which are
    /// nanoseconds on Intel but not on Apple silicon, where one unit is 125/3
    /// nanoseconds. Skipping this conversion undercounts CPU by 42x there.
    fn mach_time_to_ns(units: u64) -> u64 {
        static TIMEBASE: OnceLock<(u64, u64)> = OnceLock::new();
        let (numer, denom) = *TIMEBASE.get_or_init(|| {
            let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
            // SAFETY: mach_timebase_info only writes to the struct we pass it.
            if unsafe { mach_timebase_info(&mut info) } == 0 && info.denom != 0 {
                (info.numer as u64, info.denom as u64)
            } else {
                (1, 1)
            }
        });
        units.saturating_mul(numer) / denom
    }

    /// Read the number of threads in a process.
    fn thread_count(pid: u32) -> u64 {
        let mut task: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        let size = size_of::<libc::proc_taskinfo>() as libc::c_int;
        // SAFETY: the buffer and size match the PROC_PIDTASKINFO flavor. The
        // call returns the number of bytes it wrote.
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTASKINFO,
                0,
                std::ptr::addr_of_mut!(task) as *mut libc::c_void,
                size,
            )
        };
        if written == size {
            task.pti_threadnum.max(1) as u64
        } else {
            1
        }
    }

    pub fn sample_one(pid: u32) -> Option<ProcessSample> {
        // `ri_phys_footprint` includes resident, compressed and
        // purgeable-but-dirty memory, so it matches the Memory column in
        // Activity Monitor. Resident size alone can undercount by 10x or more
        // on an idle process.
        let mut rusage: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        // SAFETY: we pass a zeroed buffer matching the flavor we ask for.
        // proc_pid_rusage returns 0 on success.
        let rc = unsafe {
            libc::proc_pid_rusage(
                pid as libc::c_int,
                libc::RUSAGE_INFO_V2,
                std::ptr::addr_of_mut!(rusage) as *mut libc::rusage_info_t,
            )
        };
        if rc != 0 {
            return None;
        }

        Some(ProcessSample {
            pid,
            cpu_time_ns: mach_time_to_ns(
                rusage.ri_user_time.saturating_add(rusage.ri_system_time),
            ),
            memory_bytes: rusage.ri_phys_footprint,
            thread_count: thread_count(pid),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn sample_current_process() {
            let sample = sample_one(std::process::id()).expect("our own process is sampleable");
            assert!(sample.memory_bytes > 0);
            assert!(sample.thread_count >= 1);
        }

        #[test]
        fn missing_process_is_none() {
            assert!(sample_one(0).is_none());
        }

        #[test]
        fn timebase_is_applied() {
            // A zero duration stays zero whatever the timebase, and a non-zero
            // one must survive the conversion.
            assert_eq!(mach_time_to_ns(0), 0);
            assert!(mach_time_to_ns(1_000_000) > 0);
        }
    }
}

// =============================================================================
// Windows: one process handle per PID
// =============================================================================

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod windows_impl {
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    use super::ProcessSample;

    /// A `FILETIME` counts 100-nanosecond intervals.
    fn filetime_ns(time: FILETIME) -> u64 {
        (((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64) * 100
    }

    pub fn sample_one(pid: u32) -> Option<ProcessSample> {
        // `GetProcessMemoryInfo` wants PROCESS_VM_READ on top of the query
        // right; if we can't get it we still report CPU and threads.
        // SAFETY: OpenProcess either returns a valid handle or an error.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                false,
                pid,
            )
            .or_else(|_| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid))
        }
        .ok()?;

        let sample = sample_with_handle(handle, pid);

        // SAFETY: `handle` came from OpenProcess and is not used after this.
        unsafe {
            let _ = CloseHandle(handle);
        }

        sample
    }

    fn sample_with_handle(handle: HANDLE, pid: u32) -> Option<ProcessSample> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: `handle` is live and all four outputs are valid pointers.
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) }
            .ok()?;

        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        let size = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        // SAFETY: `counters` matches the size we declare.
        let memory_bytes = unsafe { GetProcessMemoryInfo(handle, &mut counters, size) }
            .map(|()| counters.WorkingSetSize as u64)
            .unwrap_or(0);

        Some(ProcessSample {
            pid,
            cpu_time_ns: filetime_ns(kernel).saturating_add(filetime_ns(user)),
            memory_bytes,
            // Windows exposes no per-process thread count that doesn't require
            // a system-wide snapshot, so each process counts as one.
            thread_count: 1,
        })
    }
}
