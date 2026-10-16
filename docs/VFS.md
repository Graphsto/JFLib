# One file namespace (`plat::vfs`) and bundled assets

Every file function of `plat` also accepts names of the form `scheme://path`: `read_file`,
`write_file`, `file_exists`, `file_exists_checked`, `mkdir`, `delete`, `rename` and `dir_list`.
Everything built on them gets the virtual names for free, for example `img_core::io::load`
and `doc_io`'s stores. Code that opens files as streams (`plat::File`) works on the folder
mounts (`config://`, `data://`, `cache://`, `DirFs`): `base::Ini::load`, the font registry's
`load_file`. Names without a scheme stay paths of the OS, or of the browser's virtual files. The same code runs on Windows, Linux, the web and
Android.

```rust
let logo = plat::read_file("app://images/logo.png");          // built into the program
let mut ini = base::Ini::load("config://settings.ini");      // %APPDATA%/MyTool, ~/.config/MyTool
ini.set_int("view", "zoom", 150);
ini.save("config://settings.ini");                           // the folder is created
plat::write_file("cache://thumbs/1.png", &png);
plat::write_file("mem://scratch.txt", b"only in memory");
```

| Part | Where |
| --- | --- |
| Mounts, providers, bundle format | [`lib/plat/vfs.rs`](../lib/plat/vfs.rs) |
| Routing in the file functions | [`lib/plat/guarded.rs`](../lib/plat/guarded.rs) |
| ZIP archives as a tree | `archive::ZipFs` in [`lib/archive/lib.rs`](../lib/archive/lib.rs) |
| Packing `apps/NAME/assets` | [`tools/assetpack.py`](../tools/assetpack.py), `app_assets_hash` in [`build_rs.py`](../build_rs.py) |
| Mounting at start-up | `ui::boot` in [`lib/ui/perm.rs`](../lib/ui/perm.rs) (called by `ui::main!`) |
| Tests | `plattest --test vfs` ([`t_vfs.rs`](../apps/plattest/t_vfs.rs)), [`apps/permtest`](../apps/permtest/main.rs) with its `assets/` |

## Built-in schemes

| Scheme | Contents | Windows | Linux | Browser |
| --- | --- | --- | --- | --- |
| `app://` | the app's assets (read-only) | inside the program | inside the program | inside the program |
| `config://` | settings | `%APPDATA%/<app>` | `$XDG_CONFIG_HOME/<app>` (`~/.config`) | `/config/<app>` |
| `data://` | data of the app | `%LOCALAPPDATA%/<app>` | `$XDG_DATA_HOME/<app>` (`~/.local/share`) | `/data/<app>` |
| `cache://` | data that can be rebuilt | `%LOCALAPPDATA%/<app>/Cache` | `$XDG_CACHE_HOME/<app>` (`~/.cache`) | `/cache/<app>` |
| `mem://` | files in memory, gone at exit | – | – | – |

`<app>` is `plat::vfs::app_id()`. `build_rs.py` sets it to the app's folder name
(`JFGUI_APP`). Otherwise it is the program's file name. `plat::vfs::app_dir(APP_CONFIG |
APP_DATA | APP_CACHE)` returns these folders. `plat::data_dir()` and `plat::cache_dir()`
return the base folders, like `config_dir()`.

The built-in schemes are mounted when they are first used. `config://`, `data://` and
`cache://` are folders of the OS: `vfs::os_path("config://a.ini")` returns the real path, and
`File::open`, `File::open_temp` and `FileLock::acquire` work on them.

## Paths inside a mount

- `/` separates, `\` is turned into `/`, and `.` and empty segments are dropped.
- `..` is refused: the name leads nowhere, and every call fails.
- `config:`, `config:/` and `config://` all name the root. Code that cuts a name at its last
  `/`, like `base::Ini` creating the parent folder, therefore stays inside the mount.
- A scheme has at least two characters: lower-case letters, digits, `+`, `-` and `.`.
  Windows drive letters (`C:/`) are never schemes.
- A scheme that is not mounted is an error, not a relative OS path. `https://` is no file.
- Writes create missing folders.
- `rename` works inside one mount. Across mounts (or between a mount and the OS) it fails;
  read and write instead.

## Mounting your own providers

```rust
use alloc::sync::Arc;
use plat::vfs::{self, DirFs, MemFs, Overlay, Sub};

// a project folder under its own scheme
vfs::mount("proj", Arc::new(DirFs::new("D:/Projects/Book")));

// user themes over the built-in ones: user files win, writes go to the user folder
let user = Arc::new(DirFs::new(&format!("{}/themes", vfs::app_dir(vfs::APP_DATA).unwrap())));
let builtin = Arc::new(Sub::new(vfs::provider("app").unwrap(), "themes").unwrap());
vfs::mount("theme", Arc::new(Overlay::new(vec![user, builtin])));

// a ZIP archive, read-only
let zip = archive::ZipFs::new(&bytes, archive::Limits::default())?;
vfs::mount("pack", Arc::new(zip));
let tex = plat::read_file("pack://textures/wood.png");
```

| Provider | Use |
| --- | --- |
| `MemFs::new()` | files in memory (also behind `mem://`) |
| `DirFs::new(root)` | an OS folder; goes through the checked OS functions |
| `Overlay::new(layers)` | several layers: reads from the first layer that has the path, lists every layer, writes to the top layer. A file of a lower layer cannot be deleted |
| `Sub::new(provider, folder)` | one folder of another provider as a root |
| `Bundle::new(bytes)` / `from_static` | a JFA1 bundle (read-only) |
| `archive::ZipFs::new(bytes, limits)` | a ZIP32 archive (stored / DEFLATE), checked and unpacked once |

Your own types implement `plat::vfs::Provider`: `kind`, `read` and `list`, and optionally
`write`, `mkdir`, `delete`, `rename` and `os_path`. `mount` replaces an earlier mount of the
same scheme. `unmount` removes it. `provider(scheme)` returns a mount's provider, for example
to build a layer from `app://`.

## Assets (`apps/NAME/assets`)

1. Put the files into `apps/NAME/assets/`. Folders are kept; names starting with `.` are
   left out.
2. `python build_rs.py app NAME` packs the folder with `tools/assetpack.py` into
   `build/<target>/assets/NAME.jfa`. It passes `--cfg jfgui_assets` and the path
   (`JFGUI_ASSETS`) to `rustc`.
3. `ui::main!` embeds the bundle with `include_bytes!` and mounts it as `app://`. An app is
   rebuilt when a file of its assets changes.

```text
python tools/assetpack.py apps/permtest/assets build/x.jfa      # pack by hand
python tools/assetpack.py --list build/x.jfa                    # size, packed size, codec, name
```

The bundle format JFA1 is a sorted index and the data. Each file is stored LZ4-compressed
when that saves at least a tenth, otherwise as it is. The LZ4 compressor of
`tools/assetpack.py` is the one of `lib/compress/lz4.rs`. `plat::vfs::Bundle` checks every
range of the index before it reads anything. Its `read` decompresses with
`compress::lz4::decompress`, which checks bounds. `plat::vfs::pack_bundle` writes the same
format from Rust.

## Permissions

`DirFs` and the app's folders go through the checked file functions, so
[the app's permissions](PERMISSIONS.md) apply to them. The app's own folders (`config://`,
`data://`, `cache://`) are always writable. `$APPCONFIG`, `$APPDATA` and `$APPCACHE` name
them in a manifest. `app://`, `mem://` and other in-memory providers touch no OS files.

## Limits

- `File::open`, `File::open_temp` and `FileLock::acquire` need a folder mount
  (`os_path`). For bundles, `mem://` and ZIP archives use `read_file` / `write_file`.
- `plat::fs` (`stat`, `read_dir`, links, times) works on OS paths only.
- `Overlay` has no whiteouts: a file of a lower layer cannot be deleted or renamed.
- Document handles of portals (Flatpak), Android's storage access framework and the
  browser's file handles are not providers yet.
- GUI programs from `apps/NAME/BINS` (`ui::main!`) share the app's name for its folders but
  get no assets. Console programs (`rt::main!`) use their own file name. Android builds
  (`tools/android.py`) get neither a name nor assets.
