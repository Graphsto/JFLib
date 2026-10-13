// Nonblocking byte-mode named pipes with an explicit current-user DACL and remote denial.
// Security APIs are loaded dynamically from advapi32; no additional static DLL dependency.

use alloc::{format, string::String, vec, vec::Vec};
use core::ffi::c_void;

#[repr(C)]
struct SecurityAttributes {
    length: u32,
    descriptor: *mut c_void,
    inherit: i32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        access: u32,
        mode: u32,
        max: u32,
        out_size: u32,
        in_size: u32,
        timeout: u32,
        security: *const SecurityAttributes,
    ) -> isize;
    fn ConnectNamedPipe(pipe: isize, overlap: *mut c_void) -> i32;
    fn DisconnectNamedPipe(pipe: isize) -> i32;
    fn ReadFile(
        file: isize,
        bytes: *mut u8,
        count: u32,
        read: *mut u32,
        overlap: *mut crate::win::Overlapped,
    ) -> i32;
    fn WriteFile(
        file: isize,
        bytes: *const u8,
        count: u32,
        written: *mut u32,
        overlap: *mut crate::win::Overlapped,
    ) -> i32;
    fn CloseHandle(handle: isize) -> i32;
    fn GetLastError() -> u32;
    fn GetCurrentProcess() -> isize;
    fn GetCurrentProcessId() -> u32;
    fn LoadLibraryExW(path: *const u16, file: isize, flags: u32) -> isize;
    fn FreeLibrary(module: isize) -> i32;
    fn GetProcAddress(module: isize, name: *const u8) -> usize;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

/// User-private listener. Dropping closes the accepting handle; peer handles are separate.
pub struct Listener {
    pipe: isize,
    name: String,
    wide_name: Vec<u16>,
    descriptor: *mut c_void,
}

/// One accepted nonblocking local connection.
pub struct Stream {
    pipe: isize,
}

impl Listener {
    /// Create a uniquely named endpoint with access granted only to the process token's user SID.
    pub fn bind(app: &str) -> Result<Self, i32> {
        if app.is_empty()
            || app.len() > 32
            || !app
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(87);
        }
        // SAFETY: GetCurrentProcessId has no pointers and requires no privilege.
        let pid = unsafe { GetCurrentProcessId() };
        let name = format!("\\\\.\\pipe\\jfgui-agent-{app}-{pid}-{}", crate::time_us());
        let wide_name = wide(&name);
        let descriptor = security_descriptor()?;
        let pipe = create_pipe(&wide_name, descriptor, true);
        if pipe == -1 {
            let error = last_error();
            // SAFETY: descriptor was allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe {
                LocalFree(descriptor);
            }
            return Err(error);
        }
        Ok(Self {
            pipe,
            name,
            wide_name,
            descriptor,
        })
    }
    /// Address to advertise only while control is active.
    pub fn endpoint(&self) -> &str {
        &self.name
    }
    /// Poll once for a client; the next accepting pipe retains exactly the same DACL.
    pub fn accept(&mut self) -> Result<Option<Stream>, i32> {
        // SAFETY: the listener owns this valid nonblocking named-pipe handle; no overlapped pointer is needed.
        let connected = unsafe { ConnectNamedPipe(self.pipe, core::ptr::null_mut()) };
        let error = if connected == 0 { last_error() } else { 535 };
        match error {
            535 => {
                let next = create_pipe(&self.wide_name, self.descriptor, false);
                if next == -1 {
                    return Err(last_error());
                }
                let accepted = core::mem::replace(&mut self.pipe, next);
                Ok(Some(Stream { pipe: accepted }))
            }
            536 => Ok(None),
            232 => {
                // SAFETY: a client disconnected before accept; reset the owned listener instance.
                unsafe {
                    DisconnectNamedPipe(self.pipe);
                }
                Ok(None)
            }
            other => Err(other),
        }
    }
}

impl Stream {
    /// Read available bytes. None means would block; Some(0) means peer EOF.
    pub fn read(&mut self, bytes: &mut [u8]) -> Result<Option<usize>, i32> {
        let mut count = 0;
        // SAFETY: the writable slice stays live for the synchronous, nonblocking operation.
        let ok = unsafe {
            ReadFile(
                self.pipe,
                bytes.as_mut_ptr(),
                bytes.len().min(u32::MAX as usize) as u32,
                &mut count,
                core::ptr::null_mut(),
            )
        };
        if ok != 0 {
            return Ok(Some(count as usize));
        }
        match last_error() {
            232 => Ok(None),
            109 | 233 => Ok(Some(0)),
            error => Err(error),
        }
    }
    /// Write available bytes. None means the pipe buffer currently has no space.
    pub fn write(&mut self, bytes: &[u8]) -> Result<Option<usize>, i32> {
        let mut count = 0;
        // SAFETY: the borrowed input slice stays live for the synchronous, nonblocking operation.
        let ok = unsafe {
            WriteFile(
                self.pipe,
                bytes.as_ptr(),
                bytes.len().min(u32::MAX as usize) as u32,
                &mut count,
                core::ptr::null_mut(),
            )
        };
        if ok != 0 {
            return if count == 0 && !bytes.is_empty() {
                Ok(None)
            } else {
                Ok(Some(count as usize))
            };
        }
        match last_error() {
            232 | 536 => Ok(None),
            109 | 233 => Ok(Some(0)),
            error => Err(error),
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        close(self.pipe);
        // SAFETY: this listener exclusively owns its allocated descriptor.
        unsafe {
            LocalFree(self.descriptor);
        }
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        close(self.pipe);
    }
}

fn create_pipe(name: &[u16], descriptor: *mut c_void, first: bool) -> isize {
    let security = SecurityAttributes {
        length: core::mem::size_of::<SecurityAttributes>() as u32,
        descriptor,
        inherit: 0,
    };
    // SAFETY: name is zero-terminated, the descriptor is live, and the security struct has native ABI layout.
    unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            3 | if first { 0x80000 } else { 0 },
            1 | 8,
            16,
            65_536,
            65_536,
            0,
            &security,
        )
    }
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(core::iter::once(0)).collect()
}
fn last_error() -> i32 {
    // SAFETY: reads the calling thread's most recent Win32 error.
    unsafe { GetLastError() as i32 }
}
fn close(handle: isize) {
    // SAFETY: this helper is called exactly once for each owned handle.
    unsafe {
        CloseHandle(handle);
    }
}

struct Module(isize);
impl Drop for Module {
    fn drop(&mut self) {
        // SAFETY: the module handle belongs to one successful LoadLibrary call.
        unsafe {
            FreeLibrary(self.0);
        }
    }
}

fn security_descriptor() -> Result<*mut c_void, i32> {
    type OpenToken = unsafe extern "system" fn(isize, u32, *mut isize) -> i32;
    type TokenInfo = unsafe extern "system" fn(isize, u32, *mut u8, u32, *mut u32) -> i32;
    type SidText = unsafe extern "system" fn(*const c_void, *mut *mut u16) -> i32;
    type Descriptor = unsafe extern "system" fn(*const u16, u32, *mut *mut c_void, *mut u32) -> i32;
    // SAFETY: the library name is fixed and terminated; Win32 resolves the system library.
    let module = Module(unsafe { LoadLibraryExW(wide("advapi32.dll").as_ptr(), 0, 0x800) });
    if module.0 == 0 {
        return Err(last_error());
    }
    let find = |name: &[u8]| {
        // SAFETY: all names supplied below are static terminated export names.
        unsafe { GetProcAddress(module.0, name.as_ptr()) }
    };
    let pointers = [
        find(b"OpenProcessToken\0"),
        find(b"GetTokenInformation\0"),
        find(b"ConvertSidToStringSidW\0"),
        find(b"ConvertStringSecurityDescriptorToSecurityDescriptorW\0"),
    ];
    if pointers.contains(&0) {
        return Err(127);
    }
    // SAFETY: each non-null address was resolved from the documented export with this exact ABI.
    let (open, info, sid_text, descriptor): (OpenToken, TokenInfo, SidText, Descriptor) = unsafe {
        (
            core::mem::transmute(pointers[0]),
            core::mem::transmute(pointers[1]),
            core::mem::transmute(pointers[2]),
            core::mem::transmute(pointers[3]),
        )
    };
    let mut token = 0;
    // SAFETY: TOKEN_QUERY requests read-only access to this process's own token.
    if unsafe { open(GetCurrentProcess(), 8, &mut token) } == 0 {
        return Err(last_error());
    }
    let mut bytes = 0;
    // SAFETY: the null/zero probe only returns the required TokenUser buffer length.
    unsafe {
        info(token, 1, core::ptr::null_mut(), 0, &mut bytes);
    }
    if bytes < core::mem::size_of::<usize>() as u32 || bytes > 65_536 {
        close(token);
        return Err(13);
    }
    let mut buffer = vec![0u8; bytes as usize];
    // SAFETY: the buffer is exactly the reported size and TokenUser is the requested record.
    let ok = unsafe { info(token, 1, buffer.as_mut_ptr(), bytes, &mut bytes) };
    let error = last_error();
    close(token);
    if ok == 0 {
        return Err(error);
    }
    // SAFETY: TOKEN_USER starts with a SID pointer; read_unaligned makes no buffer alignment assumption.
    let sid = unsafe { core::ptr::read_unaligned(buffer.as_ptr() as *const *const c_void) };
    let mut text = core::ptr::null_mut();
    // SAFETY: the SID pointer refers into the still-live TokenUser buffer; API allocates output text.
    if unsafe { sid_text(sid, &mut text) } == 0 {
        return Err(last_error());
    }
    let mut sid_string = String::new();
    for index in 0..256 {
        // SAFETY: successful SID conversion returns a terminated canonical SID string (well under 256 UTF-16 units).
        let unit = unsafe { *text.add(index) };
        if unit == 0 {
            break;
        }
        if unit > 127 {
            break;
        }
        sid_string.push(unit as u8 as char);
    }
    // SAFETY: the API requires LocalFree for this allocated SID string.
    unsafe {
        LocalFree(text as *mut c_void);
    }
    if !sid_string.starts_with("S-")
        || !sid_string
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S')
    {
        return Err(13);
    }
    let sddl = wide(&format!("D:P(A;;GA;;;{sid_string})"));
    let mut security = core::ptr::null_mut();
    // SAFETY: SDDL consists solely of fixed syntax and validated canonical current-user SID text.
    if unsafe { descriptor(sddl.as_ptr(), 1, &mut security, core::ptr::null_mut()) } == 0 {
        return Err(last_error());
    }
    Ok(security)
}
