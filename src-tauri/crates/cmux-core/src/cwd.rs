//! Best-effort process cwd lookup: what the sidebar shows, and what git
//! branch/PR detection is keyed on. A pane that reports OSC 7 (see the
//! PowerShell shell integration) overrides this — which matters on Windows,
//! where PowerShell's `cd` never moves the process working directory this
//! reads.

#[cfg(target_os = "macos")]
pub fn process_cwd(pid: u32) -> Option<String> {
    macos::pid_cwd(pid)
}

#[cfg(target_os = "linux")]
pub fn process_cwd(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

#[cfg(windows)]
pub fn process_cwd(pid: u32) -> Option<String> {
    windows::pid_cwd(pid)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn process_cwd(_pid: u32) -> Option<String> {
    None
}

/// Windows keeps a process's working directory in its PEB, with no API to
/// read another process's copy — so we read it out of the target's address
/// space, the way Process Explorer does. Same-user processes only (that is
/// all a pane ever holds), and 64-bit only: the field offsets below are the
/// documented 64-bit PEB layout, stable since Windows XP.
#[cfg(windows)]
mod windows {
    use std::os::raw::c_void;

    use windows_sys::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessBasicInformation,
    };
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    /// PEB → RTL_USER_PROCESS_PARAMETERS.
    const PEB_PROCESS_PARAMETERS: usize = 0x20;
    /// RTL_USER_PROCESS_PARAMETERS → CurrentDirectory.DosPath (UNICODE_STRING).
    const PARAMS_CURRENT_DIRECTORY: usize = 0x38;
    /// UNICODE_STRING → Buffer (after Length: u16, MaximumLength: u16, pad).
    const UNICODE_STRING_BUFFER: usize = 0x08;

    struct Process(isize);

    impl Drop for Process {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0 as _) };
        }
    }

    fn read<T: Copy>(process: &Process, address: usize) -> Option<T> {
        let mut value = std::mem::MaybeUninit::<T>::uninit();
        let ok = unsafe {
            ReadProcessMemory(
                process.0 as _,
                address as *const c_void,
                value.as_mut_ptr() as *mut c_void,
                std::mem::size_of::<T>(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then(|| unsafe { value.assume_init() })
    }

    pub fn pid_cwd(pid: u32) -> Option<String> {
        if cfg!(not(target_pointer_width = "64")) {
            return None;
        }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let process = Process(handle as isize);

        // PROCESS_BASIC_INFORMATION: the PEB address is the second pointer.
        let mut info = [0usize; 6];
        let status = unsafe {
            NtQueryInformationProcess(
                process.0 as _,
                ProcessBasicInformation,
                info.as_mut_ptr() as *mut c_void,
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            )
        };
        if status < 0 {
            return None;
        }
        let peb = info[1];
        if peb == 0 {
            return None;
        }

        let params: usize = read(&process, peb + PEB_PROCESS_PARAMETERS)?;
        let cwd = params + PARAMS_CURRENT_DIRECTORY;
        let length: u16 = read(&process, cwd)?;
        let buffer: usize = read(&process, cwd + UNICODE_STRING_BUFFER)?;
        if length == 0 || buffer == 0 {
            return None;
        }

        // `length` is a byte count read out of the target process, so treat
        // it as untrusted: an odd value would make the u16 buffer one byte
        // short of the read below. Round down — UTF-16 has no odd length.
        let bytes = length as usize & !1;
        if bytes == 0 {
            return None;
        }
        let mut utf16 = vec![0u16; bytes / 2];
        let ok = unsafe {
            ReadProcessMemory(
                process.0 as _,
                buffer as *const c_void,
                utf16.as_mut_ptr() as *mut c_void,
                bytes,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&utf16);
        // The PEB copy carries a trailing separator; nothing else expects one.
        let path = path.trim_end_matches('\\');
        (!path.is_empty()).then(|| path.to_string())
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn reads_our_own_working_directory() {
            let expected = std::env::current_dir().unwrap();
            let got = super::pid_cwd(std::process::id()).expect("own cwd");
            assert_eq!(
                got.to_lowercase(),
                expected.to_string_lossy().trim_end_matches('\\').to_lowercase()
            );
        }
    }
}

/// `proc_pidinfo(PROC_PIDVNODEPATHINFO)` via libc. Struct layouts mirror
/// XNU's `sys/proc_info.h` (verified against bindgen output of libproc).
#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_int, c_void};

    const PROC_PIDVNODEPATHINFO: c_int = 9;
    const MAXPATHLEN: usize = 1024;

    #[repr(C)]
    struct VinfoStat {
        vst_dev: u32,
        vst_mode: u16,
        vst_nlink: u16,
        vst_ino: u64,
        vst_uid: u32,
        vst_gid: u32,
        vst_atime: i64,
        vst_atimensec: i64,
        vst_mtime: i64,
        vst_mtimensec: i64,
        vst_ctime: i64,
        vst_ctimensec: i64,
        vst_birthtime: i64,
        vst_birthtimensec: i64,
        vst_size: i64,
        vst_blocks: i64,
        vst_blksize: i32,
        vst_flags: u32,
        vst_gen: u32,
        vst_rdev: u32,
        vst_qspare: [i64; 2],
    }

    #[repr(C)]
    struct VnodeInfo {
        vi_stat: VinfoStat,
        vi_type: c_int,
        vi_pad: c_int,
        vi_fsid: [i32; 2],
    }

    #[repr(C)]
    struct VnodeInfoPath {
        vip_vi: VnodeInfo,
        vip_path: [c_char; MAXPATHLEN],
    }

    #[repr(C)]
    struct ProcVnodePathInfo {
        pvi_cdir: VnodeInfoPath,
        pvi_rdir: VnodeInfoPath,
    }

    extern "C" {
        fn proc_pidinfo(
            pid: c_int,
            flavor: c_int,
            arg: u64,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;
    }

    pub fn pid_cwd(pid: u32) -> Option<String> {
        let mut info: ProcVnodePathInfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<ProcVnodePathInfo>() as c_int;
        let ret = unsafe {
            proc_pidinfo(
                pid as c_int,
                PROC_PIDVNODEPATHINFO,
                0,
                &mut info as *mut _ as *mut c_void,
                size,
            )
        };
        if ret <= 0 {
            return None;
        }
        let path = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr()) };
        let path = path.to_string_lossy();
        if path.is_empty() {
            None
        } else {
            Some(path.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn own_process_cwd_resolves() {
        let cwd = super::process_cwd(std::process::id()).expect("cwd of self");
        assert_eq!(
            std::path::PathBuf::from(cwd),
            std::env::current_dir().unwrap()
        );
    }
}
