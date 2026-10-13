// rt -- the runtime under everything: allocator, memory intrinsics, exact SSE float
// semantics, the minimal OS calls the runtime itself needs, panic handling.
//
//   rt::os::{pages_alloc, pages_free, write_stdout, write_stderr, exit, yield_now}   (and on
//   Linux the raw syscall helpers plat builds on)
//   rt::fx   cvtt / cvt / sqrt / minss / maxss / floor_i / ceil_i / div255
//   rt::x87  the extended-precision steps of f64 <-> decimal text (ini.s), fpatan
//   rt::sync SpinLock
//   rt::cpu  CPU feature bits (CPUID, cached; force_baseline for --simd sse2)
//   rt::crash the crash hook, CrashInfo, report (exit 3) -- also for panics
//
// One of the four crates that may contain `unsafe` (rt, simd, plat, plugin).
#![no_std]

extern crate alloc;

/// Define the common app entry symbol without requiring the GUI crate in headless tools.
#[macro_export]
macro_rules! main {
    ($body:expr) => {
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        fn jfgui_main() -> i32 { $body }
    };
}

#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), path = "portable/cpu.rs")]
pub mod cpu;
pub mod crash;
#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), path = "portable/fx.rs")]
pub mod fx;
#[cfg(not(target_os = "android"))]
#[cfg_attr(target_arch = "wasm32", path = "portable/mem.rs")]
mod mem;
#[cfg_attr(target_arch = "wasm32", path = "portable/os.rs")]
#[cfg_attr(target_os = "android", path = "android.rs")]
pub mod os;
pub mod sync;
#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), path = "portable/math.rs")]
pub mod x87;

/// Portable arithmetic, available on native targets for exact backend comparisons.
#[cfg(not(target_arch = "wasm32"))]
pub mod portable {
    pub mod fx;
    pub mod math;
}

/// panic: one line on stderr, the crash hook (CK_PANIC), exit code 3
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::fmt::Write;
    if crash::busy() {
        os::exit(3); // a panic inside the crash hook
    }
    let mut s = crash::StackText::<1024>::new(); // no allocation on the crash path
    let _ = writeln!(s, "panic: {info}");
    os::write_stderr(s.bytes());
    let text = s.as_str().trim_end();
    let mut ci = crash::CrashInfo::empty();
    ci.kind = crash::CK_PANIC;
    ci.msg = text.strip_prefix("panic: ").unwrap_or(text);
    crash::report(&ci)
}
