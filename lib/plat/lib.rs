// plat -- the platform layer: one module per OS with the same functions  [src/plat]
//
//   process: entry point (calls the app's jfgui_main), args(), arg_value(name), exit(code),
//            write_stdout / write_stderr, time_us(), sleep_ms(ms), date()
//   environment: env(name) (= getenv), setenv(name, Some / None), config_dir(), temp_dir(), cwd()
//   files:   read_file(path), write_file(path, data) (native: exclusive sibling + rename),
//            File::open(path, FO_*) / open_temp(dir) -> read_at / write_at / size / sync,
//            file_exists(path) (0 / 1 file / 2 dir), mkdir (with parents), delete, rename,
//            file_exists_checked(path) (Some(0) missing, None error), FileLock::acquire(path),
//            dir_list(path, f(name, is_dir, size_lo))
//   threads: thread_spawn(stack, f) -> Thread (join), thread_yield(), Sem (post / wait),
//            wake() (any thread: E_WAKE on the UI thread), wait_events(deadline, on_event)
//            (headless), cpu_count(), mem_total()
//   pool:    thr_init / thr_count / thr_shutdown / thr_self, parallel_for, Job   (pool.rs)
//   sync:    Mutex, Condvar, RwLock, channel (futex / WaitOnAddress)          (sync.rs)
//   rand:    fill_random, random_bytes, random_u64 (the OS CSPRNG)             (rand.rs)
//   net:     TcpListener, TcpStream, UdpSocket, SocketAddr, resolve            (net.rs)
//   fs:      stat / lstat / read_dir / links / set_times / free_space /
//            write_file_private / local_zone_id                                (fs.rs)
//   cpu:     cpu_features() (CPU_*), cpu_brand()                                 (cpu.rs)
//   crash:   crash_hook(Some(f)): f(&CrashInfo) before exit code 3 -- hardware faults (VEH /
//            signals, the stderr line of ASMLIB) and panics; crash_test() crashes on purpose
//   windows: the window-indexed API of the assembly (win_create / win_show / win_present /
//            wait ..., popups, clipboard, drops, pen, cursors; consts.rs, evq.rs, pwin0.rs)
//   (plat-shared) print(job) + page layout / PDF writer (print.rs; win_print.rs,
//            linux_print.rs), plugin libraries lib_open / lib_sym / lib_close / lib_seg and
//            the crash guard guard_call / guard_run / guard_tag (plib.rs; win_lib.rs,
//            linux_elfload.rs, linux_guard.rs), monitor_icc / monitor_key (icc.rs), the
//            portal's file dialog / OpenURI / Print (linux_portal.rs)
//   (jxl-module) the loadable JPEG XL writer module: jxlenc_set_path / jxlenc_ready /
//            jxlenc_encode (modhost.rs; the records in modabi.rs)
//
// The layer knows events, pixel rows, files and threads only -- no UI, no gfx.  One of the
// four crates that may contain `unsafe` (rt, simd, plat, plugin); nothing unsafe leaves it.
#![no_std]

extern crate alloc;

/// Owned native child processes with pipes and process-tree lifetime management.
#[cfg(any(windows, target_os = "linux"))]
pub mod process;
/// Mutex, Condvar, RwLock and bounded channels for worker threads.
#[cfg(any(windows, target_os = "linux"))]
pub mod sync;
/// TCP and UDP sockets (IPv4 / IPv6) with timeouts, numeric names.
#[cfg(any(windows, target_os = "linux"))]
pub mod net;
/// Metadata, directory listings, links, times, free space, owner-only files, the local zone.
#[cfg(any(windows, target_os = "linux"))]
pub mod fs;
/// Running as a system service (Windows service control manager, systemd units).
#[cfg(any(windows, target_os = "linux"))]
pub mod service;
/// Changes below folders as they happen (continuous data protection).
#[cfg(any(windows, target_os = "linux"))]
pub mod watch;
/// The operating system's trusted certificate authorities (TLS clients of public servers).
#[cfg(any(windows, target_os = "linux"))]
pub mod certs;
#[cfg(any(windows, target_os = "linux"))]
mod rand;
#[cfg(any(windows, target_os = "linux"))]
pub use rand::*;

use alloc::string::String;

pub use rt::os::{exit, write_stderr, write_stdout};

mod cpu;
mod capabilities;
pub use capabilities::*;
mod control_stream;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub use control_stream::*;
#[cfg(any(windows, target_os = "linux"))]
pub mod control_ipc;
#[cfg_attr(all(any(target_arch = "wasm32", target_os = "android"), not(web_threads)), path = "web_pool.rs")]
mod pool;
pub use cpu::*;
pub use pool::*;

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

/// Host-driven boundary shared by browser workers and the native Android adapter.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub mod web;
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub use web::*;

mod clipdrop;
// clipboard_platform: reusable rich clipboard framing.
mod cliprich;
pub use cliprich::*;
mod consts;
mod evq;
mod ime;
pub use ime::*;
#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), path = "web_icc.rs")]
mod icc;
mod pwin0;
pub use clipdrop::*;
pub use consts::*;
pub use evq::*;
pub use icc::*;
pub use pwin0::*;
// plat-shared: printing (print.rs + win_print.rs / linux_print.rs), plugin libraries and the
// crash guard (plib.rs + win_lib.rs / linux_elfload.rs, linux_guard.rs)
#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), path = "web_lib.rs")]
mod plib;
mod print;
pub use plib::*;
pub use print::*;
// jxl-module: loadable modules, the host side (the JPEG XL writer jxlenc; modhost.rs, modabi.rs)
#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), path = "web_modhost.rs")]
mod modhost;
pub use modhost::*;

/// one platform event (the EV_* record): type, button, modifiers, window index (0 = the
/// main window), position (px; E_RESIZE: size), data
#[derive(Clone, Copy, Default)]
pub struct Event {
    pub typ: u8,
    pub btn: u8,
    pub mods: u8,
    pub win: u8,
    pub x: i32,
    pub y: i32,
    pub data: u32,
}

/// the pixels to present: rows of 32-bit B, G, R, x
pub struct Frame<'a> {
    pub px: &'a [u8],
    pub w: i32,
    pub h: i32,
    pub stride: i32,
}

// ---- files
pub const FO_READ: u32 = 0; // existing file, read only
pub const FO_WRITE: u32 = 1; // create or truncate, read + write
pub const FO_RW: u32 = 2; // open or create (contents kept), read + write

/// a date and time (date(): Windows local time, Linux UTC)
#[derive(Clone, Copy, Default, Debug)]
pub struct Date {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub min: u16,
    pub sec: u16,
    pub ms: u16,
    pub wday: u16, // 0 = Sunday
}

/// the argument after `name` on the command line (argument 0, the program, is skipped)  [arg_value]
pub fn arg_value(name: &str) -> Option<String> {
    let a = args();
    let i = a.iter().skip(1).position(|x| x == name)? + 1;
    a.get(i + 1).cloned()
}

// ---- crashes (docs/modules/CRASH.md)
pub use rt::crash::{CK_ACCESS, CK_BUS, CK_DIVIDE, CK_ILLEGAL, CK_OTHER, CK_PANIC, CK_STACK, CrashHook, CrashInfo};

/// f(&CrashInfo) runs in the crash handler, after the stderr line, before exit code 3; also
/// for panics (CK_PANIC)  [plat_crash_hook]
pub fn crash_hook(f: Option<CrashHook>) {
    rt::crash::set_hook(f)
}

/// a deliberate crash for tests: `mov eax, 16` (5 bytes), then a 4-byte write to address 0x10
/// -- the faulting instruction is at crash_test + 5
#[unsafe(naked)]
#[cfg(target_arch = "x86_64")]
pub extern "C" fn crash_test() -> ! {
    core::arch::naked_asm!("mov eax, 16", "mov dword ptr [rax], eax", "ud2")
}

/// (plat-win) crash_test called from a known place (call rel32): the return address
/// crash_test_caller + 5 must be among CrashInfo::frames (plattest's call chain check)
#[unsafe(naked)]
#[cfg(target_arch = "x86_64")]
pub extern "C" fn crash_test_caller() -> ! {
    core::arch::naked_asm!("call {0}", "ud2", sym crash_test)
}

/// Deliberate trap for browser runtime diagnostics.
#[cfg(target_arch = "wasm32")]
pub extern "C" fn crash_test() -> ! { core::arch::wasm32::unreachable() }
/// Deliberate trap through the compatibility caller API.
#[cfg(target_arch = "wasm32")]
pub extern "C" fn crash_test_caller() -> ! { crash_test() }

/// Deliberate termination on Android, where no hardware exception translator is installed.
#[cfg(all(target_os = "android", not(target_arch = "x86_64")))]
pub extern "C" fn crash_test() -> ! { rt::os::exit(3) }
/// Compatibility caller for the Android diagnostic termination.
#[cfg(all(target_os = "android", not(target_arch = "x86_64")))]
pub extern "C" fn crash_test_caller() -> ! { crash_test() }
