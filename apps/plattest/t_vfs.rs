// t_vfs -- plattest --test vfs: one file namespace (lib/plat/vfs.rs, guarded.rs)
//
//   names and routes (OS paths stay OS paths, "..", unknown schemes, roots), mem:// through
//   the ordinary file functions, a DirFs mount on a scratch folder (also File, FileLock,
//   base::Ini and os_path), JFA1 bundles (pack_bundle, LZ4, damaged bundles), Overlay, Sub,
//   archive::ZipFs, the app's folders (app_id, app_dir, config://)

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use plat::vfs::{self, Bundle, DirFs, MemFs, Overlay, Provider, Route, Sub};
use plat::{FO_READ, FO_WRITE, File, FileLock};

use crate::{T, rm_tree};

fn names(path: &str) -> Vec<String> {
    let mut v = Vec::new();
    plat::dir_list(path, &mut |n, d, _| v.push(if d { format!("{n}/") } else { String::from(n) }));
    v.sort();
    v
}

pub fn t_vfs(t: &mut T) {
    routes(t);
    mem(t);
    dirfs(t);
    bundles(t);
    layers(t);
    zip(t);
    app_folders(t);
}

fn routes(t: &mut T) {
    let os = |n: &str| matches!(vfs::route(n), Route::Os);
    let bad = |n: &str| matches!(vfs::route(n), Route::Bad);
    t.check(os("C:/x") && os("c:\\x") && os("/a/b:c") && os("notes:2024/x") && os("rel/x"), "vfs: OS paths stay OS paths");
    t.check(bad("nosuch://x") && bad("mem://a/../b") && bad("mem://a/\0"), "vfs: unknown schemes and .. are refused");
    t.check(matches!(vfs::route("mem:"), Route::Vfs(_, ref p) if p.is_empty()), "vfs: mem: is the root");
    t.check(matches!(vfs::route("mem:/a//./b\\c"), Route::Vfs(_, ref p) if p == "a/b/c"), "vfs: paths are normalised");
    t.check(!vfs::mount("X", Arc::new(MemFs::new())) && !vfs::mount("a", Arc::new(MemFs::new())), "vfs: invalid schemes cannot be mounted");
    t.check(plat::read_file("https://example.org/").is_none(), "vfs: a URL is no file");
    t.check(vfs::provider("mem").is_some() && vfs::provider("nosuch").is_none(), "vfs: provider of a mount");
}

fn mem(t: &mut T) {
    t.check(plat::write_file("mem://a/b/c.txt", b"abc"), "vfs: mem:// write creates folders");
    t.check(plat::read_file("mem://a/b/c.txt").as_deref() == Some(&b"abc"[..]), "vfs: mem:// read");
    t.expect_i(plat::file_exists("mem://a/b") as i64, 2, "vfs: mem:// folder");
    t.expect_i(plat::file_exists("mem://a/b/c.txt") as i64, 1, "vfs: mem:// file");
    t.expect_i(plat::file_exists("mem://a/x") as i64, 0, "vfs: mem:// missing");
    t.check(plat::file_exists_checked("mem://a/x") == Some(0), "vfs: mem:// file_exists_checked");
    t.check(plat::mkdir("mem://d/e") && names("mem://") == ["a/", "d/"], "vfs: mem:// mkdir and listing");
    t.check(!plat::delete("mem://a/b"), "vfs: a folder with files is not deleted");
    t.check(plat::rename("mem://a/b/c.txt", "mem://d/c2.txt") && names("mem://d") == ["c2.txt", "e/"], "vfs: mem:// rename");
    t.check(plat::delete("mem://d/c2.txt") && plat::delete("mem://d/e") && plat::delete("mem://d"), "vfs: mem:// delete");
    t.check(!plat::write_file("mem://", b"x") && !plat::write_file("mem://a", b"x"), "vfs: no file over the root or a folder");
    t.check(!plat::rename("mem://a/b", "config://b"), "vfs: no rename across mounts");
    t.check(File::open("mem://a/x", FO_WRITE).is_none(), "vfs: File::open needs a folder mount");
}

fn dirfs(t: &mut T) {
    let cwd = plat::cwd().replace('\\', "/");
    let base = format!("{cwd}/build/plattest/{}-vfs-{:x}", crate::OS, plat::time_us() & 0xFF_FFFF);
    t.check(vfs::mount("scratch", Arc::new(DirFs::new(&base))), "vfs: mount a DirFs");
    t.check(plat::write_file("scratch://sub/x.txt", b"dir") && plat::file_exists(&format!("{base}/sub/x.txt")) == 1, "vfs: DirFs write lands in the folder");
    t.check(plat::read_file("scratch://sub/x.txt").as_deref() == Some(&b"dir"[..]), "vfs: DirFs read");
    t.check(names("scratch://") == ["sub/"], "vfs: DirFs listing");
    t.expect_s(vfs::os_path("scratch://sub/x.txt").as_deref(), Some(&*format!("{base}/sub/x.txt")), "vfs: os_path of a DirFs name");
    let f = File::open("scratch://sub/y.bin", FO_WRITE);
    t.check(f.as_ref().is_some_and(|f| f.write_at(0, b"stream") == Some(6)), "vfs: File::open on a DirFs");
    drop(f);
    let mut buf = [0u8; 6];
    t.check(File::open("scratch://sub/y.bin", FO_READ).and_then(|f| f.read_at(0, &mut buf)) == Some(6) && &buf == b"stream", "vfs: ... and read back");
    t.check(FileLock::acquire("scratch://LOCK").is_some(), "vfs: FileLock on a DirFs");
    let mut ini = base::Ini::new();
    ini.set_int("s", "k", 42);
    t.check(ini.save("scratch://cfg/settings.ini") && base::Ini::load("scratch://cfg/settings.ini").get_int("s", "k", 0) == 42, "vfs: base::Ini through a mount");
    t.check(plat::file_exists("scratch:") == 2 && plat::file_exists(&format!("{cwd}/scratch:")) == 0, "vfs: Ini's parent folder is the mount, not a folder 'scratch:'");
    t.check(plat::rename("scratch://sub/x.txt", "scratch://moved/x.txt") && plat::file_exists("scratch://moved/x.txt") == 1, "vfs: DirFs rename creates the folder");
    t.check(vfs::unmount("scratch") && plat::read_file("scratch://moved/x.txt").is_none(), "vfs: unmount");
    rm_tree(&base);
}

fn bundles(t: &mut T) {
    let big: Vec<u8> = b"JFLib ".iter().copied().cycle().take(6000).collect();
    let files: [(&str, &[u8]); 4] = [("b.txt", b"bee"), ("a/one.txt", b"1"), ("a/deep/two.bin", &big), ("empty", b"")];
    let packed = vfs::pack_bundle(&files);
    t.check(packed.as_ref().is_some_and(|p| p.len() < 1000), "vfs: pack_bundle compresses with LZ4");
    let Some(packed) = packed else { return };
    let Some(b) = Bundle::new(packed.clone()) else {
        t.check(false, "vfs: Bundle::new");
        return;
    };
    t.expect_i(b.len() as i64, 4, "vfs: four files in the bundle");
    t.check(b.read("a/deep/two.bin").as_deref() == Some(&big[..]) && b.read("b.txt").as_deref() == Some(&b"bee"[..]), "vfs: bundle read (LZ4 and stored)");
    t.check(b.read("empty").is_some_and(|d| d.is_empty()) && b.read("a").is_none(), "vfs: an empty file; a folder is no file");
    t.check(b.kind("") == 2 && b.kind("a") == 2 && b.kind("a/deep") == 2 && b.kind("a/d") == 0 && b.kind("b.txt") == 1, "vfs: bundle kinds");
    let mut l = Vec::new();
    b.list("a", &mut |n, d, s| l.push(format!("{n}{}{s}", if d { "/" } else { ":" })));
    t.check(l == ["deep/0", "one.txt:1"], "vfs: bundle listing");
    t.check(!b.write("x", b"y") && !b.mkdir("x") && !b.delete("b.txt"), "vfs: a bundle is read-only");
    t.check(vfs::pack_bundle(&[("a", b"1"), ("a", b"2")]).is_none() && vfs::pack_bundle(&[("../x", b"1")]).is_none(), "vfs: pack_bundle refuses duplicates and ..");
    let mut bad = packed.clone();
    bad[0] = b'X';
    let mut cut = packed.clone();
    cut.truncate(packed.len() - 3);
    let mut big_size = packed.clone();
    big_size[12 + 8 + 3] = 0x40; // the first entry's data_off points far beyond the end
    t.check(Bundle::new(bad).is_none() && Bundle::new(cut).is_none() && Bundle::new(big_size).is_none(), "vfs: damaged bundles are refused");
    t.check(Bundle::new(Vec::new()).is_some_and(|b| b.is_empty()), "vfs: an empty bundle");
}

fn layers(t: &mut T) {
    let low = Bundle::new(vfs::pack_bundle(&[("theme/a.css", b"low a"), ("theme/b.css", b"low b")]).unwrap_or_default());
    let Some(low) = low else {
        t.check(false, "vfs: bundle for the overlay");
        return;
    };
    let low: Arc<dyn Provider> = Arc::new(low);
    let top: Arc<dyn Provider> = Arc::new(MemFs::new());
    t.check(top.write("theme/a.css", b"top a"), "vfs: overlay top layer");
    vfs::mount("ov", Arc::new(Overlay::new(alloc::vec![top.clone(), low.clone()])));
    t.check(plat::read_file("ov://theme/a.css").as_deref() == Some(&b"top a"[..]), "vfs: overlay reads the top layer first");
    t.check(plat::read_file("ov://theme/b.css").as_deref() == Some(&b"low b"[..]), "vfs: ... then the lower ones");
    t.check(names("ov://theme") == ["a.css", "b.css"], "vfs: overlay lists every layer once");
    t.check(plat::write_file("ov://theme/c.css", b"new") && top.read("theme/c.css").is_some(), "vfs: overlay writes go to the top layer");
    match Sub::new(low.clone(), "theme") {
        Some(s) => {
            vfs::mount("theme", Arc::new(s));
            t.check(plat::read_file("theme://b.css").as_deref() == Some(&b"low b"[..]) && names("theme://") == ["a.css", "b.css"], "vfs: Sub shows one folder");
        }
        None => t.check(false, "vfs: Sub::new"),
    }
    t.check(Sub::new(low, "../x").is_none(), "vfs: Sub refuses ..");
    vfs::unmount("ov");
    vfs::unmount("theme");
}

fn zip(t: &mut T) {
    let entries = [
        archive::Entry { name: String::from("doc/readme.txt"), data: b"zip text".to_vec() },
        archive::Entry { name: String::from("img/a.bin"), data: alloc::vec![7u8; 3000] },
    ];
    let Ok(z) = archive::write(&entries, archive::Limits::default()) else {
        t.check(false, "vfs: archive::write");
        return;
    };
    match archive::ZipFs::new(&z, archive::Limits::default()) {
        Ok(fs) => {
            vfs::mount("pack", Arc::new(fs));
            t.check(plat::read_file("pack://doc/readme.txt").as_deref() == Some(&b"zip text"[..]), "vfs: ZipFs read");
            t.check(names("pack://") == ["doc/", "img/"] && plat::file_exists("pack://img/a.bin") == 1, "vfs: ZipFs listing");
            t.check(!plat::write_file("pack://doc/x", b"1"), "vfs: ZipFs is read-only");
            vfs::unmount("pack");
        }
        Err(_) => t.check(false, "vfs: ZipFs::new"),
    }
    t.check(archive::ZipFs::new(b"not a zip at all, sorry", archive::Limits::default()).is_err(), "vfs: ZipFs refuses a damaged archive");
}

fn app_folders(t: &mut T) {
    t.expect_s(Some(vfs::app_id().as_str()), Some("plattest"), "vfs: app_id from the build (JFGUI_APP)");
    let cfg = vfs::app_dir(vfs::APP_CONFIG).unwrap_or_default();
    let want = format!("{}/plattest", plat::config_dir().unwrap_or_default());
    t.expect_s(Some(cfg.as_str()), Some(want.as_str()), "vfs: app_dir(APP_CONFIG)");
    let cache = vfs::app_dir(vfs::APP_CACHE).unwrap_or_default();
    t.check(cache.ends_with(if cfg!(windows) { "/plattest/Cache" } else { "/plattest" }), "vfs: app_dir(APP_CACHE)");
    t.expect_s(vfs::os_path("config://x.ini").as_deref(), Some(&*format!("{cfg}/x.ini")), "vfs: config:// is the app's settings folder");
    let name = format!("config://plattest-vfs-{:x}.txt", plat::time_us() & 0xFF_FFFF);
    t.check(plat::write_file(&name, b"cfg") && plat::read_file(&name).as_deref() == Some(&b"cfg"[..]) && plat::delete(&name), "vfs: write, read and delete in config://");
    t.check(plat::file_exists("app://") == 2 && names("app://").is_empty(), "vfs: app:// without assets is empty");
}
