// perm -- what build_rs.py builds into an app, set up before it starts, and the app's
// permissions on the UI side (plat::policy, docs/PERMISSIONS.md, docs/VFS.md)
//
//   boot(app, manifest, assets)   (main!) the app's name for its folders (plat::vfs::
//                           set_app_id), its asset bundle as app:// (JFA1, apps/NAME/assets),
//                           then its manifest (apps/NAME/PERMISSIONS); Err(exit code) for a
//                           bundle or manifest it refuses.  The paths on the command line come
//                           from the user: with fs.user they may be read and written (folders
//                           with everything below).
//   grant_user(path)        the same for a path of the file dialog or a drop
//   trusted(f)              f on this thread without file checks: the library's own file
//                           dialog lists folders, scripts read and write their files
//
// The key of plat::policy stays here: apps cannot widen their own permissions.

use alloc::string::String;
use alloc::sync::Arc;

use plat::policy::{self, Key, FS_WRITE};
use plat::vfs;
use rt::sync::SpinLock;

static KEY: SpinLock<Option<Key>> = SpinLock::new(None);

/// (main!) set up the app: its name, its assets (app://), its permissions
#[doc(hidden)]
pub fn boot(app: Option<&str>, manifest: Option<&str>, assets: &'static [u8]) -> Result<(), i32> {
    if let Some(id) = app {
        vfs::set_app_id(id);
    }
    if !assets.is_empty() {
        let Some(b) = vfs::Bundle::from_static(assets) else {
            plat::write_stderr(b"error: the built-in assets (app://) are damaged\n");
            return Err(2);
        };
        vfs::mount("app", Arc::new(b));
    }
    permissions(manifest)
}

// install the app's manifest (None: the app has none, everything stays allowed)
fn permissions(manifest: Option<&str>) -> Result<(), i32> {
    let Some(text) = manifest else { return Ok(()) };
    let key = match policy::install(text) {
        Ok(k) => k,
        Err(e) => {
            plat::write_stderr(alloc::format!("error: {}\n", e).as_bytes());
            return Err(2);
        }
    };
    for a in plat::args().iter().skip(1) {
        // "--name=value": the value; other options are no paths
        let v = match a.strip_prefix("--") {
            Some(o) => match o.split_once('=') {
                Some((_, v)) => v,
                None => continue,
            },
            None => a.as_str(),
        };
        if !v.is_empty() && !v.starts_with('-') {
            grant(&key, v);
        }
    }
    *KEY.lock() = Some(key);
    Ok(())
}

fn grant(key: &Key, path: &str) {
    if key.trusted(|| plat::file_exists(path)) == 2 {
        key.grant_dir(path, FS_WRITE);
    } else {
        key.grant(path, FS_WRITE);
    }
}

/// a path the user chose (file dialog, drop): read and write it (with fs.user)
pub(crate) fn grant_user(path: &str) {
    let k = KEY.lock().clone();
    if let Some(k) = k {
        grant(&k, path);
    }
}

/// f without the file checks of the policy on this thread
pub(crate) fn trusted<R>(f: impl FnOnce() -> R) -> R {
    let k = KEY.lock().clone();
    match k {
        Some(k) => k.trusted(f),
        None => f(),
    }
}

/// the paths of a drop list (each one the user's)
pub(crate) fn grant_all(list: &[String]) {
    for p in list {
        grant_user(p);
    }
}
