// Nonblocking Unix sockets restricted to the effective user. Linux x86-64 raw syscalls.
// New code; socket paths never accept external directory or filename input.

use alloc::{format, string::String};
use rt::os::syscall6;

const EAGAIN: isize = -11;

/// Private local listener. Dropping it closes and removes only its own socket path.
pub struct Listener {
    fd: i32,
    path: String,
}

/// One nonblocking authenticated local connection.
pub struct Stream {
    fd: i32,
}

impl Listener {
    /// Bind below a verified, non-symlink, mode-0700 directory owned by the effective UID.
    pub fn bind(app: &str) -> Result<Self, i32> {
        if app.is_empty()
            || app.len() > 32
            || !app
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(22);
        }
        // SAFETY: geteuid/getpid take no pointers or process-modifying arguments.
        let (uid, pid) = unsafe {
            (
                syscall6(107, 0, 0, 0, 0, 0, 0),
                syscall6(39, 0, 0, 0, 0, 0, 0),
            )
        };
        let directory = format!("/tmp/jfgui-agent-{uid}");
        let dir_z = format!("{directory}\0");
        // SAFETY: mkdir/openat receive a valid terminated path; flags forbid following a symlink.
        let dir_fd = unsafe {
            let mkdir = syscall6(83, dir_z.as_ptr() as usize, 0o700, 0, 0, 0, 0);
            if mkdir < 0 && mkdir != -17 {
                return Err((-mkdir) as i32);
            }
            syscall6(
                257,
                -100isize as usize,
                dir_z.as_ptr() as usize,
                0x90000 | 0x20000,
                0,
                0,
                0,
            )
        };
        if dir_fd < 0 {
            return Err((-dir_fd) as i32);
        }
        let mut stat = [0u8; 144];
        // SAFETY: x86-64 fstat writes exactly the 144-byte kernel stat record to this buffer.
        let status =
            unsafe { syscall6(5, dir_fd as usize, stat.as_mut_ptr() as usize, 0, 0, 0, 0) };
        close(dir_fd as i32);
        if status < 0 {
            return Err((-status) as i32);
        }
        let mode = u32::from_ne_bytes(stat[24..28].try_into().unwrap_or([0; 4]));
        let owner = u32::from_ne_bytes(stat[28..32].try_into().unwrap_or([0; 4]));
        if owner != uid as u32 || mode & 0o170777 != 0o040700 {
            return Err(13);
        }
        let path = format!("{directory}/{app}-{pid}-{}.sock", crate::time_us());
        if path.len() >= 108 {
            return Err(36);
        }
        let mut address = [0u8; 110];
        address[..2].copy_from_slice(&1u16.to_ne_bytes());
        address[2..2 + path.len()].copy_from_slice(path.as_bytes());
        // SAFETY: socket has no borrowed memory and requests CLOEXEC/nonblocking from creation.
        let fd = unsafe { syscall6(41, 1, 1 | 0x80000 | 0x800, 0, 0, 0, 0) };
        if fd < 0 {
            return Err((-fd) as i32);
        }
        // SAFETY: sockaddr_un is bounded and includes its terminating zero byte.
        let bound = unsafe {
            syscall6(
                49,
                fd as usize,
                address.as_ptr() as usize,
                path.len() + 3,
                0,
                0,
                0,
            )
        };
        if bound < 0 {
            close(fd as i32);
            return Err((-bound) as i32);
        }
        let path_z = format!("{path}\0");
        // SAFETY: the socket lives inside the checked private directory; chmod/listen use owned path/fd.
        let status = unsafe {
            let mode = syscall6(90, path_z.as_ptr() as usize, 0o600, 0, 0, 0, 0);
            if mode < 0 {
                mode
            } else {
                syscall6(50, fd as usize, 8, 0, 0, 0, 0)
            }
        };
        if status < 0 {
            close(fd as i32);
            unlink(&path);
            return Err((-status) as i32);
        }
        Ok(Self {
            fd: fd as i32,
            path,
        })
    }
    /// Address to advertise only while control is active.
    pub fn endpoint(&self) -> &str {
        &self.path
    }
    /// Accept at most one available connection and validate its effective UID via SO_PEERCRED.
    pub fn accept(&mut self) -> Result<Option<Stream>, i32> {
        // SAFETY: accept4 is nonblocking and requests no peer address output.
        let fd = unsafe { syscall6(288, self.fd as usize, 0, 0, 0x80000 | 0x800, 0, 0) };
        if fd == EAGAIN || fd == -4 {
            return Ok(None);
        }
        if fd < 0 {
            return Err((-fd) as i32);
        }
        let mut credentials = [0u32; 3];
        let mut length = 12u32;
        // SAFETY: SO_PEERCRED writes at most the supplied 12-byte ucred record and length.
        let status = unsafe {
            syscall6(
                55,
                fd as usize,
                1,
                17,
                credentials.as_mut_ptr() as usize,
                &mut length as *mut u32 as usize,
                0,
            )
        };
        // SAFETY: geteuid has no arguments.
        let uid = unsafe { syscall6(107, 0, 0, 0, 0, 0, 0) };
        if status < 0 || length != 12 || credentials[1] != uid as u32 {
            close(fd as i32);
            return Err(13);
        }
        Ok(Some(Stream { fd: fd as i32 }))
    }
}

impl Stream {
    /// Read available bytes. None means would block; Some(0) means peer EOF.
    pub fn read(&mut self, bytes: &mut [u8]) -> Result<Option<usize>, i32> {
        // SAFETY: read receives the exclusive writable range of the caller's slice.
        result(unsafe {
            syscall6(
                0,
                self.fd as usize,
                bytes.as_mut_ptr() as usize,
                bytes.len(),
                0,
                0,
                0,
            )
        })
    }
    /// Write available bytes without SIGPIPE. None means would block.
    pub fn write(&mut self, bytes: &[u8]) -> Result<Option<usize>, i32> {
        // SAFETY: sendto reads a valid slice, uses no destination for a connected socket, and disables SIGPIPE.
        result(unsafe {
            syscall6(
                44,
                self.fd as usize,
                bytes.as_ptr() as usize,
                bytes.len(),
                0x4000,
                0,
                0,
            )
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        close(self.fd);
        unlink(&self.path);
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        close(self.fd);
    }
}

fn result(value: isize) -> Result<Option<usize>, i32> {
    if value == EAGAIN || value == -4 {
        Ok(None)
    } else if value < 0 {
        Err((-value) as i32)
    } else {
        Ok(Some(value as usize))
    }
}
fn close(fd: i32) {
    // SAFETY: this module calls close once for each owned descriptor.
    unsafe {
        syscall6(3, fd as usize, 0, 0, 0, 0, 0);
    }
}
fn unlink(path: &str) {
    let path = format!("{path}\0");
    // SAFETY: the path was created by this listener and contains no external path components.
    unsafe {
        syscall6(87, path.as_ptr() as usize, 0, 0, 0, 0, 0);
    }
}
