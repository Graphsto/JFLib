// vfs -- one file namespace on every platform: "scheme://path" reaches a mounted provider,
// every other name is a path of the OS (or of the browser's virtual files)
//
//   mount(scheme, provider) / unmount(scheme) -> bool / mounted(scheme) -> bool
//   provider(scheme) -> Option<Arc<dyn Provider>>   a mount's provider (to build an Overlay or
//                                a Sub of app://, for example)
//   route(name) -> Route         Os (no scheme), Vfs(provider, path) or Bad (a scheme name
//                                with ".." or an invalid scheme); "config:/" and "config:" are
//                                the root like "config://"
//   os_path(name) -> Option<String>   the OS path behind a name of a folder mount (for APIs
//                                that need a real file); an OS path comes back unchanged
//   app_id() / set_app_id(id)    the name of the app's own folders (build_rs.py: the app's
//                                folder name; else the exe's name)
//   app_dir(APP_CONFIG | APP_DATA | APP_CACHE) -> Option<String>   the app's own folders:
//                                Windows %APPDATA%/<id>, %LOCALAPPDATA%/<id>,
//                                %LOCALAPPDATA%/<id>/Cache; Linux the XDG config / data /
//                                cache homes + /<id>; browser /config, /data, /cache + /<id>
//   Built in, mounted on first use: mem:// (one MemFs per process), config://, data://,
//   cache:// (DirFs on app_dir), app:// (the app's bundle; ui::main! installs it, else empty)
//   Providers: MemFs, DirFs::new(root), Overlay::new(layers), Sub::new(provider, folder),
//   Bundle::new(bytes) (JFA1, tools/assetpack.py); archive::ZipFs (lib/archive)
//   pack_bundle(files) -> Vec<u8>    a JFA1 bundle (tests, tools; build_rs.py packs with
//                                tools/assetpack.py)
//
// The public file functions (guarded.rs) route names with a scheme here: read_file,
// write_file, file_exists(_checked), mkdir, delete, rename (inside one mount) and dir_list;
// File::open, File::open_temp and FileLock::acquire work for folder mounts (os_path).  Paths
// inside a mount use '/', have no "." or empty segments and never "..".  Writes create
// missing folders.  DirFs uses the checked OS functions, so the app's permissions apply to
// it; the app's own folders are always allowed (policy.rs).
//
// JFA1 bundle (little endian): "JFA1", u32 count, u32 names_len; count entries of 20 bytes
// sorted by name: u32 name_off, u16 name_len, u8 codec (0 stored, 1 LZ4 block), u8 0,
// u32 data_off (from the bundle start), u32 stored_len, u32 size; the names; the data.

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use rt::sync::SpinLock;

/// app_dir: the app's settings folder (config://)
pub const APP_CONFIG: u32 = 0;
/// app_dir: the app's data folder (data://)
pub const APP_DATA: u32 = 1;
/// app_dir: the app's cache folder (cache://)
pub const APP_CACHE: u32 = 2;

/// A file tree behind a scheme.  Paths are relative ("" is the root), '/' separated and
/// normalised by route().
pub trait Provider: Send + Sync {
    /// 0 missing, 1 file, 2 folder
    fn kind(&self, path: &str) -> u32;
    /// the whole file
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// f(name, is_dir, size) for every entry of a folder -> false when it is no folder
    fn list(&self, path: &str, f: &mut dyn FnMut(&str, bool, u64)) -> bool;
    /// replace or create a file (missing folders are created)
    fn write(&self, _path: &str, _data: &[u8]) -> bool {
        false
    }
    /// create a folder and its parents (an existing folder is fine)
    fn mkdir(&self, _path: &str) -> bool {
        false
    }
    /// a file or an empty folder
    fn delete(&self, _path: &str) -> bool {
        false
    }
    /// a file to another name (replacing it)
    fn rename(&self, _old: &str, _new: &str) -> bool {
        false
    }
    /// the OS path of path (folder mounts only)
    fn os_path(&self, _path: &str) -> Option<String> {
        None
    }
}

/// where a name leads
pub enum Route {
    /// no scheme: a path of the OS
    Os,
    /// a mounted provider and the path inside it
    Vfs(Arc<dyn Provider>, String),
    /// a scheme name that cannot be used (".." or no such mount)
    Bad,
}

static MOUNTS: SpinLock<Vec<(String, Arc<dyn Provider>)>> = SpinLock::new(Vec::new());
static APP_ID: SpinLock<Option<String>> = SpinLock::new(None);

/// mount provider under scheme (lower-case letters, digits, '+', '-', '.'; at least two
/// characters); replaces an earlier mount -> false for an invalid scheme
pub fn mount(scheme: &str, provider: Arc<dyn Provider>) -> bool {
    if !scheme_ok(scheme) {
        return false;
    }
    let mut m = MOUNTS.lock();
    m.retain(|(s, _)| s != scheme);
    m.push((String::from(scheme), provider));
    true
}

/// remove a mount -> whether there was one
pub fn unmount(scheme: &str) -> bool {
    let mut m = MOUNTS.lock();
    let n = m.len();
    m.retain(|(s, _)| s != scheme);
    m.len() != n
}

/// whether scheme is mounted (the built-in schemes count once they were used)
pub fn mounted(scheme: &str) -> bool {
    MOUNTS.lock().iter().any(|(s, _)| s == scheme)
}

fn scheme_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_lowercase() && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"+-.".contains(c))
}

// "scheme:" + "", "/" or "//" + rest -> (scheme, rest); None for an OS path
fn split(name: &str) -> Option<(&str, &str)> {
    let k = name.find(':')?;
    let (scheme, rest) = (&name[..k], &name[k + 1..]);
    if !scheme_ok(scheme) || !(rest.is_empty() || rest.starts_with('/')) {
        return None;
    }
    Some((scheme, rest.trim_start_matches('/')))
}

/// the normalised path inside a mount ('\' -> '/', no "." / empty segments) or None (a "..")
pub fn clean(path: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => return None,
            s if s.contains('\0') => return None,
            s => out.push(s),
        }
    }
    Some(out.join("/"))
}

/// where name leads (built-in schemes are mounted on first use)
pub fn route(name: &str) -> Route {
    let Some((scheme, rest)) = split(name) else { return Route::Os };
    let Some(path) = clean(rest) else { return Route::Bad };
    if let Some(p) = MOUNTS.lock().iter().find(|(s, _)| s == scheme).map(|(_, p)| p.clone()) {
        return Route::Vfs(p, path);
    }
    let builtin: Option<Arc<dyn Provider>> = match scheme {
        "mem" => Some(Arc::new(MemFs::new())),
        "app" => Some(Arc::new(Bundle::empty())),
        "config" => app_dir(APP_CONFIG).map(|d| Arc::new(DirFs::new(&d)) as Arc<dyn Provider>),
        "data" => app_dir(APP_DATA).map(|d| Arc::new(DirFs::new(&d)) as Arc<dyn Provider>),
        "cache" => app_dir(APP_CACHE).map(|d| Arc::new(DirFs::new(&d)) as Arc<dyn Provider>),
        _ => None,
    };
    let Some(b) = builtin else { return Route::Bad };
    let mut m = MOUNTS.lock();
    // another thread may have mounted it meanwhile
    let p = match m.iter().find(|(s, _)| s == scheme) {
        Some((_, p)) => p.clone(),
        None => {
            m.push((String::from(scheme), b.clone()));
            b
        }
    };
    Route::Vfs(p, path)
}

/// the provider mounted under scheme (built-in schemes are mounted on first use)
pub fn provider(scheme: &str) -> Option<Arc<dyn Provider>> {
    match route(&alloc::format!("{scheme}://")) {
        Route::Vfs(p, _) => Some(p),
        _ => None,
    }
}

/// the OS path behind name: a folder mount's file, or name itself when it has no scheme
pub fn os_path(name: &str) -> Option<String> {
    match route(name) {
        Route::Os => Some(String::from(name)),
        Route::Vfs(p, path) => p.os_path(&path),
        Route::Bad => None,
    }
}

// ---------------------------------------------------------------- the app's folders

/// set the name of the app's own folders (ui::main!: the app folder of build_rs.py); ignored
/// unless it is a plain file name (letters, digits, '.', '-', '_', not only dots)
pub fn set_app_id(id: &str) {
    let ok = !id.is_empty() && id.len() <= 64 && !id.bytes().all(|c| c == b'.')
        && id.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c));
    if ok {
        *APP_ID.lock() = Some(String::from(id));
    }
}

/// the name of the app's own folders: set_app_id, else the exe's name without extension
pub fn app_id() -> String {
    if let Some(id) = APP_ID.lock().clone() {
        return id;
    }
    exe_stem().unwrap_or_else(|| String::from("app"))
}

#[cfg(any(windows, target_os = "linux"))]
fn exe_stem() -> Option<String> {
    let p = crate::current_exe()?;
    let name = &p[p.rfind(['/', '\\']).map_or(0, |k| k + 1)..];
    let stem = name.strip_suffix(".exe").unwrap_or(name);
    (!stem.is_empty()).then(|| String::from(stem))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn exe_stem() -> Option<String> {
    None
}

/// the app's settings (APP_CONFIG), data (APP_DATA) or cache (APP_CACHE) folder
pub fn app_dir(kind: u32) -> Option<String> {
    let id = app_id();
    match kind {
        APP_CONFIG => crate::config_dir().map(|d| alloc::format!("{d}/{id}")),
        APP_DATA => crate::data_dir().map(|d| alloc::format!("{d}/{id}")),
        APP_CACHE if cfg!(windows) => crate::data_dir().map(|d| alloc::format!("{d}/{id}/Cache")),
        APP_CACHE => crate::cache_dir().map(|d| alloc::format!("{d}/{id}")),
        _ => None,
    }
}

// ---------------------------------------------------------------- MemFs

/// files in memory (mem://, tests, generated data)
pub struct MemFs {
    t: SpinLock<MemTree>,
}

#[derive(Default)]
struct MemTree {
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeSet<String>,
}

fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |k| &path[..k])
}

fn child_of<'a>(entry: &'a str, dir: &str) -> Option<&'a str> {
    let rest = if dir.is_empty() { entry } else { entry.strip_prefix(dir)?.strip_prefix('/')? };
    (!rest.is_empty() && !rest.contains('/')).then_some(rest)
}

impl MemFs {
    pub fn new() -> MemFs {
        MemFs { t: SpinLock::new(MemTree::default()) }
    }
}

impl Default for MemFs {
    fn default() -> MemFs {
        MemFs::new()
    }
}

impl MemTree {
    fn add_dirs(&mut self, mut d: &str) {
        while !d.is_empty() {
            self.dirs.insert(String::from(d));
            d = parent(d);
        }
    }

    fn kind(&self, path: &str) -> u32 {
        if self.files.contains_key(path) {
            1
        } else if path.is_empty() || self.dirs.contains(path) {
            2
        } else {
            0
        }
    }
}

impl Provider for MemFs {
    fn kind(&self, path: &str) -> u32 {
        self.t.lock().kind(path)
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.t.lock().files.get(path).cloned()
    }

    fn list(&self, path: &str, f: &mut dyn FnMut(&str, bool, u64)) -> bool {
        let entries: Vec<(String, bool, u64)> = {
            let t = self.t.lock();
            if t.kind(path) != 2 {
                return false;
            }
            let dirs = t.dirs.iter().filter_map(|d| child_of(d, path)).map(|n| (String::from(n), true, 0));
            let files = t.files.iter().filter_map(|(k, v)| child_of(k, path).map(|n| (String::from(n), false, v.len() as u64)));
            dirs.chain(files).collect()
        };
        for (n, d, s) in entries {
            f(&n, d, s);
        }
        true
    }

    fn write(&self, path: &str, data: &[u8]) -> bool {
        let mut t = self.t.lock();
        if path.is_empty() || t.dirs.contains(path) {
            return false;
        }
        let p = parent(path);
        if t.files.contains_key(p) {
            return false; // a file where a folder should be
        }
        t.add_dirs(p);
        t.files.insert(String::from(path), data.to_vec());
        true
    }

    fn mkdir(&self, path: &str) -> bool {
        let mut t = self.t.lock();
        let mut d = path;
        while !d.is_empty() {
            if t.files.contains_key(d) {
                return false;
            }
            d = parent(d);
        }
        t.add_dirs(path);
        true
    }

    fn delete(&self, path: &str) -> bool {
        let mut t = self.t.lock();
        if t.files.remove(path).is_some() {
            return true;
        }
        let busy = t.files.keys().chain(t.dirs.iter()).any(|e| child_of(e, path).is_some());
        !path.is_empty() && !busy && t.dirs.remove(path)
    }

    fn rename(&self, old: &str, new: &str) -> bool {
        let mut t = self.t.lock();
        if new.is_empty() || t.dirs.contains(new) || t.files.contains_key(parent(new)) {
            return false;
        }
        let Some(data) = t.files.remove(old) else { return false };
        t.add_dirs(parent(new));
        t.files.insert(String::from(new), data);
        true
    }
}

// ---------------------------------------------------------------- DirFs

/// an OS folder as the root (config://, data://, cache://, mounts of the app); every call
/// goes through the checked OS functions, so the app's permissions apply
pub struct DirFs {
    root: String,
}

impl DirFs {
    /// root: an absolute OS folder (created on the first write)
    pub fn new(root: &str) -> DirFs {
        DirFs { root: String::from(root.trim_end_matches(['/', '\\'])) }
    }

    fn full(&self, path: &str) -> String {
        if path.is_empty() { self.root.clone() } else { alloc::format!("{}/{}", self.root, path) }
    }
}

impl Provider for DirFs {
    fn kind(&self, path: &str) -> u32 {
        crate::file_exists(&self.full(path))
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        crate::read_file(&self.full(path))
    }

    fn list(&self, path: &str, f: &mut dyn FnMut(&str, bool, u64)) -> bool {
        crate::dir_list(&self.full(path), &mut |n, d, s| f(n, d, s as u64))
    }

    fn write(&self, path: &str, data: &[u8]) -> bool {
        let full = self.full(path);
        crate::mkdir(&self.full(parent(path)));
        !path.is_empty() && crate::write_file(&full, data)
    }

    fn mkdir(&self, path: &str) -> bool {
        crate::mkdir(&self.full(path))
    }

    fn delete(&self, path: &str) -> bool {
        !path.is_empty() && crate::delete(&self.full(path))
    }

    fn rename(&self, old: &str, new: &str) -> bool {
        crate::mkdir(&self.full(parent(new)));
        !old.is_empty() && !new.is_empty() && crate::rename(&self.full(old), &self.full(new))
    }

    fn os_path(&self, path: &str) -> Option<String> {
        Some(self.full(path))
    }
}

// ---------------------------------------------------------------- Overlay, Sub

/// several providers in one tree: a path comes from the first layer that has it, a folder
/// lists the entries of every layer, writes go to the first layer (deleting a file of a lower
/// layer is not possible)
pub struct Overlay {
    layers: Vec<Arc<dyn Provider>>,
}

impl Overlay {
    /// layers[0] is on top
    pub fn new(layers: Vec<Arc<dyn Provider>>) -> Overlay {
        Overlay { layers }
    }
}

impl Provider for Overlay {
    fn kind(&self, path: &str) -> u32 {
        self.layers.iter().map(|l| l.kind(path)).find(|&k| k != 0).unwrap_or(0)
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.layers.iter().find(|l| l.kind(path) == 1).and_then(|l| l.read(path))
    }

    fn list(&self, path: &str, f: &mut dyn FnMut(&str, bool, u64)) -> bool {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut any = false;
        for l in &self.layers {
            any |= l.list(path, &mut |n, d, s| {
                if seen.insert(String::from(n)) {
                    f(n, d, s);
                }
            });
        }
        any
    }

    fn write(&self, path: &str, data: &[u8]) -> bool {
        self.layers.first().is_some_and(|l| l.write(path, data))
    }

    fn mkdir(&self, path: &str) -> bool {
        self.layers.first().is_some_and(|l| l.mkdir(path))
    }

    fn delete(&self, path: &str) -> bool {
        self.layers.first().is_some_and(|l| l.delete(path))
    }

    fn rename(&self, old: &str, new: &str) -> bool {
        self.layers.first().is_some_and(|l| l.rename(old, new))
    }

    fn os_path(&self, path: &str) -> Option<String> {
        self.layers.iter().find(|l| l.kind(path) != 0).unwrap_or(self.layers.first()?).os_path(path)
    }
}

/// one folder of another provider as a root (a theme folder of app://)
pub struct Sub {
    inner: Arc<dyn Provider>,
    dir: String,
}

impl Sub {
    /// dir: a folder of inner ('/' separated; None for "..")
    pub fn new(inner: Arc<dyn Provider>, dir: &str) -> Option<Sub> {
        Some(Sub { inner, dir: clean(dir)? })
    }

    fn full(&self, path: &str) -> String {
        match (self.dir.is_empty(), path.is_empty()) {
            (true, _) => String::from(path),
            (false, true) => self.dir.clone(),
            (false, false) => alloc::format!("{}/{}", self.dir, path),
        }
    }
}

impl Provider for Sub {
    fn kind(&self, path: &str) -> u32 {
        self.inner.kind(&self.full(path))
    }
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.inner.read(&self.full(path))
    }
    fn list(&self, path: &str, f: &mut dyn FnMut(&str, bool, u64)) -> bool {
        self.inner.list(&self.full(path), f)
    }
    fn write(&self, path: &str, data: &[u8]) -> bool {
        !path.is_empty() && self.inner.write(&self.full(path), data)
    }
    fn mkdir(&self, path: &str) -> bool {
        self.inner.mkdir(&self.full(path))
    }
    fn delete(&self, path: &str) -> bool {
        !path.is_empty() && self.inner.delete(&self.full(path))
    }
    fn rename(&self, old: &str, new: &str) -> bool {
        !old.is_empty() && !new.is_empty() && self.inner.rename(&self.full(old), &self.full(new))
    }
    fn os_path(&self, path: &str) -> Option<String> {
        self.inner.os_path(&self.full(path))
    }
}

// ---------------------------------------------------------------- Bundle

const MAGIC: &[u8; 4] = b"JFA1";
const ENTRY: usize = 20;
const CODEC_STORED: u8 = 0;
const CODEC_LZ4: u8 = 1;

struct Item {
    name: String,
    codec: u8,
    off: usize,
    stored: usize,
    size: usize,
}

/// a read-only JFA1 bundle (app://: the app's assets folder, packed by build_rs.py)
pub struct Bundle {
    data: BundleBytes,
    items: Vec<Item>,
}

enum BundleBytes {
    Static(&'static [u8]),
    Owned(Box<[u8]>),
}

impl BundleBytes {
    fn get(&self) -> &[u8] {
        match self {
            BundleBytes::Static(b) => b,
            BundleBytes::Owned(b) => b,
        }
    }
}

fn rd32(b: &[u8], i: usize) -> usize {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
}

impl Bundle {
    /// a bundle without files
    pub fn empty() -> Bundle {
        Bundle { data: BundleBytes::Static(&[]), items: Vec::new() }
    }

    /// the bundle embedded in the program (None: malformed)
    pub fn from_static(b: &'static [u8]) -> Option<Bundle> {
        let items = index(b)?;
        Some(Bundle { data: BundleBytes::Static(b), items })
    }

    /// a bundle in memory (None: malformed)
    pub fn new(b: Vec<u8>) -> Option<Bundle> {
        let items = index(&b)?;
        Some(Bundle { data: BundleBytes::Owned(b.into_boxed_slice()), items })
    }

    /// the number of files
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// whether the bundle has no files
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn find(&self, path: &str) -> Result<usize, usize> {
        self.items.binary_search_by(|i| i.name.as_bytes().cmp(path.as_bytes()))
    }

    // whether some name lies below the folder path
    fn is_dir(&self, path: &str) -> bool {
        if path.is_empty() {
            return true;
        }
        let pre = alloc::format!("{path}/");
        let k = self.find(&pre).unwrap_or_else(|k| k);
        self.items.get(k).is_some_and(|i| i.name.starts_with(&pre))
    }
}

// the entries of a JFA1 bundle, every range checked; None when anything is out of place
fn index(b: &[u8]) -> Option<Vec<Item>> {
    if b.is_empty() {
        return Some(Vec::new());
    }
    if b.len() < 12 || &b[..4] != MAGIC {
        return None;
    }
    let (count, pool_len) = (rd32(b, 4), rd32(b, 8));
    let pool = 12usize.checked_add(count.checked_mul(ENTRY)?)?;
    if pool.checked_add(pool_len)? > b.len() {
        return None;
    }
    let mut items: Vec<Item> = Vec::with_capacity(count);
    for k in 0..count {
        let e = 12 + k * ENTRY;
        let (no, nl) = (rd32(b, e), u16::from_le_bytes([b[e + 4], b[e + 5]]) as usize);
        let (codec, off, stored, size) = (b[e + 6], rd32(b, e + 8), rd32(b, e + 12), rd32(b, e + 16));
        if no.checked_add(nl)? > pool_len || off.checked_add(stored)? > b.len() || codec > CODEC_LZ4 {
            return None;
        }
        if codec == CODEC_STORED && stored != size {
            return None;
        }
        let name = core::str::from_utf8(&b[pool + no..pool + no + nl]).ok()?;
        if clean(name).as_deref() != Some(name) || name.is_empty() {
            return None;
        }
        if items.last().is_some_and(|l| l.name.as_bytes() >= name.as_bytes()) {
            return None; // not sorted, or twice
        }
        items.push(Item { name: name.to_owned(), codec, off, stored, size });
    }
    Some(items)
}

impl Provider for Bundle {
    fn kind(&self, path: &str) -> u32 {
        if self.find(path).is_ok() {
            1
        } else if self.is_dir(path) {
            2
        } else {
            0
        }
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let i = &self.items[self.find(path).ok()?];
        let blob = &self.data.get()[i.off..i.off + i.stored];
        match i.codec {
            CODEC_LZ4 => compress::lz4::decompress(blob, i.size).filter(|d| d.len() == i.size),
            _ => Some(blob.to_vec()),
        }
    }

    fn list(&self, path: &str, f: &mut dyn FnMut(&str, bool, u64)) -> bool {
        if self.kind(path) != 2 {
            return false;
        }
        let pre = if path.is_empty() { String::new() } else { alloc::format!("{path}/") };
        let start = self.find(&pre).unwrap_or_else(|k| k);
        let mut last_dir = String::new();
        for i in self.items[start..].iter().take_while(|i| i.name.starts_with(&pre)) {
            let rest = &i.name[pre.len()..];
            match rest.find('/') {
                None => f(rest, false, i.size as u64),
                Some(k) => {
                    if rest[..k] != last_dir {
                        last_dir = String::from(&rest[..k]);
                        f(&last_dir, true, 0);
                    }
                }
            }
        }
        true
    }
}

/// a JFA1 bundle of (name, data) pairs: LZ4 where that saves a tenth (tests, tools); None for
/// a name route() would refuse, a name twice, or a bundle beyond 4 GiB
pub fn pack_bundle(files: &[(&str, &[u8])]) -> Option<Vec<u8>> {
    let mut v: Vec<(&str, &[u8])> = files.to_vec();
    v.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    if v.windows(2).any(|w| w[0].0 == w[1].0) || v.iter().any(|(n, _)| n.is_empty() || clean(n).as_deref() != Some(*n)) {
        return None;
    }
    let blobs: Vec<(u8, Vec<u8>, usize)> = v
        .iter()
        .map(|(_, d)| {
            let c = if d.is_empty() { Vec::new() } else { compress::lz4::compress(d) };
            if !d.is_empty() && c.len() * 10 <= d.len() * 9 { (CODEC_LZ4, c, d.len()) } else { (CODEC_STORED, d.to_vec(), d.len()) }
        })
        .collect();
    let pool_len: usize = v.iter().map(|(n, _)| n.len()).sum();
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    let u32_of = |x: usize| u32::try_from(x).ok();
    out.extend_from_slice(&u32_of(v.len())?.to_le_bytes());
    out.extend_from_slice(&u32_of(pool_len)?.to_le_bytes());
    let mut off = 12 + ENTRY * v.len() + pool_len;
    let mut name_off = 0;
    for ((n, _), (codec, blob, size)) in v.iter().zip(&blobs) {
        out.extend_from_slice(&u32_of(name_off)?.to_le_bytes());
        out.extend_from_slice(&u16::try_from(n.len()).ok()?.to_le_bytes());
        out.push(*codec);
        out.push(0);
        out.extend_from_slice(&u32_of(off)?.to_le_bytes());
        out.extend_from_slice(&u32_of(blob.len())?.to_le_bytes());
        out.extend_from_slice(&u32_of(*size)?.to_le_bytes());
        name_off += n.len();
        off += blob.len();
    }
    for (n, _) in &v {
        out.extend_from_slice(n.as_bytes());
    }
    for (_, blob, _) in &blobs {
        out.extend_from_slice(blob);
    }
    u32_of(out.len())?;
    Some(out)
}
