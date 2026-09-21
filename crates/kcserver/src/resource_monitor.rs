//
// resource_monitor.rs
//
// Copyright (C) 2024-2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Global resource usage monitor for all kernel sessions.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use kcshared::kernel_message::{KernelMessage, ResourceUpdate};
use kcshared::websocket_message::WebsocketMessage;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;

use crate::kernel_session::KernelSession;
use crate::process_metrics::{self, ProcessSample};
use crate::process_tree;

/// Settings for the resource usage monitor.
#[derive(Clone, Copy, Debug)]
pub struct ResourceMonitorConfig {
    /// How often to sample, in milliseconds. A value of 0 disables sampling.
    pub sample_interval_ms: u64,

    /// Whether a session's child processes count towards its reported usage.
    /// When false, only the session's own process is measured and no child
    /// enumeration is performed at all.
    pub include_children: bool,
}

impl Default for ResourceMonitorConfig {
    fn default() -> Self {
        Self {
            sample_interval_ms: 1000,
            include_children: true,
        }
    }
}

/// Turns the cumulative CPU times of a session's processes into a usage
/// percentage, where 100 means one core fully busy.
#[derive(Default)]
struct CpuTracker {
    /// Cumulative CPU time per PID as of the previous sample.
    previous: HashMap<u32, u64>,

    /// When the previous sample was taken.
    sampled_at: Option<Instant>,
}

impl CpuTracker {
    fn usage_percent(&mut self, samples: &[ProcessSample], now: Instant) -> u64 {
        let elapsed = self.sampled_at.replace(now).map(|then| now - then);

        let mut busy_ns: u64 = 0;
        let mut current = HashMap::with_capacity(samples.len());
        for sample in samples {
            // A process we haven't seen before contributes nothing this time
            // around: we only know how much CPU it burned while we watched it.
            if let Some(previous) = self.previous.get(&sample.pid) {
                busy_ns += sample.cpu_time_ns.saturating_sub(*previous);
            }
            current.insert(sample.pid, sample.cpu_time_ns);
        }

        // Replacing the map drops the PIDs that have since exited.
        self.previous = current;

        match elapsed.map(|elapsed| elapsed.as_nanos()) {
            Some(elapsed_ns) if elapsed_ns > 0 => (busy_ns as u128 * 100 / elapsed_ns) as u64,
            _ => 0,
        }
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
/// * `config` - Initial monitor settings
/// * `interval_update_rx` - Receiver for interval update requests
/// * `current_interval` - Shared storage for the current interval value
pub fn start_global_resource_monitor(
    kernel_sessions: Arc<RwLock<Vec<KernelSession>>>,
    config: ResourceMonitorConfig,
    mut interval_update_rx: mpsc::Receiver<u64>,
    current_interval: Arc<RwLock<u64>>,
) {
    // Don't start if monitoring is disabled
    if config.sample_interval_ms == 0 {
        log::info!("Resource monitoring disabled (sample_interval_ms = 0)");
        // Still spawn the task to handle potential enable requests
    } else {
        log::info!(
            "Starting global resource monitor with {}ms interval (child processes {})",
            config.sample_interval_ms,
            if config.include_children {
                "included"
            } else {
                "excluded"
            }
        );
    }

    tokio::spawn(async move {
        // One CPU tracker per session, keyed by session ID
        let mut trackers: HashMap<String, CpuTracker> = HashMap::new();

        // Track current interval
        let mut current_sample_interval_ms = config.sample_interval_ms;

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

                    // Read the clock once so every session in this tick
                    // measures against the same elapsed time
                    let now = Instant::now();

                    // Sessions we sampled on this tick; anything else has its
                    // CPU tracker discarded below
                    let mut sampled_sessions = HashSet::new();

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

                        let pids = if config.include_children {
                            process_tree::get_process_tree(&session_id, pid)
                        } else {
                            HashSet::from([pid])
                        };

                        let samples = process_metrics::sample(&pids);

                        log::trace!(
                            "[session {}] Monitoring resource usage for process tree with root PID {}: {} processes; {:?}",
                            session_id,
                            pid,
                            pids.len(),
                            pids
                        );

                        let memory_bytes: u64 =
                            samples.iter().map(|sample| sample.memory_bytes).sum();
                        let thread_count: u64 =
                            samples.iter().map(|sample| sample.thread_count).sum();
                        let cpu_percent = trackers
                            .entry(session_id.clone())
                            .or_default()
                            .usage_percent(&samples, now);

                        sampled_sessions.insert(session_id.clone());

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

                    // Drop trackers for sessions that have gone away
                    trackers.retain(|session_id, _| sampled_sessions.contains(session_id));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(pid: u32, cpu_time_ns: u64) -> ProcessSample {
        ProcessSample {
            pid,
            cpu_time_ns,
            memory_bytes: 0,
            thread_count: 1,
        }
    }

    #[test]
    fn first_sample_reports_nothing() {
        let mut tracker = CpuTracker::default();
        assert_eq!(tracker.usage_percent(&[sample(1, 5_000_000)], Instant::now()), 0);
    }

    #[test]
    fn one_busy_core_reads_as_100_percent() {
        let mut tracker = CpuTracker::default();
        let start = Instant::now();
        tracker.usage_percent(&[sample(1, 0)], start);

        // One second of CPU over one second of wall time
        let later = start + Duration::from_secs(1);
        assert_eq!(tracker.usage_percent(&[sample(1, 1_000_000_000)], later), 100);
    }

    #[test]
    fn usage_sums_across_processes() {
        let mut tracker = CpuTracker::default();
        let start = Instant::now();
        tracker.usage_percent(&[sample(1, 0), sample(2, 0)], start);

        let later = start + Duration::from_secs(1);
        let usage =
            tracker.usage_percent(&[sample(1, 1_000_000_000), sample(2, 500_000_000)], later);
        assert_eq!(usage, 150);
    }

    #[test]
    fn newly_discovered_process_does_not_spike() {
        let mut tracker = CpuTracker::default();
        let start = Instant::now();
        tracker.usage_percent(&[sample(1, 0)], start);

        // PID 2 shows up already holding hours of CPU time; it must not be
        // counted as though it burned all of that since the last tick.
        let later = start + Duration::from_secs(1);
        let usage = tracker.usage_percent(&[sample(1, 0), sample(2, 3_600_000_000_000)], later);
        assert_eq!(usage, 0);
    }

    #[test]
    fn exited_processes_are_forgotten() {
        let mut tracker = CpuTracker::default();
        let start = Instant::now();
        tracker.usage_percent(&[sample(1, 0), sample(2, 0)], start);
        tracker.usage_percent(&[sample(1, 0)], start + Duration::from_secs(1));
        assert_eq!(tracker.previous.len(), 1);
    }
}
