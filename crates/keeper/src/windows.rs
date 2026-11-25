use anyhow::{Context, Result, bail};
use std::io::{self, Error};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SetFileSecurityW};
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::Console::{GetStdHandle, STD_INPUT_HANDLE};
use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject};

struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn new(sddl: &str) -> io::Result<Self> {
        let sddl: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
        let mut descriptor = null_mut();
        check(unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, null_mut()) })?;

        Ok(Self(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

fn check(success: i32) -> io::Result<()> {
    if success == 0 {
        return Err(Error::last_os_error());
    }

    Ok(())
}

pub fn secure_path(path: &Path, directory: bool) -> io::Result<()> {
    let descriptor = Descriptor::new(if directory { "D:P(A;OICI;GA;;;OW)(A;OICI;GA;;;SY)" } else { "D:P(A;;GA;;;OW)(A;;GA;;;SY)" })?;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    check(unsafe { SetFileSecurityW(path.as_ptr(), DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION, descriptor.0) })
}

pub(super) fn pipe_name(id: uuid::Uuid) -> String {
    format!(r"\\.\pipe\mycelium-keeper-{id}")
}

pub(super) fn listen(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let descriptor = Descriptor::new("D:P(A;;GA;;;OW)(A;;GA;;;SY)")?;
    let mut security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };

    unsafe { ServerOptions::new().reject_remote_clients(true).first_pipe_instance(first).create_with_security_attributes_raw(name, (&mut security as *mut SECURITY_ATTRIBUTES).cast()) }
}

pub(super) struct Job(OwnedHandle);

impl Job {
    pub(super) fn new(child: &tokio::process::Child) -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(null(), null()) };
        if handle.is_null() {
            return Err(Error::last_os_error());
        }

        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        check(unsafe { SetInformationJobObject(job.0.as_raw_handle(), JobObjectExtendedLimitInformation, (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&limits) as u32) })?;
        let child = child.raw_handle().ok_or_else(|| Error::other("The keeper child already exited."))?;
        check(unsafe { AssignProcessToJobObject(job.0.as_raw_handle(), child) })?;

        Ok(job)
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        check(unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) })
    }
}

pub fn resolve_program(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let candidates = if path.components().count() > 1 || path.has_root() {
        vec![path.to_path_buf()]
    } else {
        std::env::split_paths(&std::env::var_os("PATH")?).map(|directory| directory.join(path)).collect()
    };

    for candidate in candidates {
        if let Some(extension) = candidate.extension() {
            if extensions.split(';').any(|entry| entry.trim_start_matches('.').eq_ignore_ascii_case(&extension.to_string_lossy())) && candidate.is_file() {
                return std::path::absolute(candidate).ok();
            }
        } else {
            for extension in extensions.split(';').filter_map(|entry| entry.strip_prefix('.')) {
                let path = candidate.with_extension(extension);
                if path.is_file() {
                    return std::path::absolute(path).ok();
                }
            }
        }
    }

    None
}

pub fn run_child(command: Vec<String>) -> Result<i32> {
    let Some((program, args)) = command.split_first() else {
        bail!("Give the agent command after --.");
    };

    let mut ready = 0u8;
    let mut received = 0;
    let stdin = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    check(unsafe { ReadFile(stdin, &mut ready, 1, &mut received, null_mut()) })?;
    if received != 1 || ready != 1 {
        bail!("The keeper did not assign the agent to its job.");
    }

    let resolved = resolve_program(program).unwrap_or_else(|| PathBuf::from(program));
    let status = std::process::Command::new(resolved).args(args).status().with_context(|| format!("start {program}"))?;

    Ok(status.code().unwrap_or(-1))
}
