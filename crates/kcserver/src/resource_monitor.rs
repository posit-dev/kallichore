//
// resource_monitor.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Global resource usage monitor for all kernel sessions.

#[cfg(target_os = "linux")]
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

// =============================================================================
// macOS: use proc_pid_rusage to get phys_footprint (matches Activity Monitor)
// =============================================================================

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos_memory {
    /// rusage_info_v2 layout from Apple's <sys/resource.h>.
    /// We need this struct to read the `ri_phys_footprint` field, which
    /// represents the "real memory" cost of a process — the same metric
    /// that Activity Monitor displays in its Memory column.
    #[repr(C)]
    struct RusageInfoV2 {
        ri_uuid: [u8; 16],
        ri_user_time: u64,
        ri_system_time: u64,
        ri_pkg_idle_wkups: u64,
        ri_interrupt_wkups: u64,
        ri_pageins: u64,
        ri_wired_size: u64,
        ri_resident_size: u64,
        ri_phys_footprint: u64,
        ri_proc_start_abstime: u64,
        ri_proc_exit_abstime: u64,
        ri_child_user_time: u64,
        ri_child_system_time: u64,
        ri_child_pkg_idle_wkups: u64,
        ri_child_interrupt_wkups: u64,
        ri_child_pageins: u64,
        ri_child_elapsed_abstime: u64,
        ri_diskio_bytesread: u64,
        ri_diskio_byteswritten: u64,
    }

    const RUSAGE_INFO_V2: libc::c_int = 2;

    #[link(name = "proc", kind = "dylib")]
    extern "C" {
        fn proc_pid_rusage(
            pid: libc::c_int,
            flavor: libc::c_int,
            buffer: *mut RusageInfoV2,
        ) -> libc::c_int;
    }

    /// Get the physical footprint of a process.
    ///
    /// This uses `proc_pid_rusage` with `RUSAGE_INFO_V2` to read
    /// `ri_phys_footprint`, which includes resident, compressed, and
    /// purgeable-but-dirty memory — matching what Activity Monitor reports.
    ///
    /// The default `sysinfo` crate uses `pti_resident_size` (RSS), which
    /// excludes compressed memory and dramatically undercounts on macOS.
    pub fn get_phys_footprint(pid: u32) -> Option<u64> {
        // SAFETY: proc_pid_rusage is a stable macOS API. We pass a properly
        // sized and zeroed buffer. The function returns 0 on success.
        let mut rusage: RusageInfoV2 = unsafe { std::mem::zeroed() };
        let result = unsafe {
            proc_pid_rusage(pid as libc::c_int, RUSAGE_INFO_V2, &mut rusage)
        };

        if result == 0 {
            Some(rusage.ri_phys_footprint)
        } else {
            None
        }
    }
}

/// CPU usage collected for a process tree (used on non-Linux platforms)
#[cfg(not(target_os = "linux"))]
struct ProcessMetrics {
    cpu_percent: u64,
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

    /// Compute CPU usage percentage for a set of processes.
    /// Returns the total CPU percentage across all PIDs in the set.
    ///
    /// # Arguments
    /// * `pids` - The set of process IDs to compute CPU usage for
    /// * `current_total_cpu` - The current total system CPU time (should be read once per monitoring tick)
    fn compute_cpu_usage(
        &mut self,
        pids: &std::collections::HashSet<u32>,
        current_total_cpu: u64,
    ) -> f32 {
        use crate::proc_stat;

        let total_cpu_delta = current_total_cpu.saturating_sub(self.prev_total_cpu);

        // If no time has passed (or first call), we can't compute usage
        if total_cpu_delta == 0 || self.prev_total_cpu == 0 {
            // Still update the tracking for next time
            for &pid in pids {
                if let Some(stat) = proc_stat::parse_proc_stat(pid) {
                    self.prev_times.insert(pid, stat.cpu_time());
                }
            }
            return 0.0;
        }

        let mut total_process_cpu_delta: u64 = 0;
        let mut new_times = HashMap::new();

        for &pid in pids {
            if let Some(stat) = proc_stat::parse_proc_stat(pid) {
                let current_time = stat.cpu_time();
                new_times.insert(pid, current_time);

                if let Some(&prev_time) = self.prev_times.get(&pid) {
                    total_process_cpu_delta += current_time.saturating_sub(prev_time);
                }
                // If no previous time, this is a new process - contributes 0 to delta
            }
        }

        // Update prev_times with new values (don't replace entirely, as there may be
        // entries for other sessions that we need to preserve)
        for (pid, time) in new_times {
            self.prev_times.insert(pid, time);
        }

        // Compute percentage: (process_delta / total_delta) * 100 * num_cpus
        // The result is scaled to 100% per CPU core (like sysinfo does)
        let num_cpus = proc_stat::count_cpus() as f32;
        (total_process_cpu_delta as f32 / total_cpu_delta as f32) * 100.0 * num_cpus
    }

    /// Update the previous total CPU time after all sessions have been processed.
    /// This should be called once per monitoring tick, after all calls to compute_cpu_usage.
    fn update_prev_total_cpu(&mut self, current_total_cpu: u64) {
        self.prev_total_cpu = current_total_cpu;
    }

    /// Remove stale entries from prev_times that are no longer tracked.
    /// Call this periodically with the set of all currently tracked PIDs across all sessions.
    fn cleanup_stale_entries(&mut self, active_pids: &std::collections::HashSet<u32>) {
        self.prev_times.retain(|pid, _| active_pids.contains(pid));
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

                    // On Linux, read the system CPU time ONCE for all sessions in this tick
                    // This prevents artificial spikes in later sessions due to near-zero time deltas
                    #[cfg(target_os = "linux")]
                    let current_total_cpu = crate::proc_stat::read_total_cpu_time();

                    // Track all PIDs across all sessions for cleanup (Linux only)
                    #[cfg(target_os = "linux")]
                    let mut all_tracked_pids = std::collections::HashSet::new();

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

                        // Track all PIDs for cleanup (Linux only)
                        #[cfg(target_os = "linux")]
                        all_tracked_pids.extend(&tree_pids);

                        // Log trace info about the process tree, including a list of the
                        // PIDs being monitored
                        log::trace!(
                            "[session {}] Monitoring resource usage for process tree with root PID {}: {} processes; {:?}",
                            session_id,
                            pid,
                            tree_pids.len(),
                            tree_pids
                        );

                        // Processes needing to be refreshed
                        let pids_to_refresh: Vec<Pid> =
                            tree_pids.iter().map(|&p| Pid::from_u32(p)).collect();

                        // On macOS, only refresh CPU — memory is collected
                        // via proc_pid_rusage (phys_footprint) instead of sysinfo
                        #[cfg(target_os = "macos")]
                        let refresh_kind = ProcessRefreshKind::new()
                            .with_cpu();

                        // On Windows, refresh both CPU and memory
                        #[cfg(target_os = "windows")]
                        let refresh_kind = ProcessRefreshKind::new()
                            .with_cpu()
                            .with_memory();

                        // We don't refresh CPU on Linux here, because there's a bug in the
                        // sysinfo crate that causes CPU usage to be reported as 0.0
                        // when using ProcessesToUpdate::Some(). Instead, we compute CPU
                        // usage ourselves using /proc data.
                        #[cfg(target_os = "linux")]
                        let refresh_kind = ProcessRefreshKind::new()
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
                            let cpu = cpu_tracker.compute_cpu_usage(&tree_pids, current_total_cpu);
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

                    // On Linux, update the tracker's previous total CPU time after processing all sessions
                    // This ensures all sessions in this tick use the same time delta
                    #[cfg(target_os = "linux")]
                    {
                        cpu_tracker.update_prev_total_cpu(current_total_cpu);
                        // Clean up stale entries from dead processes to prevent memory leak
                        cpu_tracker.cleanup_stale_entries(&all_tracked_pids);
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
    #[cfg(not(target_os = "macos"))] system: &System,
    #[cfg(target_os = "macos")] _system: &System,
    pids: &std::collections::HashSet<u32>,
) -> (u64, u64) {
    let mut total_memory = 0u64;
    let mut total_threads = 0u64;

    for &pid in pids {
        // On macOS, use proc_pid_rusage to get phys_footprint instead of
        // sysinfo's resident_size. Activity Monitor reports phys_footprint,
        // which includes compressed memory — resident_size does not, and can
        // undercount by 10x or more on idle processes.
        #[cfg(target_os = "macos")]
        {
            if let Some(footprint) = macos_memory::get_phys_footprint(pid) {
                total_memory += footprint;
                total_threads += 1;
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
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
                    // On Windows, tasks() is not available
                    // Assume 1 thread per process as a baseline
                    total_threads += 1;
                }
            }
        }
    }

    (total_memory, total_threads)
}

/// Collect CPU metrics for a set of processes.
///
/// This function sums CPU usage for all processes in the provided set of PIDs.
/// Memory and thread counts are collected separately by `collect_memory_and_threads`.
///
/// # Arguments
///
/// * `system` - The sysinfo System instance (must have been refreshed for the given PIDs)
/// * `pids` - Set of process IDs to collect metrics for
///
/// # Returns
///
/// Aggregated CPU metrics for the processes
#[cfg(not(target_os = "linux"))]
fn collect_tree_metrics(system: &System, pids: &std::collections::HashSet<u32>) -> ProcessMetrics {
    let mut total_cpu = 0.0f32;

    // Sum CPU for all processes in tree (using cached data)
    for &pid in pids {
        let sysinfo_pid = Pid::from_u32(pid);
        if let Some(proc) = system.process(sysinfo_pid) {
            total_cpu += proc.cpu_usage();
        }
    }

    ProcessMetrics {
        cpu_percent: total_cpu.round() as u64,
    }
}
