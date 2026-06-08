//
// job_object.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Windows job object support for kernel process cleanup.
//!
//! On Windows there is no implicit parent/child lifetime relationship the way
//! there is on Unix; a child process keeps running after its parent exits
//! unless something explicitly tears it down.
//!
//! Historically the supervisor relied on console inheritance for this: kernels
//! were spawned attached to the supervisor's console, so when the supervisor
//! (and its console) went away, the attached kernels were terminated too.
//! Spawning kernels with `CREATE_NO_WINDOW` gives each kernel its own console
//! (which is necessary to avoid inheriting a dead console; see
//! [`super::startup`]), but it also severs that implicit cleanup and leaves
//! kernels orphaned when the supervisor exits.
//!
//! To restore "kernels die with the supervisor" independent of console
//! inheritance, we place every kernel into a single job object configured with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The supervisor holds the only handle
//! to that job for its entire lifetime; when the supervisor process exits for
//! any reason (graceful shutdown or crash), the handle is closed, and Windows
//! terminates every process remaining in the job.

/// Assign a freshly spawned kernel process to the supervisor's kill-on-close
/// job object so that it is terminated if the supervisor exits.
///
/// On non-Windows platforms this is a no-op; process group / session semantics
/// handle orphan cleanup there.
#[allow(unused_variables)]
pub fn assign_to_supervisor_job(child: &tokio::process::Child) {
    #[cfg(target_os = "windows")]
    {
        windows_impl::assign_to_supervisor_job(child);
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use once_cell::sync::OnceCell;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// Wrapper that lets us cache a raw `HANDLE` in a `static`. The handle is
    /// owned by the supervisor process for its entire lifetime and is never
    /// closed explicitly: it is closed by the OS when the process exits, which
    /// is precisely the event that triggers `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    struct JobHandle(HANDLE);

    // SAFETY: A Windows job object HANDLE is just a kernel handle value; it is
    // safe to use from any thread, and we only ever pass it to thread-safe
    // Win32 APIs.
    #[allow(unsafe_code)]
    unsafe impl Send for JobHandle {}
    #[allow(unsafe_code)]
    unsafe impl Sync for JobHandle {}

    /// The process-wide job object. `None` if it could not be created/configured,
    /// in which case we fall back to leaving the kernel unmanaged rather than
    /// failing the spawn.
    static JOB: OnceCell<Option<JobHandle>> = OnceCell::new();

    fn supervisor_job() -> Option<HANDLE> {
        JOB.get_or_init(|| create_kill_on_close_job().map(JobHandle))
            .as_ref()
            .map(|job| job.0)
    }

    #[allow(unsafe_code)]
    fn create_kill_on_close_job() -> Option<HANDLE> {
        // SAFETY: We pass a null name and null security attributes to create an
        // anonymous job object, then configure it before returning. On failure
        // we close any handle we obtained.
        unsafe {
            let job = match CreateJobObjectW(None, windows::core::PCWSTR::null()) {
                Ok(handle) => handle,
                Err(e) => {
                    log::warn!(
                        "Failed to create job object for kernel cleanup; kernels may \
                         outlive the supervisor on exit: {}",
                        e
                    );
                    return None;
                }
            };

            // Configure the job so that closing the last handle to it (which
            // happens when the supervisor process exits) terminates every
            // process still assigned to the job.
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let info_ptr: *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION = &info;
            if let Err(e) = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                info_ptr.cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) {
                log::warn!(
                    "Failed to configure kill-on-close job object; kernels may outlive \
                     the supervisor on exit: {}",
                    e
                );
                let _ = CloseHandle(job);
                return None;
            }

            log::debug!("Created kill-on-close job object for kernel cleanup");
            Some(job)
        }
    }

    pub fn assign_to_supervisor_job(child: &tokio::process::Child) {
        let job = match supervisor_job() {
            Some(job) => job,
            None => return,
        };

        let raw = match child.raw_handle() {
            Some(handle) => handle,
            None => {
                log::warn!("Kernel process has no handle; cannot assign to job object");
                return;
            }
        };

        // `raw` is already a `*mut c_void` (the Windows process handle).
        let process = HANDLE(raw);

        // SAFETY: `job` is a valid job object handle owned by this process and
        // `process` is the live handle to the just-spawned child.
        #[allow(unsafe_code)]
        unsafe {
            if let Err(e) = AssignProcessToJobObject(job, process) {
                log::warn!(
                    "Failed to assign kernel process to job object; it may outlive the \
                     supervisor on exit: {}",
                    e
                );
            } else {
                log::trace!("Assigned kernel process to supervisor job object");
            }
        }
    }
}
