// policy -- the app's permissions (apps/NAME/PERMISSIONS, docs/PERMISSIONS.md): parsed once at
// start-up, then checked by the public file, process, shell, plugin and print functions
//
//   install(text) -> Result<Key, String>   parse and activate a manifest (once per process);
//                                          before that, and in apps without one, all is allowed
//   active() -> bool
//   fs_allowed(path, FS_READ | FS_WRITE) / process_allowed(program) / url_allowed(url) /
//   plugin_allowed(path) / print_allowed() -> bool
//                                          a refusal is recorded (denials) and printed once
//                                          per target to stderr: "PERMDENY kind target"
//   net_connect_allowed(addr) / net_listen_allowed(addr) / net_name_allowed(host) -> bool
//   net_resolved(host, addrs)              (resolvers) the addresses of a permitted name:
//                                          connections to them follow that name's rules
//   Key::grant(path, FS_*) / Key::grant_dir(dir, FS_*)   a file / folder the user handed over
//                                          (file dialog, drop, command line); needs fs.user
//   Key::trusted(f)                        f runs on this thread without the file checks (the
//                                          library's own file dialog and test scripts)
//   denials() -> Vec<Denial>               the last DENIALS_MAX refusals (audit)
//   normalize(path) -> Option<String>      absolute, '/' separators, no "." / ".." / empty
//                                          segments; Windows: upper-case drive letter, no
//                                          device, stream or reserved names; Linux: no ".."
//                                          at all (the kernel applies it after symbolic
//                                          links); None = refused
//   normalize_for(path, windows, cwd)      the same rules for either OS (tests, tools)
//   font_dirs() -> [folder]                the system font folders in scan order (fontdb)
//   cap_mask() -> CAP_* bits               what the policy leaves of capabilities()
//
// Manifest: one rule per line, '#' starts a comment, a key may repeat:
//   fs.read PATTERN..     read files below PATTERN
//   fs.write PATTERN..    write (and read) files below PATTERN
//   fs.user               files the user chooses (dialog, drop, command line): read + write
//   process PATTERN|NAME..   programs Command::spawn / run_elevated may start (NAME: no '/')
//   url SCHEME..|*        open_url schemes (https, mailto, ...)
//   plugins PATTERN..     native libraries for lib_open
//   print                 plat::print
//   net.connect HOST:PORT..   TCP connections and UDP datagrams to HOST (a name, *.name for
//                         every name below it, an IPv4 / [IPv6] address, a block 10.0.0.0/8
//                         or [fd00::]/8, localhost = the loopback addresses, * = any) on PORT
//                         (a number, a range 8000-8099 or *); names are checked when they are
//                         resolved, their addresses then count for the name; with any name
//                         rule, port 53 (DNS) is open too
//   net.listen ADDR:PORT..    TCP listeners and UDP sockets with a fixed port on ADDR (an
//                         address, a block, localhost or *; 0.0.0.0 / [::] = every interface)
//                         and PORT (port 0 = any free one: the rule needs 0 or *)
// A PATTERN is an absolute path (X:/.. and //server/share/.. count on Windows only, /.. on
// Linux only), or starts with $CONFIG $TEMP $HOME $EXE $FONTS or the app's own folders
// $APPCONFIG $APPDATA $APPCACHE (vfs::app_dir; a variable without a value drops the
// pattern); '*' / '?' match inside one segment, a '**' segment any number of segments
// ("dir/**" also matches dir itself); "**" alone matches every path.  Windows compares
// without case.  Always readable: the font folders, the exe folder and on Linux
// /etc/resolv.conf and /etc/hosts (netio's resolver); always writable: the app's own
// folders (config://, data://, cache://; vfs::set_app_id comes first).
//
// The checks are lexical: a symbolic link inside an allowed folder can point outside it.  On
// Linux a path with a ".." segment is refused: "allowed/link/../x" would leave the folder.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rt::sync::SpinLock;

/// read access ([`fs_allowed`], [`Key::grant`])
pub const FS_READ: u32 = 1;
/// write access; implies [`FS_READ`]
pub const FS_WRITE: u32 = 2;
/// refusals kept for [`denials`]
pub const DENIALS_MAX: usize = 64;

/// one refused access
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Denial {
    /// "fs.read", "fs.write", "process", "url", "plugins" or "print"
    pub kind: &'static str,
    /// the path, program, URL or "" (print)
    pub target: String,
}

struct Rule {
    segs: Vec<String>,
    mode: u32,
}

#[derive(Default)]
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
struct Policy {
    fs: Vec<Rule>,
    explicit_read: bool,
    explicit_write: bool,
    user: bool,
    process: Vec<Vec<String>>,
    names: Vec<String>,
    urls: Vec<String>,
    plugins: Vec<Vec<String>>,
    print: bool,
    connect: Vec<NetRule>,
    listen: Vec<NetRule>,
    /// the addresses of resolved names with the port ranges of their rules
    resolved: Vec<([u8; 16], u16, u16)>,
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static POLICY: SpinLock<Option<Policy>> = SpinLock::new(None);
static DENIALS: SpinLock<Vec<Denial>> = SpinLock::new(Vec::new());
// threads inside Key::trusted: thread id + 1, 0 = free
const BYPASS_SLOTS: usize = 4;
static BYPASS: [AtomicU64; BYPASS_SLOTS] = [const { AtomicU64::new(0) }; BYPASS_SLOTS];

/// The right to widen the installed policy for the user's own files and to run trusted
/// library code; only [`install`] creates one (the UI runtime keeps it to itself).
#[derive(Clone)]
pub struct Key(());

/// parse and activate a manifest; Err names the first bad line.  Only the first call of a
/// process can succeed.
pub fn install(text: &str) -> Result<Key, String> {
    let p = parse(text)?;
    let mut slot = POLICY.lock();
    if slot.is_some() {
        return Err(String::from("a policy is already installed"));
    }
    *slot = Some(p);
    ACTIVE.store(true, Ordering::SeqCst);
    Ok(Key(()))
}

/// whether a policy restricts this process
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// whether path may be read (FS_READ) or written (FS_WRITE)
pub fn fs_allowed(path: &str, mode: u32) -> bool {
    if !active() || bypassed() {
        return true;
    }
    let ok = normalize(path).is_some_and(|n| {
        let segs = split(&fold(&n));
        let g = POLICY.lock();
        let Some(p) = g.as_ref() else { return true };
        p.fs.iter().any(|r| (r.mode & mode == mode || r.mode & FS_WRITE != 0) && seg_match(&r.segs, &segs))
    });
    ok || deny(if mode & FS_WRITE != 0 { "fs.write" } else { "fs.read" }, path)
}

/// whether Command::spawn / run_elevated may start program (a path, or a name found in PATH)
pub fn process_allowed(program: &str) -> bool {
    if !active() {
        return true;
    }
    let ok = {
        let g = POLICY.lock();
        let Some(p) = g.as_ref() else { return true };
        if program.contains(['/', '\\']) {
            normalize(program).is_some_and(|n| {
                let segs = split(&fold(&n));
                p.process.iter().any(|r| seg_match(r, &segs))
            })
        } else {
            let n = fold(program);
            p.names.iter().any(|r| *r == n)
        }
    };
    ok || deny("process", program)
}

/// whether open_url may open url (its scheme)
pub fn url_allowed(url: &str) -> bool {
    if !active() {
        return true;
    }
    let scheme = match url.find(':') {
        Some(k) if k > 0 => url[..k].to_ascii_lowercase(),
        _ => String::new(),
    };
    let ok = {
        let g = POLICY.lock();
        let Some(p) = g.as_ref() else { return true };
        !scheme.is_empty() && p.urls.iter().any(|u| u == "*" || *u == scheme)
    };
    ok || deny("url", url)
}

/// whether lib_open may load the native library at path
pub fn plugin_allowed(path: &str) -> bool {
    if !active() {
        return true;
    }
    let ok = normalize(path).is_some_and(|n| {
        let segs = split(&fold(&n));
        let g = POLICY.lock();
        let Some(p) = g.as_ref() else { return true };
        p.plugins.iter().any(|r| seg_match(r, &segs))
    });
    ok || deny("plugins", path)
}

/// whether plat::print may print
pub fn print_allowed() -> bool {
    if !active() {
        return true;
    }
    let ok = POLICY.lock().as_ref().is_none_or(|p| p.print);
    ok || deny("print", "")
}

/// the last refusals, oldest first
pub fn denials() -> Vec<Denial> {
    DENIALS.lock().clone()
}

/// the CAP_* bits the policy leaves (all without a policy)
pub fn cap_mask() -> u64 {
    if !active() {
        return u64::MAX;
    }
    let g = POLICY.lock();
    let Some(p) = g.as_ref() else { return u64::MAX };
    let mut m = u64::MAX;
    if !(p.explicit_read || p.explicit_write || p.user) {
        m &= !crate::CAP_FILE_READ;
    }
    if !(p.explicit_write || p.user) {
        m &= !(crate::CAP_FILE_WRITE | crate::CAP_PERSISTENT_STORAGE);
    }
    if !p.print {
        m &= !crate::CAP_PRINT;
    }
    if p.plugins.is_empty() {
        m &= !crate::CAP_NATIVE_PLUGINS;
    }
    m
}

impl Key {
    /// let the app read (FS_READ) or read and write (FS_WRITE) the file at path, which the
    /// user chose; false without fs.user or for a path normalize refuses
    pub fn grant(&self, path: &str, mode: u32) -> bool {
        self.add(path, mode, false)
    }

    /// the same for a folder and everything below it
    pub fn grant_dir(&self, dir: &str, mode: u32) -> bool {
        self.add(dir, mode, true)
    }

    fn add(&self, path: &str, mode: u32, dir: bool) -> bool {
        let Some(n) = normalize(path) else { return false };
        let mut segs = split(&fold(&n));
        if dir {
            segs.push(String::from("**"));
        }
        let mut g = POLICY.lock();
        let Some(p) = g.as_mut() else { return false };
        if !p.user {
            return false;
        }
        if !p.fs.iter().any(|r| r.mode == mode && r.segs == segs) {
            p.fs.push(Rule { segs, mode });
        }
        true
    }

    /// run f on this thread without the file checks
    pub fn trusted<R>(&self, f: impl FnOnce() -> R) -> R {
        let me = tid() + 1;
        if BYPASS.iter().any(|b| b.load(Ordering::Acquire) == me) {
            return f(); // nested
        }
        let slot = BYPASS.iter().find(|b| b.compare_exchange(0, me, Ordering::AcqRel, Ordering::Relaxed).is_ok());
        let r = f();
        if let Some(b) = slot {
            b.store(0, Ordering::Release);
        }
        r
    }
}

fn bypassed() -> bool {
    let me = tid() + 1;
    BYPASS.iter().any(|b| b.load(Ordering::Acquire) == me)
}

#[cfg(any(windows, target_os = "linux"))]
fn tid() -> u64 {
    crate::plat_tid()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn tid() -> u64 {
    0
}

// record a refusal (once per kind and target on stderr) -> false
fn deny(kind: &'static str, target: &str) -> bool {
    let d = Denial { kind, target: String::from(target) };
    let mut g = DENIALS.lock();
    let seen = g.contains(&d);
    if g.len() == DENIALS_MAX {
        g.remove(0);
    }
    g.push(d);
    drop(g);
    if !seen {
        let sep = if target.is_empty() { "" } else { " " };
        crate::write_stderr(alloc::format!("PERMDENY {}{}{}\n", kind, sep, target).as_bytes());
    }
    false
}

// ---------------------------------------------------------------- the manifest

fn parse(text: &str) -> Result<Policy, String> {
    let mut p = Policy::default();
    for d in font_dirs() {
        add_dir_rule(&mut p.fs, &d, FS_READ);
    }
    if let Some(e) = exe_dir() {
        add_dir_rule(&mut p.fs, &e, FS_READ);
    }
    for k in [crate::vfs::APP_CONFIG, crate::vfs::APP_DATA, crate::vfs::APP_CACHE] {
        if let Some(d) = crate::vfs::app_dir(k) {
            add_dir_rule(&mut p.fs, &d, FS_WRITE);
        }
    }
    if cfg!(target_os = "linux") {
        for f in ["/etc/resolv.conf", "/etc/hosts"] {
            p.fs.push(Rule { segs: split(f), mode: FS_READ });
        }
    }
    for (i, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("");
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        let args: Vec<&str> = words.collect();
        let bad = |msg: &str| Err(alloc::format!("PERMISSIONS:{}: {}", i + 1, msg));
        match key {
            "fs.read" | "fs.write" => {
                if args.is_empty() {
                    return bad("expected at least one pattern");
                }
                let mode = if key == "fs.write" { FS_WRITE } else { FS_READ };
                for a in &args {
                    for segs in pattern(a).map_err(|e| alloc::format!("PERMISSIONS:{}: {}", i + 1, e))? {
                        p.fs.push(Rule { segs, mode });
                    }
                }
                if mode == FS_WRITE {
                    p.explicit_write = true;
                } else {
                    p.explicit_read = true;
                }
            }
            "fs.user" | "print" => {
                if !args.is_empty() {
                    return bad("takes no arguments");
                }
                if key == "print" {
                    p.print = true;
                } else {
                    p.user = true;
                }
            }
            "process" | "plugins" => {
                if args.is_empty() {
                    return bad("expected at least one pattern");
                }
                for a in &args {
                    if key == "process" && !a.contains(['/', '\\', '$']) {
                        p.names.push(fold(a));
                        continue;
                    }
                    let pats = pattern(a).map_err(|e| alloc::format!("PERMISSIONS:{}: {}", i + 1, e))?;
                    if key == "process" {
                        p.process.extend(pats);
                    } else {
                        p.plugins.extend(pats);
                    }
                }
            }
            "net.connect" | "net.listen" => {
                if args.is_empty() {
                    return bad("expected at least one HOST:PORT");
                }
                for a in &args {
                    let r = net_rule(a, key == "net.listen").map_err(|e| alloc::format!("PERMISSIONS:{}: {}", i + 1, e))?;
                    if key == "net.listen" {
                        p.listen.push(r);
                    } else {
                        p.connect.push(r);
                    }
                }
            }
            "url" => {
                if args.is_empty() {
                    return bad("expected at least one scheme or *");
                }
                for a in &args {
                    let ok = *a == "*" || a.bytes().all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c));
                    if !ok {
                        return bad("a scheme has only letters, digits, '+', '-' and '.'");
                    }
                    p.urls.push(a.to_ascii_lowercase());
                }
            }
            _ => return bad("unknown permission (fs.read, fs.write, fs.user, process, url, plugins, print, net.connect, net.listen)"),
        }
    }
    Ok(p)
}

fn add_dir_rule(rules: &mut Vec<Rule>, dir: &str, mode: u32) {
    if let Some(n) = normalize(dir) {
        let mut segs = split(&fold(&n));
        segs.push(String::from("**"));
        rules.push(Rule { segs, mode });
    }
}

// a pattern -> its segment lists (several for $FONTS, none for a variable without a value)
fn pattern(a: &str) -> Result<Vec<Vec<String>>, String> {
    if a == "**" {
        return Ok(alloc::vec![alloc::vec![String::from("**")]]);
    }
    let bases: Vec<String> = if let Some(v) = a.strip_prefix('$') {
        let end = v.find(['/', '\\']).unwrap_or(v.len());
        let base = match &v[..end] {
            "CONFIG" => crate::config_dir().into_iter().collect(),
            "TEMP" => crate::temp_dir().into_iter().collect(),
            "HOME" => crate::home().into_iter().collect(),
            "EXE" => exe_dir().into_iter().collect(),
            "FONTS" => font_dirs(),
            "APPCONFIG" => crate::vfs::app_dir(crate::vfs::APP_CONFIG).into_iter().collect(),
            "APPDATA" => crate::vfs::app_dir(crate::vfs::APP_DATA).into_iter().collect(),
            "APPCACHE" => crate::vfs::app_dir(crate::vfs::APP_CACHE).into_iter().collect(),
            other => {
                return Err(alloc::format!("unknown variable ${} (CONFIG, TEMP, HOME, EXE, FONTS, APPCONFIG, APPDATA, APPCACHE)", other));
            }
        };
        base.into_iter().map(|b| alloc::format!("{}{}", b, &v[end..])).collect()
    } else {
        // "X:/.." and "//server/share/.." are Windows paths, "/.." Linux paths; the other
        // OS ignores them, so one manifest serves both
        let b = a.as_bytes();
        let win_abs = (is_drive(a) && matches!(b.get(2), Some(b'/' | b'\\'))) || a.starts_with("//") || a.starts_with("\\\\");
        let lin_abs = !win_abs && a.starts_with('/');
        if !win_abs && !lin_abs {
            return Err(alloc::format!("{} is not absolute (start it with /, X:/ or a variable)", a));
        }
        if win_abs != cfg!(windows) {
            return Ok(Vec::new());
        }
        alloc::vec![String::from(a)]
    };
    let mut out = Vec::new();
    for b in bases {
        match norm(&b, cfg!(windows), true, &|| String::new()) {
            Some(n) => out.push(split(&fold(&n))),
            None => return Err(alloc::format!("{} is not a valid path pattern", a)),
        }
    }
    Ok(out)
}

/// the system font folders in the order fontdb scans them: jfgui_FONTDIRS (';' separated,
/// on Linux also ':'), else the Windows / XDG folders
pub fn font_dirs() -> Vec<String> {
    let mut v = Vec::new();
    if let Some(e) = crate::env("jfgui_FONTDIRS").filter(|e| !e.is_empty()) {
        let sep = |c: char| c == ';' || (cfg!(target_os = "linux") && c == ':');
        return e.split(sep).filter(|s| !s.is_empty()).map(String::from).collect();
    }
    let env = |var: &str, suffix: &str| crate::env(var).filter(|s| !s.is_empty()).map(|s| s + suffix);
    if cfg!(windows) {
        v.extend(env("WINDIR", "/Fonts"));
        v.extend(env("LOCALAPPDATA", "/Microsoft/Windows/Fonts"));
    } else {
        v.push(String::from("/usr/share/fonts"));
        v.push(String::from("/usr/local/share/fonts"));
        match env("XDG_DATA_HOME", "/fonts") {
            Some(d) => v.push(d),
            None => v.extend(env("HOME", "/.local/share/fonts")),
        }
        v.extend(env("HOME", "/.fonts"));
    }
    v
}

#[cfg(any(windows, target_os = "linux"))]
fn exe_dir() -> Option<String> {
    let p = crate::current_exe()?;
    let k = p.rfind(['/', '\\'])?;
    Some(String::from(&p[..k]))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn exe_dir() -> Option<String> {
    None
}

// ---------------------------------------------------------------- the network

/// the host of a net.connect / net.listen rule (the browser keeps the rules unused)
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Host {
    Any,
    /// a DNS name in lower case; "*.example.org": every name below example.org
    Name(String),
    /// an address block: 16 bytes (IPv4 as ::ffff:a.b.c.d) and the prefix bits
    Block([u8; 16], u8),
    /// 127.0.0.0/8 and ::1
    Loopback,
}

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct NetRule {
    host: Host,
    lo: u16,
    hi: u16,
}

#[cfg(any(windows, target_os = "linux"))]
const RESOLVED_MAX: usize = 4096;
#[cfg(any(windows, target_os = "linux"))]
const V6_LOOPBACK: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];

impl Host {
    fn has_name(&self, h: &str) -> bool {
        match self {
            Host::Any => true,
            Host::Name(n) => match n.strip_prefix("*.") {
                Some(base) => h.len() > base.len() + 1 && h.ends_with(base) && h.as_bytes()[h.len() - base.len() - 1] == b'.',
                None => h == n,
            },
            _ => false,
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    fn has_ip(&self, ip: &[u8; 16]) -> bool {
        match self {
            Host::Any => true,
            Host::Name(_) => false,
            Host::Block(net, bits) => prefix_eq(ip, net, *bits),
            Host::Loopback => prefix_eq(ip, &mapped([127, 0, 0, 0]), 104) || *ip == V6_LOOPBACK,
        }
    }
}

fn mapped(v4: [u8; 4]) -> [u8; 16] {
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, v4[0], v4[1], v4[2], v4[3]]
}

#[cfg(any(windows, target_os = "linux"))]
fn prefix_eq(a: &[u8; 16], b: &[u8; 16], bits: u8) -> bool {
    let (whole, rest) = ((bits / 8) as usize, bits % 8);
    if a[..whole] != b[..whole] {
        return false;
    }
    rest == 0 || (a[whole] ^ b[whole]) & (0xFFu8 << (8 - rest)) == 0
}

// "HOST:PORT" of the manifest
fn net_rule(a: &str, listen: bool) -> Result<NetRule, String> {
    let err = || alloc::format!("{} is not HOST:PORT (example.org:443, *.example.org:*, 10.0.0.0/8:22, [::1]:8080, *:*)", a);
    let k = a.rfind(':').ok_or_else(err)?;
    let (host, port) = (&a[..k], &a[k + 1..]);
    let num = |s: &str| if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) { s.parse::<u16>().ok() } else { None };
    let (lo, hi) = if port == "*" {
        (0, u16::MAX)
    } else if let Some((x, y)) = port.split_once('-') {
        match (num(x), num(y)) {
            (Some(x), Some(y)) if x <= y => (x, y),
            _ => return Err(err()),
        }
    } else {
        let n = num(port).ok_or_else(err)?;
        (n, n)
    };
    let host = if host == "*" {
        Host::Any
    } else if host.eq_ignore_ascii_case("localhost") {
        Host::Loopback
    } else {
        let (h, bits) = match host.rsplit_once('/') {
            Some((h, b)) if !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()) => (h, Some(b.parse::<u32>().map_err(|_| err())?)),
            Some(_) => return Err(err()),
            None => (host, None),
        };
        let bracketed = h.len() >= 2 && h.starts_with('[') && h.ends_with(']');
        let bare = if bracketed { &h[1..h.len() - 1] } else { h };
        if !bracketed && bare.contains(':') {
            return Err(alloc::format!("{}: an IPv6 address needs brackets ([::1]:80)", a));
        }
        match ip16(bare) {
            Some((ip, v4)) => {
                let max = if v4 { 32 } else { 128 };
                let b = bits.unwrap_or(max);
                if b > max || (v4 && bracketed) {
                    return Err(err());
                }
                Host::Block(ip, (b + if v4 { 96 } else { 0 }) as u8)
            }
            None if bracketed || bits.is_some() => return Err(err()),
            None if listen => return Err(alloc::format!("{}: net.listen takes an address, a block, localhost or *", a)),
            None => {
                let n = bare.trim_end_matches('.').to_ascii_lowercase();
                let labels = n.strip_prefix("*.").unwrap_or(&n);
                let ok = !labels.is_empty()
                    && labels.split('.').all(|l| !l.is_empty() && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
                if !ok {
                    return Err(err());
                }
                Host::Name(n)
            }
        }
    };
    Ok(NetRule { host, lo, hi })
}

// an IPv4 / IPv6 address -> 16 bytes, is IPv4
#[cfg(any(windows, target_os = "linux"))]
fn ip16(s: &str) -> Option<([u8; 16], bool)> {
    match crate::net::IpAddr::parse(s)? {
        crate::net::IpAddr::V4(a) => Some((mapped(a), true)),
        crate::net::IpAddr::V6(a) => Some((a, false)),
    }
}

// the browser has no sockets: a dotted IPv4 address (numeric labels) still parses, the rules
// are never asked
#[cfg(not(any(windows, target_os = "linux")))]
fn ip16(s: &str) -> Option<([u8; 16], bool)> {
    let p: Vec<&str> = s.split('.').collect();
    let v: Vec<u8> = p.iter().filter_map(|x| x.parse::<u8>().ok()).collect();
    if p.len() == 4 && v.len() == 4 { Some((mapped([v[0], v[1], v[2], v[3]]), true)) } else if s.contains(':') { Some(([0; 16], false)) } else { None }
}

#[cfg(any(windows, target_os = "linux"))]
fn to16(ip: &crate::net::IpAddr) -> [u8; 16] {
    match ip {
        crate::net::IpAddr::V4(a) => mapped(*a),
        crate::net::IpAddr::V6(a) => *a,
    }
}

/// whether a TCP connection or a UDP datagram to addr is allowed: a net.connect rule with
/// that address, an address a permitted name resolved to, or port 53 (DNS) with name rules
#[cfg(any(windows, target_os = "linux"))]
pub fn net_connect_allowed(addr: &crate::net::SocketAddr) -> bool {
    if !active() {
        return true;
    }
    let ip = to16(&addr.ip);
    let port = addr.port;
    let ok = POLICY.lock().as_ref().is_none_or(|p| {
        p.connect.iter().any(|r| (r.lo..=r.hi).contains(&port) && r.host.has_ip(&ip))
            || p.resolved.iter().any(|(a, lo, hi)| *a == ip && (*lo..=*hi).contains(&port))
            || (port == 53 && p.connect.iter().any(|r| matches!(r.host, Host::Any | Host::Name(_))))
    });
    ok || deny("net.connect", &alloc::format!("{}", addr))
}

/// whether a TCP listener (or a UDP socket with a fixed port) may bind addr
#[cfg(any(windows, target_os = "linux"))]
pub fn net_listen_allowed(addr: &crate::net::SocketAddr) -> bool {
    if !active() {
        return true;
    }
    let ip = to16(&addr.ip);
    let ok = POLICY.lock().as_ref().is_none_or(|p| p.listen.iter().any(|r| (r.lo..=r.hi).contains(&addr.port) && r.host.has_ip(&ip)));
    ok || deny("net.listen", &alloc::format!("{}", addr))
}

/// whether host may be resolved: a net.connect rule names it (or allows every host)
pub fn net_name_allowed(host: &str) -> bool {
    if !active() {
        return true;
    }
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    let ok = POLICY.lock().as_ref().is_none_or(|p| p.connect.iter().any(|r| r.host.has_name(&h)));
    ok || deny("net.connect", host)
}

/// (resolvers) host resolved to addrs: connections to them get the ports of host's rules
#[cfg(any(windows, target_os = "linux"))]
pub fn net_resolved(host: &str, addrs: &[crate::net::SocketAddr]) {
    if !active() {
        return;
    }
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    let mut g = POLICY.lock();
    let Some(p) = g.as_mut() else { return };
    let ranges: Vec<(u16, u16)> =
        p.connect.iter().filter(|r| matches!(r.host, Host::Name(_)) && r.host.has_name(&h)).map(|r| (r.lo, r.hi)).collect();
    for a in addrs {
        let ip = to16(&a.ip);
        for &(lo, hi) in &ranges {
            if !p.resolved.contains(&(ip, lo, hi)) {
                if p.resolved.len() == RESOLVED_MAX {
                    p.resolved.remove(0);
                }
                p.resolved.push((ip, lo, hi));
            }
        }
    }
}

// ---------------------------------------------------------------- paths

/// path as an absolute, normalised path of this OS (relative paths against cwd()), or None
/// for a path the policy refuses
pub fn normalize(path: &str) -> Option<String> {
    norm(path, cfg!(windows), false, &crate::cwd)
}

/// normalize with the rules of Windows (windows = true) or Linux against the folder cwd
pub fn normalize_for(path: &str, windows: bool, cwd: &str) -> Option<String> {
    norm(path, windows, false, &|| String::from(cwd))
}

// glob: '*' and '?' are pattern characters (manifest), not refused names
fn norm(path: &str, win: bool, glob: bool, cwd: &dyn Fn() -> String) -> Option<String> {
    if path.is_empty() || path.contains('\0') {
        return None;
    }
    if !win {
        let full = if path.starts_with('/') {
            String::from(path)
        } else {
            let c = cwd();
            if !c.starts_with('/') {
                return None;
            }
            alloc::format!("{}/{}", c, path)
        };
        if full.split('/').any(|s| s == "..") {
            return None; // resolved after symbolic links by the kernel, not lexically
        }
        let segs = segments(&full, false, glob)?;
        return Some(alloc::format!("/{}", segs.join("/")));
    }
    let mut s: String = path.replace('\\', "/");
    // \\?\ and \\.\ prefixes: only drive and UNC paths
    for pre in ["//?/", "//./", "/??/"] {
        if let Some(rest) = s.strip_prefix(pre) {
            if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case("UNC/") {
                s = alloc::format!("//{}", &rest[4..]);
            } else if is_drive(rest) {
                s = String::from(rest);
            } else {
                return None;
            }
            break;
        }
    }
    let (root, rest) = if let Some(unc) = s.strip_prefix("//") {
        let mut it = unc.splitn(3, '/');
        let server = it.next().filter(|x| !x.is_empty())?;
        let share = it.next().filter(|x| !x.is_empty())?;
        if !name_ok(server, glob) || !name_ok(share, glob) {
            return None;
        }
        (alloc::format!("//{}/{}", server, share), String::from(it.next().unwrap_or("")))
    } else if is_drive(&s) {
        let d = s.as_bytes()[0].to_ascii_uppercase() as char;
        match s.as_bytes().get(2) {
            None => (alloc::format!("{}:", d), String::new()),
            Some(b'/') => (alloc::format!("{}:", d), String::from(&s[3..])),
            Some(_) => return None, // "C:file": relative to the drive's own current folder
        }
    } else {
        let c = cwd();
        if c.is_empty() || !(c.starts_with(['/', '\\']) || is_drive(&c)) {
            return None;
        }
        let base = norm(&c, true, false, &|| String::new())?;
        if s.starts_with('/') {
            // rooted: the drive (or share) of the current folder
            let root = match base.strip_prefix("//") {
                Some(u) => {
                    let mut it = u.splitn(3, '/');
                    alloc::format!("//{}/{}", it.next()?, it.next()?)
                }
                None => String::from(&base[..2]),
            };
            return norm(&alloc::format!("{}{}", root, s), true, glob, cwd);
        }
        return norm(&alloc::format!("{}/{}", base.trim_end_matches('/'), s), true, glob, cwd);
    };
    let segs = segments(&rest, true, glob)?;
    if segs.is_empty() {
        return Some(if root.starts_with("//") { root } else { alloc::format!("{}/", root) });
    }
    Some(alloc::format!("{}/{}", root, segs.join("/")))
}

// "X:" at the start (absolute "X:/..", or the drive-relative "X:file" that norm refuses)
fn is_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

// "." / "" dropped, ".." removes the previous segment (None above the root); Windows: the
// trailing dots and blanks Win32 strips, no stream, device or forbidden names
fn segments(rest: &str, win: bool, glob: bool) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for seg in rest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            _ if win => {
                let t = seg.trim_end_matches(['.', ' ']);
                if t.is_empty() || !name_ok(t, glob) {
                    return None;
                }
                out.push(String::from(t));
            }
            _ => out.push(String::from(seg)),
        }
    }
    Some(out)
}

// a Windows file name: no control characters, no < > : " | (and * ? outside patterns), no
// device name (CON, NUL, COM1 .., also with an extension)
fn name_ok(name: &str, glob: bool) -> bool {
    if name.chars().any(|c| (c as u32) < 0x20 || "<>:\"|".contains(c) || (!glob && "*?".contains(c))) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").trim_end_matches(' ');
    let u = stem.to_ascii_uppercase();
    let dev = matches!(u.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || (u.len() == 4 && (u.starts_with("COM") || u.starts_with("LPT")) && u.as_bytes()[3].is_ascii_digit())
        || (u.len() == 5 && (u.starts_with("COM") || u.starts_with("LPT")) && ["¹", "²", "³"].contains(&&stem[3..]));
    !dev
}

// the form rules and paths are compared in: Windows without case
fn fold(s: &str) -> String {
    if cfg!(windows) { s.chars().flat_map(char::to_lowercase).collect() } else { String::from(s) }
}

fn split(n: &str) -> Vec<String> {
    let mut v: Vec<String> = n.split('/').map(String::from).collect();
    if v.len() > 1 && v.last().is_some_and(|s| s.is_empty()) {
        v.pop(); // "C:/" -> ["c:"]
    }
    v
}

// pattern segments against path segments ("**" = any number of segments)
fn seg_match(p: &[String], s: &[String]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(f) if f == "**" => (0..=s.len()).any(|k| seg_match(&p[1..], &s[k..])),
        Some(f) => !s.is_empty() && glob(f.as_bytes(), s[0].as_bytes()) && seg_match(&p[1..], &s[1..]),
    }
}

// '*' any run, '?' one character, inside one segment (bytes; '?' takes one UTF-8 byte)
fn glob(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && p[pi] == b'*' {
            pi += 1;
            star = Some((pi, si));
        } else if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// whether pattern (manifest syntax after variables) matches path; both normalised with the
/// rules of this OS (tests)
pub fn pattern_matches(pattern: &str, path: &str) -> bool {
    let Ok(pats) = self::pattern(pattern) else { return false };
    let Some(n) = normalize(path) else { return false };
    let segs = split(&fold(&n));
    pats.iter().any(|p| seg_match(p, &segs))
}
