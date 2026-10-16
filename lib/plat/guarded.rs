// guarded -- the public file functions: a name with a scheme goes to its vfs provider
// (vfs.rs), any other name is checked against the app's permissions (policy.rs) and then
// handed to the OS call (win_sys.rs, linux_sys.rs, web_files.rs: the *_raw functions, which
// the platform layer uses for its own system files)
//
// A refused path looks like a missing or unreadable one: 0 / None / false.  The streaming
// File / FileLock calls work on OS paths and on folder mounts (config://, data://, cache://,
// DirFs); for the other providers they return None.

use alloc::vec::Vec;

use crate::policy::{fs_allowed, FS_READ, FS_WRITE};
use crate::vfs::{self, Route};
use crate::{File, FileLock, FO_READ};

/// 0 missing, 1 file, 2 directory  [plat_file_exists]
pub fn file_exists(path: &str) -> u32 {
    match vfs::route(path) {
        Route::Os => if fs_allowed(path, FS_READ) { crate::file_exists_raw(path) } else { 0 },
        Route::Vfs(p, rest) => p.kind(&rest),
        Route::Bad => 0,
    }
}

/// Some(0) only for a missing path, Some(1/2) for file/directory, None for other errors.
pub fn file_exists_checked(path: &str) -> Option<u32> {
    match vfs::route(path) {
        Route::Os => if fs_allowed(path, FS_READ) { crate::file_exists_checked_raw(path) } else { None },
        Route::Vfs(p, rest) => Some(p.kind(&rest)),
        Route::Bad => None,
    }
}

/// create a directory and its missing parents; an existing directory is fine  [plat_mkdir]
pub fn mkdir(path: &str) -> bool {
    match vfs::route(path) {
        Route::Os => fs_allowed(path, FS_WRITE) && crate::mkdir_raw(path),
        Route::Vfs(p, rest) => p.mkdir(&rest),
        Route::Bad => false,
    }
}

/// a file or an empty directory  [plat_delete]
pub fn delete(path: &str) -> bool {
    match vfs::route(path) {
        Route::Os => fs_allowed(path, FS_WRITE) && crate::delete_raw(path),
        Route::Vfs(p, rest) => p.delete(&rest),
        Route::Bad => false,
    }
}

/// rename, replacing an existing target (both names on the OS or in the same mount)  [plat_rename]
pub fn rename(old: &str, new: &str) -> bool {
    match (vfs::route(old), vfs::route(new)) {
        (Route::Os, Route::Os) => fs_allowed(old, FS_WRITE) && fs_allowed(new, FS_WRITE) && crate::rename_raw(old, new),
        (Route::Vfs(p, a), Route::Vfs(q, b)) if alloc::sync::Arc::ptr_eq(&p, &q) => p.rename(&a, &b),
        _ => false,
    }
}

/// f(name, is_dir, size_lo) for every entry except "." and ".."  [plat_dir_list]
pub fn dir_list(path: &str, f: &mut dyn FnMut(&str, bool, u32)) -> bool {
    match vfs::route(path) {
        Route::Os => fs_allowed(path, FS_READ) && crate::dir_list_raw(path, f),
        Route::Vfs(p, rest) => p.list(&rest, &mut |n, d, s| f(n, d, s as u32)),
        Route::Bad => false,
    }
}

/// the contents of a file  [plat_file_read]
pub fn read_file(path: &str) -> Option<Vec<u8>> {
    match vfs::route(path) {
        Route::Os => if fs_allowed(path, FS_READ) { crate::read_file_raw(path) } else { None },
        Route::Vfs(p, rest) => p.read(&rest),
        Route::Bad => None,
    }
}

/// Write an exclusively created sibling, flush it, then replace path (a mount: its own way of
/// replacing a file; missing folders are created). [plat_file_write]
pub fn write_file(path: &str, data: &[u8]) -> bool {
    match vfs::route(path) {
        Route::Os => fs_allowed(path, FS_WRITE) && crate::write_file_raw(path, data),
        Route::Vfs(p, rest) => !rest.is_empty() && p.write(&rest, data),
        Route::Bad => false,
    }
}

// the OS path of a name: itself, or the file behind a folder mount
fn os(path: &str) -> Option<alloc::string::String> {
    match vfs::route(path) {
        Route::Os => Some(alloc::string::String::from(path)),
        Route::Vfs(p, rest) => p.os_path(&rest),
        Route::Bad => None,
    }
}

impl File {
    /// FO_READ (existing, read only), FO_WRITE (create / truncate), FO_RW (open or create)
    pub fn open(path: &str, mode: u32) -> Option<File> {
        let path = os(path)?;
        if fs_allowed(&path, if mode == FO_READ { FS_READ } else { FS_WRITE }) { File::open_raw(&path, mode) } else { None }
    }

    /// Create a named file exclusively; never open an existing path.
    #[cfg(any(windows, target_os = "linux"))]
    pub fn create_new(path: &str) -> Option<File> {
        let path = os(path)?;
        if fs_allowed(&path, FS_WRITE) { File::create_new_raw(&path) } else { None }
    }

    /// a new file without a lasting name in dir (None = temp_dir) that disappears when it is
    /// closed (also after a crash)  [plat_file_open_temp]
    pub fn open_temp(dir: Option<&str>) -> Option<File> {
        match dir {
            None => File::open_temp_raw(None),
            Some(d) => {
                let d = os(d)?;
                if fs_allowed(&d, FS_WRITE) { File::open_temp_raw(Some(&d)) } else { None }
            }
        }
    }
}

impl FileLock {
    /// Open or create without truncating. None means busy, invalid path, refused or I/O failure.
    pub fn acquire(path: &str) -> Option<Self> {
        let path = os(path)?;
        if fs_allowed(&path, FS_WRITE) { FileLock::acquire_raw(&path) } else { None }
    }
}
