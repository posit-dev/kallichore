//
// resource_monitor.rs
//
// Copyright (C) 2024-2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Global resource usage monitor for all kernel sessions.
//!
//! This module provides efficient resource monitoring by:
//! - Running a single global task that refreshes process data once per interval
//! - Iterating over all connected kernel sessions to collect and send metrics
//! - Avoiding redundant process table scans (one refresh serves all kernels)

use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use kcshared::kernel_message::{KernelMessage, ResourceUpdate};
use kcshared::websocket_message::WebsocketMessage;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;

use crate::kernel_session::KernelSession;

/// Metrics collected for a process tree
struct ProcessMetrics {
    cpu_percent: u64,
    memory_bytes: u64,
    thread_count: u64,
}

/// Start the global resource monitor.
///
/// This function spawns a background task that periodically samples resource
/// usage for all connected kernel sessions. It is efficient because it:
/// - Refreshes the process table once per interval (not per kernel)
/// - Only sends updates to sessions with connected clients
/// - Reuses the System instance for accurate CPU measurements
/// - Supports dynamic interval updates via a channel
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

        // Consume the first tick immediately
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Skip if monitoring is disabled
                    if current_sample_interval_ms == 0 {
                        continue;
                    }

                    // Refresh all process data once
                    system.refresh_processes(ProcessesToUpdate::All);

                    // Get the current timestamp
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);

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
                            None => continue,
                        };

                        // Release the state lock before collecting metrics
                        drop(state_guard);

                        // Collect metrics for this kernel's process tree
                        let metrics = collect_tree_metrics(&system, pid);

                        // Create the resource update message
                        let update = ResourceUpdate {
                            cpu_percent: metrics.cpu_percent,
                            memory_bytes: metrics.memory_bytes,
                            thread_count: metrics.thread_count,
                            sampling_period_ms: current_sample_interval_ms,
                            timestamp,
                        };

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

/// Collect metrics for a process and all its descendants.
///
/// This function walks the process tree starting from the given root PID,
/// summing CPU usage, memory, and thread counts for the entire tree.
///
/// # Arguments
///
/// * `system` - The sysinfo System instance (must have been refreshed)
/// * `root_pid` - The root process ID to start from
///
/// # Returns
///
/// Aggregated metrics for the process tree
fn collect_tree_metrics(system: &System, root_pid: u32) -> ProcessMetrics {
    let pid = Pid::from_u32(root_pid);

    let mut total_cpu = 0.0f32;
    let mut total_memory = 0u64;
    let mut total_threads = 0u64;

    // Collect all PIDs in process tree using BFS
    let mut pids_to_check = vec![pid];
    let mut visited = HashSet::new();

    while let Some(check_pid) = pids_to_check.pop() {
        if !visited.insert(check_pid) {
            continue; // Already visited
        }

        // Find children by checking parent_pid
        for (child_pid, proc) in system.processes() {
            if proc.parent() == Some(check_pid) && !visited.contains(child_pid) {
                pids_to_check.push(*child_pid);
            }
        }
    }

    // Sum metrics for all processes in tree (using cached data)
    for pid in &visited {
        if let Some(proc) = system.process(*pid) {
            total_cpu += proc.cpu_usage();
            total_memory += proc.memory();
            // Thread count: use tasks() if available, otherwise assume 1 thread
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                if let Some(tasks) = proc.tasks() {
                    total_threads += tasks.len() as u64;
                } else {
                    total_threads += 1;
                }
            }
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            {
                // On macOS and Windows, tasks() is not available
                // Assume 1 thread per process as a baseline
                total_threads += 1;
            }
        }
    }

    ProcessMetrics {
        cpu_percent: total_cpu as u64,
        memory_bytes: total_memory,
        thread_count: total_threads,
    }
}
