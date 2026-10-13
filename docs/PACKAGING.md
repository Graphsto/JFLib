# Packaging -- setup.exe and .deb with a choice of style

An app that only uses library widgets can be shipped in every style ([STYLES.md](STYLES.md)).
`tools/package.py` builds one variant per style and lets the user pick the look at install
time; the installed program contains only that style.

```
python tools/package.py PaintCraft                         # build/dist/PaintCraft_Setup.exe
                                                         # build/dist/PaintCraft_0.1.0_amd64.deb
python tools/package.py paint --styles winui,classic     # a subset of the styles
python tools/package.py paint --no-deb                   # Windows only (--no-win: .deb only)
python tools/package.py PaintCraft --out build/pkg         # another output folder
python tools/package.py PaintCraft --impl asm              # legacy: the assembly reference, build/dist_asm
```

What it does:

1. **Variants**: one `python build_rs.py app NAME --style S1,S2,.. --profile size
   --target both --out <out>/rs_size` for every style and target (the libraries are built
   once, both targets side by side, the style variants linked in parallel into
   `<out>/rs_size/<target>/<style>/`); each exe is copied to `<out>/<App>/<style>/<target>/`.
   A package of PaintCraft with four styles and both targets takes about 7 minutes from scratch
   on 12 logical CPUs (one style after the other: 19.5), less when the libraries are built
   already; the eight LTO links then run at once and set the time. For a quick try,
   `--styles winui --no-deb` builds one variant. `--profile size` (the
   default) builds the UI and platform crates with opt-level z and the compute crates and the
   app with 2, fat LTO: 14-21 % smaller than the opt-level-3 release of R13, 4-15 % smaller
   than `--release` with opt-level 2 (PaintCraft 4-5 %, gallery / paint 14-15 %); codecs, effects
   and rendering as fast,
   UI scripts about 4 % slower ([rewrite/REPORT.md](../../rewrite/REPORT.md) section 2).
   `package.py --profile o2|release` picks another variant.
2. **Windows**: builds the setup stub `apps/installer` (same profile), renders a preview of every
   variant (`--shot` at 1100 x 700 DIP, light theme, scaled to 480 x 305) and writes
   `<out>/<App>_Setup.exe` = stub + payload + trailer.
3. **Linux**: assembles the package tree and runs `dpkg-deb` in WSL (Ubuntu-24.04).
4. **Modules** (jxl-module): the loadable modules of the app (`build_rs.py` `APP_MODULES`:
   PaintCraft's JPEG XL writer `jxlenc.dll` / `jxlenc.so`, the same file for every style) are
   built with the variants and installed next to the exe: one extra-file entry (kind 5) of
   the setup -- installed into the program folder, listed in `install.ini`, removed by
   `uninstall.exe` -- and `/usr/lib/<deb>/` in the .deb.

`--impl asm` does the same with `legacy/build.py` and the assembly setup stub (the frozen
reference; it needs the assembly toolchain). Both modes produce the same payload format and
the same .deb layout.

## Inputs

| File | Content |
|---|---|
| `apps/NAME/PACKAGE` | `key=value` lines: `display` (shown name), `version`, `publisher`, `summary` (one line, .deb / .desktop comment), `description` (one line), `generic` (.desktop GenericName), `categories` (.desktop), `deb_name`, `default_win` / `default_linux` (preselected style), `styles` (the variants, in order) |
| `apps/NAME/res/app.ico` | icon resource 1 of the Windows exe (`build_rs.py` compiles it with `llvm-rc` and links it: window, taskbar, Explorer, shortcuts) |
| `apps/NAME/res/app_{48,128,256}.png` | Linux icons (hicolor theme) |
| `styles/S/INFO` | line 1: the style's name in the setup / debconf, line 2: a one-line description (`--impl asm`: `legacy/ASMLIB/styles/S/INFO`) |

Texts are German (the setup and the .deb questions are). Example, `apps/PaintCraft/PACKAGE`:

```
display=PaintCraft
version=0.1.0
publisher=JFLib
summary=Bildbearbeitung im Stil von paint.net
description=Bildbearbeitung mit Ebenen, Effekten und vielen Bildformaten, geschrieben in Rust mit JFLib.
generic=Bildbearbeitung
categories=Graphics;2DGraphics;RasterGraphics;
deb_name=PaintCraft
default_win=winui
default_linux=kde
styles=winui classic macos kde
```

**Icons**: `python tools/mkicon.py NAME --glyph E771 --color 1E78E6` renders a glyph of the icon
font on a gradient square in every icon size (16..256 px), cuts anti-aliased rounded corners
(`tools/pngutil.py`) and writes `app.ico` and the PNGs. It renders with the `icongen` app of the
assembly reference (`legacy/build.py`), which has no Rust port; the icons of paint and PaintCraft
are committed, so packaging does not need it.

## Windows: setup.exe

`apps/installer` (Windows only: `apps/installer/TARGETS` = `win`) is a normal library app
in the winui style. The packaged setup is that exe with a payload and a trailer appended.

* **Pages**: style (list with the INFO names and descriptions, preview of the selection) ->
  options (folder, default `%LOCALAPPDATA%\Programs\<display>` -- per user, no administrator
  rights; desktop shortcut, start menu entry, "installed apps" entry, start after installing)
  -> progress -> done (an error page on failures).
* **Installs** `<app>.exe` (the chosen variant), `uninstall.exe` (the setup without its
  payload), `install.ini` (what was created), `.lnk` files through COM `IShellLinkW`, and
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\<app>` (DisplayName, DisplayIcon,
  UninstallString, ...). ole32 / shell32 / advapi32 are loaded with `LoadLibraryA` inside
  `plat::inst` (the only place of the shell calls).
* `uninstall.exe --uninstall` removes everything listed in `install.ini`; a hidden `cmd`
  deletes `uninstall.exe` and the folder after the process ended.
* Running the setup again installs another style over the old one (the app's settings stay).
* Options: `--dir PATH`, `--style ID`.
* **Test mode**: with `ASMINST_SHELL_DIR=DIR` the desktop and the start menu are `DIR\Desktop`
  and `DIR\Programs`, and the registry values go to `DIR\registry.txt` -- a test never touches
  the user's shell or registry. Always run the setup with `--script` (and `--dir` into a test
  folder):

  ```
  set ASMINST_SHELL_DIR=%CD%\build\dist\test\shell
  build\dist\paint_Setup.exe --script build\dist\inst.uis --dir %CD%\build\dist\test\inst
  build\dist\test\inst\uninstall.exe --uninstall --script build\dist\un.uis
  ```

  with, for example,

  ```
  # inst.uis: the preselected style, default options (start after installing is off)
  click "Weiter"
  click "Installieren"
  wait 500
  expect "Fertigstellen" exists
  ```

  ```
  # un.uis
  click "Deinstallieren"
  wait 500
  expect "Schließen" exists
  ```

  `apps/installer/tests/nopayload.uis` checks the bare stub (no payload: the error page); it
  runs in `build_rs.py test` and in parity.

**Payload layout** (written by `tools/package.py`, read by `apps/installer/payload.rs`;
little endian, offsets relative to the payload start):

| Part | Layout |
|---|---|
| header | `"AIPK"`, u32 entry count (at most 256) |
| entry (128 bytes each) | u32 kind, u32 stored size, u32 unpacked size, u32 flags (1 = zlib), u64 offset, u64 0, name[48], label[48] (UTF-8, NUL-padded) |
| kinds | 1 manifest (`key=value` lines: `app`, `display`, `version`, `publisher`, `default`, `desc_<style>`), 2 a style variant (the exe; name = style id, label = INFO name), 3 its preview PNG, 5 an extra file (name = its path in the program folder; e.g. PaintCraft's `jxlenc.dll`) |
| trailer | `"ASMINST1"`, u64 stub size, u64 payload size (the last 24 bytes of the file) |

## Linux: .deb

Built in WSL (`dpkg-deb --root-owner-group -Zxz`, inside the Linux file system for the
permissions). Every variant goes to `/usr/lib/<deb>/<App>-<style>`, the app's modules next to
them (`/usr/lib/PaintCraft/jxlenc.so`: the program looks next to its real path); debconf asks for the look
(`<deb>/style`, a select question), `postinst` registers every variant with
`update-alternatives` and points `/usr/bin/<deb>` at the chosen one. Change it later with
`sudo dpkg-reconfigure <deb>`. Also installed: `/usr/share/applications/<deb>.desktop` and the
hicolor icons. Text files are written with LF line ends (maintainer scripts).

```
sudo apt install ./build/dist/PaintCraft_0.1.0_amd64.deb
sudo dpkg-reconfigure PaintCraft          # pick another style
```

Tests never install the package on the real WSL system: check it with
`dpkg-deb --info / --contents` and extract it with `dpkg-deb -x` into a test folder.

## Text-shaping notices

The Rust build uses pinned Cargo dependencies for OpenType/Unicode shaping. The
first build needs Cargo and a populated cache or registry access. Native setup
and Debian packages include all files listed in `lib/text_shape/NOTICE.md` under
`licenses/text_shape/`; browser and loose executable outputs carry the same
notices. Preserve this directory when distributing an app. G'MIC, Paint.NET and
third-party plugins are separate installations and are not bundled.
