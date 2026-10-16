// permtest -- the permission manifest end to end: apps/permtest/PERMISSIONS goes through
// build_rs.py (JFGUI_PERMISSIONS) into ui::main!, which installs it before this code runs
//
//   permtest FILE     FILE (named on the command line: the user's, fs.user) may be written,
//                     a neighbour of it may not; $TEMP/permtest-* may be written, the rest
//                     of $TEMP not; print and the listed URL schemes are allowed, plugins and
//                     other schemes are not.  Prints PASS / FAIL; exit code = failures.
//   (build_rs.py test runs it through tests/manifest.uis; --script is ignored)
#![no_std]
#![no_main]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::format;

use plat::policy;

ui::main!(permtest());

struct T {
    fails: i32,
}

impl T {
    fn check(&mut self, ok: bool, name: &str) {
        if !ok {
            self.fails += 1;
        }
        plat::write_stdout(format!("{} {}\n", if ok { "PASS" } else { "FAIL" }, name).as_bytes());
    }
}

fn permtest() -> i32 {
    let mut t = T { fails: 0 };
    t.check(policy::active(), "permtest: ui::main! installed the manifest");
    let args = plat::args();
    let Some(file) = args.iter().skip(1).find(|a| !a.starts_with('-') && !a.ends_with(".uis")).cloned() else {
        t.check(false, "permtest: usage: permtest FILE");
        return t.fails;
    };

    // the command line's file (fs.user), not its neighbour
    t.check(plat::write_file(&file, b"granted"), "permtest: the command line's file can be written");
    t.check(plat::read_file(&file).as_deref() == Some(&b"granted"[..]), "permtest: ... and read");
    let other = format!("{}.other", file);
    t.check(!plat::write_file(&other, b"x"), "permtest: its neighbour cannot be written");
    t.check(plat::delete(&file), "permtest: the command line's file can be deleted");

    // $TEMP/permtest-*/** only
    let temp = plat::temp_dir().unwrap_or_default();
    let dir = format!("{}/permtest-{:x}", temp, plat::time_us() & 0xFF_FFFF);
    t.check(plat::mkdir(&dir), "permtest: mkdir $TEMP/permtest-*");
    let f = format!("{dir}/a.txt");
    t.check(plat::write_file(&f, b"temp") && plat::read_file(&f).is_some(), "permtest: write and read below it");
    t.check(plat::delete(&f) && plat::delete(&dir), "permtest: delete below it");
    t.check(!plat::write_file(&format!("{}/other-permtest.txt", temp), b"x"), "permtest: the rest of $TEMP is refused");

    // always readable: the exe folder; nothing else of the repository
    let exe = plat::current_exe().unwrap_or_default();
    t.check(plat::file_exists(&exe) == 1, "permtest: the exe folder is readable");
    t.check(plat::read_file("README.md").is_none(), "permtest: other files are refused");

    // the other permissions
    t.check(policy::url_allowed("mailto:a@b") && !policy::url_allowed("file:///etc/passwd"), "permtest: url https mailto");
    let caps = plat::capabilities();
    t.check(caps.supports(plat::CAP_PRINT | plat::CAP_FILE_WRITE), "permtest: print and files in capabilities()");
    t.check(!caps.supports(plat::CAP_NATIVE_PLUGINS), "permtest: no plugins in capabilities()");
    let d = policy::denials();
    t.check(d.iter().any(|x| x.kind == "fs.write" && x.target == other), "permtest: the refusal is recorded");
    plat::write_stdout(format!("permtest: {} failure(s)\n", t.fails).as_bytes());
    t.fails
}
