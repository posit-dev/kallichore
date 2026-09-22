//
// job_object.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Windows job object support for kernel process cleanup and accounting.
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
//!
//! Each kernel additionally gets its own job, nested inside the supervisor's.
//! Job membership is inherited by child processes, so this per-session job is
//! how the resource monitor learns which processes belong to a session without
//! taking a snapshot of the whole system. These nested jobs carry no limits of
//! their own; cleanup remains the supervisor job's responsibility.

/// Assign a freshly spawned kernel process to the supervisor's kill-on-close
/// job object, and to a per-session job used for resource accounting.
///
/// On non-Windows platforms this is a no-op; process group / session semantics
/// handle orphan cleanup there, and the process tree is read from the OS
/// directly.
#[allow(unused_variables)]
pub fn assign_to_jobs(session_id: &str, child: &tokio::process::Child) {
    #[cfg(target_os = "windows")]
    {
        windows_impl::assign_to_jobs(session_id, child);
    }
}

/// Release the per-session job object held for a kernel, if any. Called when
/// the kernel process exits, and again when the session is deleted; releasing
/// a session that holds no job is a no-op.
///
/// This does not terminate anything: the per-session job sets no limits.
#[allow(unused_variables)]
pub fn release_session_job(session_id: &str) {
    #[cfg(target_os = "windows")]
    {
        windows_impl::release_session_job(session_id);
    }
}

/// List the PIDs belonging to a kernel's job object, which is every process it
/// has spawned, however deeply nested.
///
/// Returns `None` if the session has no job object, e.g. because it could not
/// be created at spawn time.
#[cfg(target_os = "windows")]
pub fn session_process_ids(session_id: &str) -> Option<std::collections::HashSet<u32>> {
    windows_impl::session_process_ids(session_id)
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use core::ffi::c_void;
    use std::collections::{HashMap, HashSet};
    use std::mem::size_of;
    use std::sync::Mutex;

    use once_cell::sync::{Lazy, OnceCell};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// Ceiling on how many job members we will ask for, so a pathological
    /// process tree can't make us allocate without bound.
    const MAX_TRACKED_PROCESSES: usize = 4096;

    /// Wrapper that lets us cache a raw `HANDLE` in a `static`.
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
    ///
    /// The handle is owned by the supervisor process for its entire lifetime
    /// and is never closed explicitly: it is closed by the OS when the process
    /// exits, which is precisely the event that triggers
    /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    static SUPERVISOR_JOB: OnceCell<Option<JobHandle>> = OnceCell::new();

    /// Per-session job objects, keyed by session ID. Keying by session rather
    /// than PID means a restart replaces the entry instead of stranding the
    /// job of the process it just replaced.
    static SESSION_JOBS: Lazy<Mutex<HashMap<String, JobHandle>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    fn supervisor_job() -> Option<HANDLE> {
        SUPERVISOR_JOB
            .get_or_init(|| create_kill_on_close_job().map(JobHandle))
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

    #[allow(unsafe_code)]
    pub fn assign_to_jobs(session_id: &str, child: &tokio::process::Child) {
        let raw = match child.raw_handle() {
            Some(handle) => handle,
            None => {
                log::warn!("Kernel process has no handle; cannot assign to job object");
                return;
            }
        };

        // `raw` is already a `*mut c_void` (the Windows process handle).
        let process = HANDLE(raw);

        // The supervisor job has to come first: assigning a process that is
        // already in a job nests the second job beneath the first, and we want
        // the per-session job to be the nested one.
        if let Some(job) = supervisor_job() {
            // SAFETY: `job` is a valid job object handle owned by this process
            // and `process` is the live handle to the just-spawned child.
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

        if let Some(job) = create_session_job(process) {
            // A restart hands us a new process for a session we already track;
            // the job the old process was in is finished with.
            if let Some(previous) = SESSION_JOBS
                .lock()
                .unwrap()
                .insert(session_id.to_string(), JobHandle(job))
            {
                // SAFETY: we own the replaced handle and nothing else holds it.
                unsafe {
                    let _ = CloseHandle(previous.0);
                }
            }
        }
    }

    /// Create the accounting job for one kernel and put the kernel in it.
    #[allow(unsafe_code)]
    fn create_session_job(process: HANDLE) -> Option<HANDLE> {
        // SAFETY: an anonymous job object with no limits set; on failure we
        // close any handle we obtained.
        unsafe {
            let job = match CreateJobObjectW(None, windows::core::PCWSTR::null()) {
                Ok(handle) => handle,
                Err(e) => {
                    log::warn!(
                        "Failed to create per-session job object; child processes will not \
                         be counted towards this session's resource usage: {}",
                        e
                    );
                    return None;
                }
            };

            if let Err(e) = AssignProcessToJobObject(job, process) {
                // Nesting needs Windows 8 or later, and fails if an outer job
                // forbids it. Accounting degrades to the kernel process alone.
                log::warn!(
                    "Failed to assign kernel process to its per-session job object; child \
                     processes will not be counted towards its resource usage: {}",
                    e
                );
                let _ = CloseHandle(job);
                return None;
            }

            Some(job)
        }
    }

    #[allow(unsafe_code)]
    pub fn release_session_job(session_id: &str) {
        let Some(job) = SESSION_JOBS.lock().unwrap().remove(session_id) else {
            return;
        };
        // SAFETY: the handle came from CreateJobObjectW and we just took sole
        // ownership of it. The job sets no limits, so closing it terminates
        // nothing.
        unsafe {
            let _ = CloseHandle(job.0);
        }
    }

    pub fn session_process_ids(session_id: &str) -> Option<HashSet<u32>> {
        let jobs = SESSION_JOBS.lock().unwrap();
        let job = jobs.get(session_id)?.0;
        query_process_ids(job)
    }

    #[allow(unsafe_code)]
    fn query_process_ids(job: HANDLE) -> Option<HashSet<u32>> {
        let mut capacity = 32usize;

        loop {
            // A Vec<usize> gives us the alignment the ULONG_PTR id list needs.
            let bytes =
                size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() + capacity * size_of::<usize>();
            let slots = bytes.div_ceil(size_of::<usize>());
            let mut buffer: Vec<usize> = vec![0; slots];
            let byte_len = slots * size_of::<usize>();

            // SAFETY: the buffer is `byte_len` bytes and aligned for the list
            // struct, which is what we tell the call it has to work with.
            let result = unsafe {
                QueryInformationJobObject(
                    job,
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr() as *mut c_void,
                    byte_len as u32,
                    None,
                )
            };

            // SAFETY: the buffer is at least as large as the struct and is
            // zeroed, so this is a valid read whether or not the call
            // succeeded.
            let list = unsafe { &*(buffer.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST) };

            if result.is_err() {
                // ERROR_MORE_DATA leaves the counts filled in even though the
                // list itself didn't fit, so we can size the retry exactly.
                let assigned = list.NumberOfAssignedProcesses as usize;
                if assigned > capacity && capacity < MAX_TRACKED_PROCESSES {
                    capacity = assigned.min(MAX_TRACKED_PROCESSES);
                    continue;
                }
                return None;
            }

            let count = (list.NumberOfProcessIdsInList as usize).min(capacity);
            // SAFETY: the call reported `count` entries written into the list.
            let pids = unsafe { std::slice::from_raw_parts(list.ProcessIdList.as_ptr(), count) };
            return Some(pids.iter().map(|&pid| pid as u32).collect());
        }
    }
}
