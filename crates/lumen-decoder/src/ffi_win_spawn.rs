//! Starting a worker on Windows inside an `AppContainer` and a job object.
//!
//! Windows applies this kind of sandbox when a process is created, so the host
//! does it here and the worker only checks, in [`crate::ffi_sandbox`], that it
//! is inside it. The `AppContainer` holds no capability, which denies the network
//! and every file the container was not granted. The job object allows one
//! process, so the worker cannot start another, and carries the memory and CPU
//! limits. Closing the job kills the worker and anything left in it.
#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "Win32 sizes and codes: every value cast here is a small constant or a DWORD error code"
)]

use std::ffi::{OsStr, c_void};
use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, LocalFree, SetHandleInformation, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT,
    SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, FreeSid, PSECURITY_DESCRIPTOR, PSID, SECURITY_CAPABILITIES,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_GENERIC_EXECUTE, FILE_GENERIC_READ};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOB_OBJECT_LIMIT_PROCESS_TIME, JOBOBJECT_BASIC_UI_RESTRICTIONS,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicUIRestrictions,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW, UpdateProcThreadAttribute,
    WaitForSingleObject,
};

use crate::limits::{CPU_SECONDS, MEMORY_BYTES};

/// The `AppContainer` the workers run in. It is derived from this name, so every
/// host process shares one container and one security identifier.
const CONTAINER_NAME: &str = "lumen.decoder.worker";

/// Every user-interface restriction a job can set (`JOB_OBJECT_UILIMIT_ALL`).
const JOB_OBJECT_UILIMIT_ALL: u32 = 0xFF;

/// Names the step that failed, so a failed start says which part of the sandbox
/// could not be set up.
fn at(step: &str, e: io::Error) -> io::Error {
    io::Error::new(e.kind(), format!("{step}: {e}"))
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

/// A security identifier from `DeriveAppContainerSidFromAppContainerName`.
struct Sid(PSID);

impl Drop for Sid {
    fn drop(&mut self) {
        // SAFETY: the pointer came from the derive call and is freed once.
        unsafe { FreeSid(self.0) };
    }
}

fn container_sid() -> io::Result<Sid> {
    /// `HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)`: the profile exists already.
    const ALREADY_EXISTS: i32 = 0x8007_00B7_u32 as i32;
    static PROFILE: OnceLock<i32> = OnceLock::new();
    let name = wide(OsStr::new(CONTAINER_NAME));
    let created = *PROFILE.get_or_init(|| {
        let mut sid: PSID = null_mut();
        // SAFETY: `name` is NUL-terminated and outlives the call; the display
        // name and description reuse it; no capabilities are passed.
        let hr = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                null(),
                0,
                &raw mut sid,
            )
        };
        if hr == 0 {
            // SAFETY: a successful create returns a SID this caller owns.
            unsafe { FreeSid(sid) };
        }
        hr
    });
    if created != 0 && created != ALREADY_EXISTS {
        return Err(at("create profile", io::Error::from_raw_os_error(created)));
    }
    let mut sid: PSID = null_mut();
    // SAFETY: `name` is NUL-terminated; `sid` is a valid out pointer.
    let hr = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &raw mut sid) };
    if hr == 0 {
        Ok(Sid(sid))
    } else {
        Err(io::Error::from_raw_os_error(hr))
    }
}

/// Lets the container read and run the worker executable. Without this the
/// container cannot open the file it is started from.
fn grant_read_execute(path: &Path, sid: &Sid) -> io::Result<()> {
    let mut name = wide(path.as_os_str());
    let mut old: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `name` is NUL-terminated; the out pointers are valid. The
    // descriptor owns the DACL and is freed below.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &raw mut old,
            null_mut(),
            &raw mut descriptor,
        )
    };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    // SAFETY: an all-zero EXPLICIT_ACCESS_W is a valid starting value.
    let mut access: EXPLICIT_ACCESS_W = unsafe { zeroed() };
    access.grfAccessPermissions = FILE_GENERIC_READ | FILE_GENERIC_EXECUTE;
    access.grfAccessMode = GRANT_ACCESS;
    access.grfInheritance = 0; // NO_INHERITANCE
    access.Trustee = TRUSTEE_W {
        pMultipleTrustee: null_mut(),
        MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
        TrusteeForm: TRUSTEE_IS_SID,
        TrusteeType: TRUSTEE_IS_UNKNOWN,
        ptstrName: sid.0.cast(),
    };
    let mut merged: *mut ACL = null_mut();
    // SAFETY: one entry, valid for the call; `old` is the DACL read above.
    let rc = unsafe { SetEntriesInAclW(1, &raw const access, old, &raw mut merged) };
    let result = if rc == 0 {
        // SAFETY: `name` is NUL-terminated and `merged` a valid DACL.
        let rc = unsafe {
            SetNamedSecurityInfoW(
                name.as_mut_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                merged,
                null(),
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(rc as i32))
        }
    } else {
        Err(io::Error::from_raw_os_error(rc as i32))
    };
    // SAFETY: both blocks were allocated by the calls above and are not used
    // again; freeing a null pointer is harmless.
    unsafe {
        LocalFree(merged.cast());
        LocalFree(descriptor.cast());
    }
    result
}

/// A job that allows one process, caps its memory and CPU time, removes the
/// user-interface rights and kills what is left when the handle closes.
fn make_job() -> io::Result<OwnedHandle> {
    // SAFETY: no attributes and no name.
    let raw = unsafe { CreateJobObjectW(null(), null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a fresh handle this function owns.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    // SAFETY: all-zero is a valid value for this plain C struct.
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_PROCESS_TIME
        | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    // In units of 100 ns.
    limits.BasicLimitInformation.PerProcessUserTimeLimit = (CPU_SECONDS * 10_000_000) as i64;
    limits.ProcessMemoryLimit = MEMORY_BYTES as usize;
    // SAFETY: `limits` is valid for its size for the call.
    let ok = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: JOB_OBJECT_UILIMIT_ALL,
    };
    // SAFETY: `ui` is valid for its size for the call.
    let ok = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectBasicUIRestrictions,
            (&raw const ui).cast::<c_void>(),
            size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

/// One pipe: the end the worker gets (inheritable) and the end the host keeps.
fn pipe(child_reads: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
    // SAFETY: valid out pointers; default security and buffer size.
    if unsafe { CreatePipe(&raw mut read, &raw mut write, null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both handles are fresh and owned here.
    let (read, write) = unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    };
    let (child, host) = if child_reads {
        (read, write)
    } else {
        (write, read)
    };
    // SAFETY: the handle is valid; only the inherit flag changes.
    if unsafe {
        SetHandleInformation(
            child.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((child, host))
}

/// The quoted command line `CreateProcessW` expects.
fn command_line(worker: &Path, args: &[String]) -> Vec<u16> {
    fn quote(s: &OsStr, out: &mut Vec<u16>) {
        out.push(u16::from(b'"'));
        let mut backslashes = 0usize;
        for c in s.encode_wide() {
            if c == u16::from(b'\\') {
                backslashes += 1;
                out.push(c);
                continue;
            }
            if c == u16::from(b'"') {
                out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes + 1));
            }
            backslashes = 0;
            out.push(c);
        }
        out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
        out.push(u16::from(b'"'));
    }
    let mut line = Vec::new();
    quote(worker.as_os_str(), &mut line);
    for a in args {
        line.push(u16::from(b' '));
        quote(OsStr::new(a), &mut line);
    }
    line.push(0);
    line
}

/// A running worker. Dropping it closes the job, which kills the worker.
pub struct Worker {
    process: OwnedHandle,
    job: OwnedHandle,
}

/// A started worker and the host's ends of its three pipes.
pub struct Spawned {
    /// The worker.
    pub worker: Worker,
    /// Writes to the worker's stdin.
    pub stdin: File,
    /// Reads the worker's stdout.
    pub stdout: File,
    /// Reads the worker's stderr.
    pub stderr: File,
}

/// Starts `worker` with `args` in the container and the job, with an empty
/// environment and only its three standard handles inherited.
///
/// # Errors
/// If the container, the job, the pipes or the process cannot be set up. There
/// is no fallback to an unsandboxed start.
pub fn spawn(worker: &Path, args: &[String]) -> io::Result<Spawned> {
    let sid = container_sid().map_err(|e| at("container", e))?;
    grant_read_execute(worker, &sid).map_err(|e| at("grant", e))?;
    let job = make_job().map_err(|e| at("job", e))?;
    let (child_in, host_in) = pipe(true).map_err(|e| at("pipe", e))?;
    let (child_out, host_out) = pipe(false).map_err(|e| at("pipe", e))?;
    let (child_err, host_err) = pipe(false).map_err(|e| at("pipe", e))?;

    let mut capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: sid.0,
        Capabilities: null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let job_handle: HANDLE = job.as_raw_handle();
    let mut inherited: [HANDLE; 3] = [
        child_in.as_raw_handle(),
        child_out.as_raw_handle(),
        child_err.as_raw_handle(),
    ];

    let mut size = 0usize;
    // SAFETY: the sizing call takes a null list; it fails by design and sets `size`.
    unsafe { InitializeProcThreadAttributeList(null_mut(), 3, 0, &raw mut size) };
    // A usize buffer keeps the list pointer-aligned.
    let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
    let list: LPPROC_THREAD_ATTRIBUTE_LIST = storage.as_mut_ptr().cast();
    // SAFETY: `list` points at `size` writable bytes.
    if unsafe { InitializeProcThreadAttributeList(list, 3, 0, &raw mut size) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        // SAFETY: each value outlives `CreateProcessW` below and has the size given.
        let ok = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                (&raw mut capabilities).cast::<c_void>(),
                size_of::<SECURITY_CAPABILITIES>(),
                null_mut(),
                null(),
            ) != 0
                && UpdateProcThreadAttribute(
                    list,
                    0,
                    PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                    (&raw const job_handle).cast::<c_void>(),
                    size_of::<HANDLE>(),
                    null_mut(),
                    null(),
                ) != 0
                && UpdateProcThreadAttribute(
                    list,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    inherited.as_mut_ptr().cast::<c_void>(),
                    size_of::<[HANDLE; 3]>(),
                    null_mut(),
                    null(),
                ) != 0
        };
        if !ok {
            return Err(at("attributes", io::Error::last_os_error()));
        }
        // SAFETY: all-zero is valid for these plain C structs.
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = child_in.as_raw_handle();
        startup.StartupInfo.hStdOutput = child_out.as_raw_handle();
        startup.StartupInfo.hStdError = child_err.as_raw_handle();
        startup.lpAttributeList = list;
        // SAFETY: as above.
        let mut info: PROCESS_INFORMATION = unsafe { zeroed() };

        let app = wide(worker.as_os_str());
        let mut line = command_line(worker, args);
        // The one variable the loader and the system libraries expect; nothing of
        // the host's own environment reaches the worker.
        let root = std::env::var_os("SystemRoot");
        let mut environment: Vec<u16> = Vec::new();
        if let Some(r) = &root {
            environment.extend(OsStr::new("SystemRoot=").encode_wide());
            environment.extend(r.encode_wide());
            environment.push(0);
        }
        environment.push(0);
        if root.is_none() {
            environment.push(0);
        }
        let directory = root.as_ref().map(|d| wide(d));
        // SAFETY: every pointer is to a live, NUL-terminated buffer or null;
        // `startup` carries the attribute list built above.
        let ok = unsafe {
            CreateProcessW(
                app.as_ptr(),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                environment.as_ptr().cast::<c_void>(),
                directory.as_ref().map_or(null(), Vec::as_ptr),
                &raw const startup.StartupInfo,
                &raw mut info,
            )
        };
        if ok == 0 {
            return Err(at("create process", io::Error::last_os_error()));
        }
        // SAFETY: the thread handle is not needed; the process handle is kept.
        unsafe { CloseHandle(info.hThread) };
        // SAFETY: a fresh handle this function owns.
        Ok(unsafe { OwnedHandle::from_raw_handle(info.hProcess) })
    })();
    // SAFETY: the list was initialised above and is deleted once.
    unsafe { DeleteProcThreadAttributeList(list) };
    let process = result?;
    drop((child_in, child_out, child_err));
    Ok(Spawned {
        worker: Worker { process, job },
        stdin: File::from(host_in),
        stdout: File::from(host_out),
        stderr: File::from(host_err),
    })
}

impl Worker {
    /// `Some(true)` if it exited with status 0, `Some(false)` if it exited
    /// otherwise, `None` while it runs.
    ///
    /// # Errors
    /// If the process cannot be queried.
    pub fn try_wait(&self) -> io::Result<Option<bool>> {
        // SAFETY: a valid process handle; a zero timeout only polls.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0u32;
                // SAFETY: a valid handle and out pointer.
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &raw mut code) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(code == 0))
            }
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// Kills every process in the job. Already finished is not an error.
    pub fn kill(&self) {
        // SAFETY: a valid job handle. Terminating an empty job only fails.
        unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) };
    }
}
