//
// resource_monitor.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Global resource usage monitor for all kernel sessions.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use kcshared::kernel_message::{KernelMessage, ResourceUpdate};
use kcshared::websocket_message::WebsocketMessage;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;

use crate::kernel_session::KernelSession;
use crate::process_tree;

/// Metrics collected for a process tree (used on non-Linux platforms)
#[cfg(not(target_os = "linux"))]
struct ProcessMetrics {
    cpu_percent: u64,
    memory_bytes: u64,
    thread_count: u64,
}

/// Tracks CPU times for computing CPU usage percentage on Linux.
///
/// sysinfo doesn't compute CPU usage when using ProcessesToUpdate::Some(),
/// so we track CPU times ourselves and compute the percentage manually.
#[cfg(target_os = "linux")]
struct CpuTracker {
    /// Previous CPU times per process: pid -> (utime + stime)
    prev_times: HashMap<u32, u64>,
    /// Previous total system CPU time (sum of all CPU jiffies)
    prev_total_cpu: u64,
}

#[cfg(target_os = "linux")]
impl CpuTracker {
    fn new() -> Self {
        Self {
            prev_times: HashMap::new(),
            prev_total_cpu: 0,
        }
    }

    /// Read total CPU time from /proc/stat (sum of all jiffies across all CPUs)
    fn read_total_cpu_time() -> u64 {
        let Ok(content) = std::fs::read_to_string("/proc/stat") else {
            return 0;
        };

        // First line is "cpu  user nice system idle iowait irq softirq steal guest guest_nice"
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

    /// Read CPU time (utime + stime) for a process from /proc/[pid]/stat
    fn read_process_cpu_time(pid: u32) -> Option<u64> {
        let stat_path = format!("/proc/{}/stat", pid);
        let content = std::fs::read_to_string(stat_path).ok()?;

        // Format: pid (comm) state ppid pgrp session tty_nr tpgid flags
        //         minflt cminflt majflt cmajflt utime stime cutime cstime ...
        // Fields 14 and 15 (1-indexed) are utime and stime
        // comm can contain spaces and parens, so find the last ')'
        let last_paren = content.rfind(')')?;
        let fields_after_comm = &content[last_paren + 2..]; // Skip ") "
        let fields: Vec<&str> = fields_after_comm.split_whitespace().collect();

        // fields[0] = state, fields[1] = ppid, ..., fields[11] = utime, fields[12] = stime
        if fields.len() < 13 {
            return None;
        }

        let utime: u64 = fields[11].parse().ok()?;
        let stime: u64 = fields[12].parse().ok()?;
        Some(utime + stime)
    }

    /// Compute CPU usage percentage for a set of processes.
    /// Returns the total CPU percentage across all PIDs in the set.
    fn compute_cpu_usage(&mut self, pids: &std::collections::HashSet<u32>) -> f32 {
        // Read current total CPU time
        let current_total_cpu = Self::read_total_cpu_time();
        let total_cpu_delta = current_total_cpu.saturating_sub(self.prev_total_cpu);

        // If no time has passed (or first call), we can't compute usage
        if total_cpu_delta == 0 || self.prev_total_cpu == 0 {
            // Still update the tracking for next time
            self.prev_total_cpu = current_total_cpu;
            for &pid in pids {
                if let Some(cpu_time) = Self::read_process_cpu_time(pid) {
                    self.prev_times.insert(pid, cpu_time);
                }
            }
            return 0.0;
        }

        let mut total_process_cpu_delta: u64 = 0;
        let mut new_times = HashMap::new();

        for &pid in pids {
            if let Some(current_time) = Self::read_process_cpu_time(pid) {
                new_times.insert(pid, current_time);

                if let Some(&prev_time) = self.prev_times.get(&pid) {
                    total_process_cpu_delta += current_time.saturating_sub(prev_time);
                }
                // If no previous time, this is a new process - contributes 0 to delta
            }
        }

        // Update state for next call
        self.prev_total_cpu = current_total_cpu;
        // Update prev_times with new values (don't replace entirely, as there may be
        // entries for other sessions that we need to preserve)
        for (pid, time) in new_times {
            self.prev_times.insert(pid, time);
        }

        // Compute percentage: (process_delta / total_delta) * 100 * num_cpus
        // The result is scaled to 100% per CPU core (like sysinfo does)
        let num_cpus = Self::count_cpus() as f32;
        (total_process_cpu_delta as f32 / total_cpu_delta as f32) * 100.0 * num_cpus
    }

    /// Count the number of CPUs by reading /proc/stat
    fn count_cpus() -> usize {
        let Ok(content) = std::fs::read_to_string("/proc/stat") else {
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
}

/// Start the global resource monitor.
///
/// This function spawns a background task that periodically samples resource
/// usage for all connected kernel sessions.
///
/// # Arguments
///
/// * `kernel_sessions` - Shared access to all kernel sessions
/// * `sample_interval_ms` - Initial sampling interval in milliseconds (0 disables monitoring)
/// * `interval_update_rx` - Receiver for interval update requests
/// * `current_interval` - Shared storage for the current interval value
pub fn start_global_resource_monitor(
    kernel_sessions: Arc<RwLock<Vec<KernelSession>>>,
    sample_interval_ms: u64,
    mut interval_update_rx: mpsc::Receiver<u64>,
    current_interval: Arc<RwLock<u64>>,
) {
    // Don't start if monitoring is disabled
    if sample_interval_ms == 0 {
        log::info!("Resource monitoring disabled (sample_interval_ms = 0)");
        // Still spawn the task to handle potential enable requests
    } else {
        log::info!(
            "Starting global resource monitor with {}ms interval",
            sample_interval_ms
        );
    }

    tokio::spawn(async move {
        // Create a System instance and keep it alive for accurate CPU measurements
        let mut system = System::new();

        // On Linux, use our own CPU tracker since sysinfo doesn't compute CPU
        // usage when using ProcessesToUpdate::Some()
        #[cfg(target_os = "linux")]
        let mut cpu_tracker = CpuTracker::new();

        // Track current interval
        let mut current_sample_interval_ms = sample_interval_ms;

        // Create the interval timer (or use a very long interval if disabled)
        let effective_interval = if current_sample_interval_ms == 0 {
            Duration::from_secs(3600) // 1 hour when disabled
        } else {
            Duration::from_millis(current_sample_interval_ms)
        };
        let mut interval = tokio::time::interval(effective_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        // Prime CPU usage statistics (sysinfo needs an initial refresh)
        system.refresh_cpu_usage();
        // Consume the first tick immediately
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Skip if monitoring is disabled
                    if current_sample_interval_ms == 0 {
                        continue;
                    }

                    // Clone session data we need while holding the lock briefly
                    // This avoids holding the std::sync::RwLock across await points
                    let session_data: Vec<_> = {
                        let sessions = match kernel_sessions.read() {
                            Ok(guard) => guard,
                            Err(e) => {
                                log::error!("Failed to acquire read lock on kernel_sessions: {}", e);
                                continue;
                            }
                        };

                        sessions
                            .iter()
                            .map(|s| {
                                (
                                    s.connection.session_id.clone(),
                                    s.state.clone(),
                                    s.ws_json_tx.clone(),
                                )
                            })
                            .collect()
                    };
                    // Lock is now released

                    // Check if any clients are connected before doing any work
                    let mut has_connected_clients = false;
                    for (_, state, _) in &session_data {
                        let state_guard = state.read().await;
                        if state_guard.connected {
                            has_connected_clients = true;
                            break;
                        }
                    }

                    // Skip all work if no clients are connected
                    if !has_connected_clients {
                        continue;
                    }

                    // Get the current timestamp
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);

                    for (session_id, state, ws_json_tx) in session_data {
                        // Read the kernel state (tokio::sync::RwLock)
                        let state_guard = state.read().await;

                        // Skip if no client is connected
                        if !state_guard.connected {
                            continue;
                        }

                        // Skip if no process is running
                        let pid = match state_guard.process_id {
                            Some(pid) => pid,
                            None => {
                                continue;
                            }
                        };

                        // Release the state lock before collecting metrics
                        drop(state_guard);

                        // Get the process tree using OS-specific efficient enumeration
                        let tree_pids = process_tree::get_process_tree(pid);

                        // Log trace info about the process tree, including a list of the
                        // PIDs being monitored
                        log::trace!(
                            "[session {}] Monitoring resource usage for process tree with root PID {}: {} processes; {:?}",
                            session_id,
                            pid,
                            tree_pids.len(),
                            tree_pids
                        );

                        // Refresh only the processes we need for memory info
                        // On non-Linux platforms, also request CPU since sysinfo computes it correctly
                        let pids_to_refresh: Vec<Pid> =
                            tree_pids.iter().map(|&p| Pid::from_u32(p)).collect();

                        #[cfg(target_os = "linux")]
                        let refresh_kind = ProcessRefreshKind::new()
                            .with_memory();

                        #[cfg(not(target_os = "linux"))]
                        let refresh_kind = ProcessRefreshKind::new()
                            .with_cpu()
                            .with_memory();

                        system.refresh_processes_specifics(
                            ProcessesToUpdate::Some(&pids_to_refresh),
                            refresh_kind,
                        );

                        // Update the process cache tick counter (Windows only)
                        process_tree::tick_process_cache(pid);

                        // Collect metrics for this kernel's process tree
                        // On Linux, compute CPU ourselves; on other platforms use sysinfo
                        #[cfg(target_os = "linux")]
                        let cpu_percent = {
                            let cpu = cpu_tracker.compute_cpu_usage(&tree_pids);
                            // Note: We don't call clear_stale_entries here because this loop
                            // processes multiple sessions, and clearing based on one session's
                            // PIDs would remove tracking for other sessions. Stale entries
                            // (for dead processes) are harmless and will be ignored.
                            cpu.round() as u64
                        };

                        #[cfg(not(target_os = "linux"))]
                        let cpu_percent = {
                            let metrics = collect_tree_metrics(&system, &tree_pids);
                            metrics.cpu_percent
                        };

                        let (memory_bytes, thread_count) = collect_memory_and_threads(&system, &tree_pids);

                        // Create the resource update message
                        let update = ResourceUpdate {
                            cpu_percent,
                            memory_bytes,
                            thread_count,
                            sampling_period_ms: current_sample_interval_ms,
                            timestamp,
                        };

                        // Store the resource usage in the session state
                        {
                            let mut state_guard = state.write().await;
                            state_guard.resource_usage =
                                Some(kallichore_api::models::ResourceUsage {
                                    cpu_percent: cpu_percent as i64,
                                    memory_bytes: memory_bytes as i64,
                                    thread_count: thread_count as i64,
                                    sampling_period_ms: current_sample_interval_ms as i64,
                                    timestamp: timestamp as i64,
                                });
                        }

                        let msg = WebsocketMessage::Kernel(KernelMessage::ResourceUsage(update));

                        // Send the update (non-blocking, ignore errors if channel is full)
                        if let Err(e) = ws_json_tx.try_send(msg) {
                            log::trace!(
                                "[session {}] Failed to send resource update: {}",
                                session_id,
                                e
                            );
                        }
                    }
                }
                Some(new_interval_ms) = interval_update_rx.recv() => {
                    log::info!(
                        "Updating resource sample interval from {}ms to {}ms",
                        current_sample_interval_ms,
                        new_interval_ms
                    );

                    current_sample_interval_ms = new_interval_ms;

                    // Update the shared storage
                    if let Ok(mut guard) = current_interval.write() {
                        *guard = new_interval_ms;
                    }

                    // Recreate the interval with the new duration
                    let effective_interval = if new_interval_ms == 0 {
                        Duration::from_secs(3600) // 1 hour when disabled
                    } else {
                        Duration::from_millis(new_interval_ms)
                    };
                    interval = tokio::time::interval(effective_interval);
                    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

                    // Consume the first tick immediately
                    interval.tick().await;
                }
            }
        }
    });
}

/// Collect memory and thread count for a set of processes.
///
/// This function sums memory and thread counts for all processes in the provided set of PIDs.
/// CPU usage is handled separately on Linux due to sysinfo limitations.
///
/// # Arguments
///
/// * `system` - The sysinfo System instance (must have been refreshed for the given PIDs)
/// * `pids` - Set of process IDs to collect metrics for
///
/// # Returns
///
/// Tuple of (memory_bytes, thread_count)
fn collect_memory_and_threads(
    system: &System,
    pids: &std::collections::HashSet<u32>,
) -> (u64, u64) {
    let mut total_memory = 0u64;
    let mut total_threads = 0u64;

    for &pid in pids {
        let sysinfo_pid = Pid::from_u32(pid);
        if let Some(proc) = system.process(sysinfo_pid) {
            total_memory += proc.memory();
            // Thread count: use tasks() if available, otherwise assume 1 thread
            #[cfg(target_os = "linux")]
            {
                if let Some(tasks) = proc.tasks() {
                    total_threads += tasks.len() as u64;
                } else {
                    total_threads += 1;
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                // On macOS and Windows, tasks() is not available
                // Assume 1 thread per process as a baseline
                total_threads += 1;
            }
        }
    }

    (total_memory, total_threads)
}

/// Collect metrics for a set of processes.
///
/// This function sums CPU usage, memory, and thread counts for all processes
/// in the provided set of PIDs.
///
/// # Arguments
///
/// * `system` - The sysinfo System instance (must have been refreshed for the given PIDs)
/// * `pids` - Set of process IDs to collect metrics for
///
/// # Returns
///
/// Aggregated metrics for the processes
#[cfg(not(target_os = "linux"))]
fn collect_tree_metrics(system: &System, pids: &std::collections::HashSet<u32>) -> ProcessMetrics {
    let mut total_cpu = 0.0f32;
    let mut total_memory = 0u64;
    let mut total_threads = 0u64;

    // Sum metrics for all processes in tree (using cached data)
    for &pid in pids {
        let sysinfo_pid = Pid::from_u32(pid);
        if let Some(proc) = system.process(sysinfo_pid) {
            total_cpu += proc.cpu_usage();
            total_memory += proc.memory();
            // On macOS and Windows, tasks() is not available
            // Assume 1 thread per process as a baseline
            total_threads += 1;
        }
    }

    ProcessMetrics {
        cpu_percent: total_cpu.round() as u64,
        memory_bytes: total_memory,
        thread_count: total_threads,
    }
}
