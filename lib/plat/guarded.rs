// guarded -- the public file functions: the check of the app's permissions (policy.rs), then
// the OS call (win_sys.rs, linux_sys.rs, web_files.rs: the *_raw functions, which the
// platform layer uses for its own system files)
//
// A refused path looks like a missing or unreadable one: 0 / None / false.

use alloc::vec::Vec;

use crate::policy::{fs_allowed, FS_READ, FS_WRITE};
use crate::{File, FileLock, FO_READ};

/// 0 missing, 1 file, 2 directory  [plat_file_exists]
pub fn file_exists(path: &str) -> u32 {
    if fs_allowed(path, FS_READ) { crate::file_exists_raw(path) } else { 0 }
}

/// Some(0) only for a missing path, Some(1/2) for file/directory, None for other errors.
pub fn file_exists_checked(path: &str) -> Option<u32> {
    if fs_allowed(path, FS_READ) { crate::file_exists_checked_raw(path) } else { None }
}

/// create a directory and its missing parents; an existing directory is fine  [plat_mkdir]
pub fn mkdir(path: &str) -> bool {
    fs_allowed(path, FS_WRITE) && crate::mkdir_raw(path)
}

/// a file or an empty directory  [plat_delete]
pub fn delete(path: &str) -> bool {
    fs_allowed(path, FS_WRITE) && crate::delete_raw(path)
}

/// rename, replacing an existing target  [plat_rename]
pub fn rename(old: &str, new: &str) -> bool {
    fs_allowed(old, FS_WRITE) && fs_allowed(new, FS_WRITE) && crate::rename_raw(old, new)
}

/// f(name, is_dir, size_lo) for every entry except "." and ".."  [plat_dir_list]
pub fn dir_list(path: &str, f: &mut dyn FnMut(&str, bool, u32)) -> bool {
    fs_allowed(path, FS_READ) && crate::dir_list_raw(path, f)
}

/// the contents of a file  [plat_file_read]
pub fn read_file(path: &str) -> Option<Vec<u8>> {
    if fs_allowed(path, FS_READ) { crate::read_file_raw(path) } else { None }
}

/// Write an exclusively created sibling, flush it, then replace path. [plat_file_write]
pub fn write_file(path: &str, data: &[u8]) -> bool {
    fs_allowed(path, FS_WRITE) && crate::write_file_raw(path, data)
}

impl File {
    /// FO_READ (existing, read only), FO_WRITE (create / truncate), FO_RW (open or create)
    pub fn open(path: &str, mode: u32) -> Option<File> {
        if fs_allowed(path, if mode == FO_READ { FS_READ } else { FS_WRITE }) { File::open_raw(path, mode) } else { None }
    }

    /// Create a named file exclusively; never open an existing path.
    #[cfg(any(windows, target_os = "linux"))]
    pub fn create_new(path: &str) -> Option<File> {
        if fs_allowed(path, FS_WRITE) { File::create_new_raw(path) } else { None }
    }

    /// a new file without a lasting name in dir (None = temp_dir) that disappears when it is
    /// closed (also after a crash)  [plat_file_open_temp]
    pub fn open_temp(dir: Option<&str>) -> Option<File> {
        if dir.is_none_or(|d| fs_allowed(d, FS_WRITE)) { File::open_temp_raw(dir) } else { None }
    }
}

impl FileLock {
    /// Open or create without truncating. None means busy, invalid path, refused or I/O failure.
    pub fn acquire(path: &str) -> Option<Self> {
        if fs_allowed(path, FS_WRITE) { FileLock::acquire_raw(path) } else { None }
    }
}
