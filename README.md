# JFLIB

**A GUI library in Rust, with its own renderer, interchangeable styles, and apps built on top.**

The library is designed to be used by AI Agents, not by Humans – Thus the entire Codebase is optimized for hardware efficiency.

JFLIB brings widgets, layout, text, 2D graphics, image formats, and platform integration
together in one library. This repository contains both the library and its applications:
from small GUI examples to Paint, PaintCraft, jfWord, and the Build Manager.
The Rust code uses `#![no_std]`, `core`, and `alloc`. A Python build script invokes
`rustc` directly for the library and apps. Complex text shaping uses a small Cargo
subproject with pinned HarfRust and Unicode dependencies; these are statically
linked. See the [dependency notices](lib/text_shape/NOTICE.md).

![Paint displaying a JFLIB sample drawing created in the app](docs/images/readme/paint-winui.png)

[Library](#the-library) · [Platforms](#platforms) · [Apps](#apps) ·
[Build with Python](#building-apps-with-python) · [Build Manager](#build-manager) ·
[Rust examples](#small-gui-examples) · [Styles](#styles) · [Documentation](#documentation)

## The library

An app describes its interface in `App::frame`. JFLIB handles layout, input, focus,
and rendering, while the app keeps its state in a Rust struct. Widgets are drawn by
the selected style, so the same interface can be built with different appearances
without changing the application logic.

- **Widgets and layout:** buttons, checkboxes, switches, text fields, sliders, lists,
  tabs, menus, dialogs, scroll areas, toolbars, and Office ribbons.
- **Data views and app shells:** virtualized grids with sorting and multiple selection,
  trees, navigation panes, command bars, splitters, wizards, charts, toasts, and date and
  time inputs, drawn by every style.
- **Custom rendering:** a software renderer for text, paths, shapes, gradients,
  image compositing, and effects, with scalable interfaces and light and dark themes.
- **Images and documents:** built-in image codecs, including PNG, JPEG, WebP, TIFF,
  AVIF, and JPEG XL, plus shared document, layout, and DOCX components for jfWord.
- **Shared UI core:** native desktop windows, the browser host, and the Android adapter
  use the same Rust widgets and renderer.
- **Networking and security:** an HTTP/1.1 server and client, TLS 1.3 (private CAs, and a
  TLS 1.2/1.3 client for public servers with X.509 path building and the system's trust store),
  SMTP, an SSH-2 client, DNS, cryptography (BLAKE3, Argon2id, ChaCha20-Poly1305, AES-GCM,
  X25519, Ed25519, RSA, ECDSA), LZ4 and zstd, an embedded journal database, and deduplicating
  encrypted repositories -- written for BackupJF, usable by any program.
- **App automation:** built-in PNG screenshots, widget dumps, and `.uis` scripts,
  with optional JSON/MCP control that must be explicitly enabled.
- **Modular design:** dedicated `.rlib` components expose safe APIs to applications.
  `unsafe` is confined to the runtime, platform, SIMD, and plugin/module boundaries.

JFLIB grew out of the Rust rewrite of jfguiLib. Some identifiers, window titles,
and text in the apps and detailed documentation still use the earlier name or refer
to the original assembly version.

## Platforms

The following table describes the build paths available in the current repository:

| Platform | Output / build target | Status |
| --- | --- | --- |
| **Windows** | Native x86-64 app, `--target win` | Win32 windows and desktop integration |
| **Linux** | Native x86-64 app, `--target linux` | Wayland and X11; the backend is selected at startup |
| **Android** | Native APK, `--target android` | `androidGallery`, ARM64 and x86-64 emulator; Android 8 / API 26 or later |
| **macOS** | Browser output through `--target web` | Access through the web port; the current build script has no native Apple target |
| **iOS** | Browser output through `--target web` | Access through the web port; the current build script has no native iOS app/IPA target |
| **Web** | WebAssembly and a shared browser host, `--target web` | Includes `template`, `gallery`, `paint`, `PaintCraft`, and `jfWord` |

The documented browser test matrix covers Chromium and Firefox. Separate Safari/iOS
validation has not yet been documented here. The `macos` style is independent of the
target platform: it can also be built into a Windows app, for example.

The Android port is currently available through the Pocket Gallery. File picking,
persistent storage, the system clipboard, and other desktop services are not yet fully
implemented in the Android adapter. The [Android documentation](apps/androidGallery/README.md)
describes the port and its current limitations.

## Apps

Pass the app names in the first column exactly as written to `build_rs.py app NAME`.

| App | Description | Targets |
| --- | --- | --- |
| [`template`](apps/template/main.rs) | A click counter as a starting point for your own GUIs | Windows, Linux, Web |
| [`gallery`](apps/gallery/main.rs) | A widget gallery for exploring controls, layout, and styles | Windows, Linux, Web |
| [`paint`](apps/paint/main.rs) | A paint app with brushes, shapes, text, selections, and layers | Windows, Linux, Web |
| [`PaintCraft`](apps/PaintCraft/src/main.rs) | An image editor with layers, history, effects, many image formats, and native plugins | Windows, Linux, Web |
| [`jfWord`](apps/jfWord/README.md) | A word processor with a ribbon, page view, text styles, tables, references, and DOCX support | Windows, Linux, Web |
| [`buildManager`](apps/buildManager/README.md) | A graphical build app for applications, styles, profiles, tests, and release packages | Windows, Linux |
| [`BackupCraft`](apps/BackupCraft/README.md) | A backup suite: console, server (`bc-server`), agents (`bc-agent`), and `bcctl`; deduplicated, encrypted, signed backups with schedules, roles, and audit; copy jobs, replication with failover, continuous protection, tape, QEMU/KVM VMs with instant recovery and SureBackup, volume images, a central view of several servers | Windows, Linux |
| [`androidGallery`](apps/androidGallery/README.md) | A touch gallery with widgets, text input, and theme switching | Android; also Windows, Linux, and Web |

jfWord is under active development; full Word feature coverage and file compatibility
are not yet complete. Native plugins and operating-system services are more limited
in the browser than on the desktop.

PaintCraft now imports PSD/PSB raster layers, exports layered 8-bit PSD, offers
bitmap brush tips and dynamics, and connects to standalone G'MIC-Qt on Windows
and Linux. Its text tool supports OpenType shaping, bidirectional text and basic
IME composition display; English and French UI catalogs are available with German
fallback for untranslated strings. High-depth imports are converted to its 8-bit
SDR canvas. The [handbook comparison](apps/PaintCraft/docs/MANUAL_PARITY.md) lists
tested behavior and the remaining Photoshop, HDR and plugin limitations.
The optional classic Paint.NET DLL host now also connects to native Linux PaintCraft
through Wine. Brush Factory / Dynamic Draw JSON and legacy XML presets can be
imported for supported controls, with unsupported active parameters reported explicitly.
See the [Wine setup](tools/paintcraft-pdn-host/README.md#linuxwine-setup) and
[preset guide](apps/PaintCraft/docs/CREATIVE_TOOLS.md#brush-factory--dynamic-draw-presets).

PaintCraft's Rust sources are organized by domain under `apps/PaintCraft/src/`.
The [architecture guide](apps/PaintCraft/docs/ARCHITECTURE.md) covers application
state, documents, tools, effects, workspace components and external integrations.

**PaintCraft: dark WinUI style, with tools, colors, history, and layers**

![PaintCraft with the JF drawing open and its tool windows visible](docs/images/readme/paintcraft-dark.png)

**BackupCraft: the jobs page in the dark backupcraft style (demo data)**

![BackupCraft's backup jobs with status badges, the command bar, and the navigation pane](docs/images/readme/backupcraft-dark.png)

**jfWord: the same document view in the light and dark jfOffice style**

| Light | Dark |
| --- | --- |
| ![jfWord with its ribbon, navigation pane, and academic document template](docs/images/readme/jfword-light.png) | ![jfWord in the dark jfOffice style](docs/images/readme/jfword-dark.png) |

The images on this page were captured from the Rust apps and the GUI examples below,
using the built-in screenshot renderer on Windows. The Pocket Gallery image is a desktop
preview of the shared Android UI code; the Build Manager displays its bundled sample catalog.
The screenshots currently show German UI text; the code examples below use English labels.

## Building apps with Python

**Run all commands below from the root of this repository**, the directory containing
`build_rs.py`, `apps/`, `lib/`, and `styles/`.

### Prerequisites

- **Python 3**; the build tools use the standard library.
- **Rust installed through rustup**, with `rustc` and `cargo` on your `PATH`. The code targets Rust **1.98.x**.
- **LLVM / LLD**: `lld-link` for Windows, `ld.lld` for Linux, and `llvm-rc` for Windows
  app icons. `llvm-nm` is used for additional compiler checks. On Windows, the build
  script also checks `C:\Program Files\LLVM\bin` automatically; otherwise, the tools
  must be on your `PATH`.
- The appropriate **Rust target** for your desired output.

On systems where Python is named `python3`, replace `python` with `python3`.
Web and Android builds need the additional tools listed below.
The first build downloads the versions pinned in `lib/text_shape/Cargo.lock` from
crates.io. Later builds use Cargo's cache; `CARGO_NET_OFFLINE=true` enforces offline
use after that cache has been populated. No system HarfBuzz installation is needed.
Keep the generated `licenses/` folder with distributed binaries; the setup and
Debian packagers include the dependency notices automatically.

### Windows quick start

```powershell
rustup target add x86_64-pc-windows-msvc
python build_rs.py app gallery --target win
.\build\win\gallery.exe
```

### Linux quick start

```sh
rustup target add x86_64-unknown-linux-gnu
python3 build_rs.py app gallery --target linux
./build/linux/gallery
```

Displaying the Linux app requires an active Wayland or X11 session; building it does not
require a display. Windows can also build Linux output when the Linux Rust target and
`ld.lld` are installed. To run that output, the build script uses WSL, with `Ubuntu-24.04`
as the default distribution. Set `jfgui_WSL_DISTRO` to select a different distribution.

### More apps and build options

```sh
# Build individual apps
python build_rs.py app paint --target win
python build_rs.py app PaintCraft --target win
python build_rs.py app jfWord --target win

# Build Windows and Linux together; both Rust targets must be installed
python build_rs.py app jfWord --target both

# Build and launch; everything after -- is passed to the app
python build_rs.py run gallery --target win -- --theme dark

# Optimized build in build/release/win/
python build_rs.py app PaintCraft --target win --release

# Build all apps for a target, or just the library components
python build_rs.py all --target win
python build_rs.py lib --target win
```

| Option | Effect |
| --- | --- |
| `--target win` / `linux` / `web` / `android` | Build one target; the app must support it |
| `--target both` | Windows and Linux; the default for `app`, `lib`, and `all` |
| `--target all` | Windows, Linux, and Web where supported; Android is built separately |
| `--style kde` | Select a style; otherwise, use `apps/NAME/STYLE`, falling back to `winui` |
| `--style winui,classic,macos,kde,jfoffice` | Build several style variants in one invocation |
| `--release` | Optimized build with LTO; output defaults to `build/release/` |
| `--profile size` | Release profile tuned for smaller output; writes to `build/size/` |
| `--profile o2` | Optimized build without LTO; writes to `build/o2/` |
| `--out build/my-build` | Use a custom output directory, including for release builds |
| `-j 8` | Limit the number of concurrent compiler processes |

Development builds produce `build/win/NAME.exe` or `build/linux/NAME`.
When building multiple styles, the path also includes the style, for example
`build/win/classic/gallery.exe`. Selecting a single explicit style does not add this
subdirectory. Builds are incremental, and warnings are treated as build errors.
PaintCraft's additional modules, such as `jxlenc.dll` or `jxlenc.so`, are built automatically.

### Browser builds

```sh
rustup target add wasm32-unknown-unknown
rustup component add rust-src
python build_rs.py app gallery --target web
python tools/serve_web.py build/web/gallery
```

Then open [http://127.0.0.1:8766/](http://127.0.0.1:8766/). LLVM's `wasm-ld` is also
required. The default build includes scalar, SIMD128, and worker variants; the browser
host selects a variant based on available capabilities. The local server supplies the
isolation headers required for shared memory.

Use `--web-mode scalar` to build only the scalar variant, which does not require
`rust-src`. Other apps follow the same pattern, for example
`python build_rs.py app jfWord --target web`. See the
[web and agent infrastructure guide](docs/AGENT_WEB.md) and [WASM compute documentation](docs/WASM_COMPUTE.md).

### Android APKs

Additional requirements are **JDK 17+**, **Android SDK Platform 36**, **Build-Tools 35+**,
and **NDK r29**. `ANDROID_SDK_ROOT` / `ANDROID_HOME` and `ANDROID_NDK_HOME` can override
the automatically detected paths.

```sh
rustup target add aarch64-linux-android
python build_rs.py app androidGallery --target android
```

Output: `build/android/androidGallery-arm64-v8a-debug.apk`.
For an x86-64 emulator:

```sh
rustup target add x86_64-linux-android
python build_rs.py app androidGallery --target android --android-abi x86_64
```

Android builds use `rustc`, the SDK, and the NDK directly, without Gradle. The APK is
signed with a local debug key, including when using `--release`. Other apps need an
Android entry in `TARGETS` and a suitable manifest and resources.

<img src="docs/images/readme/pocket-gallery.png" alt="Pocket Gallery desktop preview with buttons, a checkbox, a switch, a slider, accent colors, and text input" width="320">

## Build Manager

The **Build Manager is itself a JFLIB app**. It provides a graphical workspace for
the existing Python build tools: select apps and styles, save profiles, prepare jobs,
track builds, run tests, and launch completed apps. Python, Rust, and LLVM are still
required when using the Build Manager.

![Build Manager with app selection, platforms, styles, a build profile, and results](docs/images/readme/build-manager.png)

Build and launch it once:

```powershell
# Windows
python build_rs.py app buildManager --target win
.\build\win\buildManager.exe
```

```sh
# Linux
python3 build_rs.py app buildManager --target linux
./build/linux/buildManager
```

The current app uses German UI labels. The steps below include those labels so you can
find the corresponding controls:

1. Open **Project & Tools (Projekt & Werkzeuge)** and select the JFLIB directory
   or its `build_rs.py` if automatic detection fails. Check the Python path and,
   on Windows, the WSL distribution if needed.
2. Select apps on the left. Under **Development (Entwicklung)**, choose target
   platforms, optional styles, compiler processes, and app tests. Save the selection
   as a profile if desired.
3. Choose **Prepare jobs (Aufträge vorbereiten)**, review the displayed commands,
   and add the jobs to the queue.
4. Under **Jobs & Results (Aufträge & Ergebnisse)**, inspect logs, tests, and build
   outputs. Use **Launch (Starten)** to create launch profiles with a theme, scale,
   and additional arguments.
5. Use **Release** to prepare optimized builds and packages. **Package data (Paketdaten)**
   contains the metadata from `apps/APP/PACKAGE`.

The GUI currently offers **Windows, Linux, and Web** build targets. Build Android apps
through the Python script. Release packages need suitable package metadata and resources.
Windows installers require a Windows host; Debian packages require `dpkg-deb`
(through WSL on Windows). Output defaults to `build/manager/`, and profiles are saved
in `build-profiles.json`.

To build a package directly from the terminal, for example forCraft:

```sh
python tools/package.py PaintCraft --styles winui,classic --no-deb
```

This creates the Windows installer at `build/dist/PaintCraft_Setup.exe`.
See the [Build Manager documentation](apps/buildManager/README.md) and
[packaging guide](docs/PACKAGING.md) for details.

## Small GUI examples

### A window with a click counter

Create `apps/hello/src/main.rs` with this complete app:

```rust
#![no_std]
#![no_main]
#![deny(unsafe_code)]

extern crate alloc;
use ui::*;

const DESC: AppDesc = AppDesc {
    title: "Hello JFLIB",
    flags: AF_RESIZABLE,
    w: 480,
    h: 300,
    min_w: 320,
    min_h: 240,
    ..AppDesc::NONE
};

#[derive(Default)]
struct Counter {
    clicks: u32,
}

impl App for Counter {
    fn frame(&mut self, ui: &mut Ui) {
        ui.box_begin(0, BOX_V | pad(24), 12);
        ui.text(TS_TITLE, "Hello JFLIB!");
        ui.text(TS_BODY, &alloc::format!("Clicks: {}", self.clicks));
        if ui.accent_button(100, "Click me") & EV_ACT != 0 {
            self.clicks += 1;
        }
        if ui.button(101, "Reset") & EV_ACT != 0 {
            self.clicks = 0;
        }
        ui.box_end();
    }
}

ui::main!(ui::launch(&DESC, Counter::default(), &style::STYLE));
```

```sh
python build_rs.py run hello --target win
# On Linux: python3 build_rs.py run hello --target linux
```

![The click counter example after two clicks, with German labels in this screenshot](docs/images/readme/example-counter.png)

`AppDesc` describes the window, `Counter` holds the state, and `frame` describes the
widgets. Buttons report activation through `EV_ACT`. Sizes and spacing are expressed
in device-independent pixels (DIP); the library handles scaling. `style::STYLE` refers
to the style selected at build time.

### A text field, checkbox, and slider

In the example above, replace `Counter`, its `impl App`, and the `ui::main!` invocation
with this version. Keep the crate attributes, `extern crate alloc`, `use ui::*`, and
`DESC` unchanged:

```rust
#[derive(Default)]
struct Form {
    name: alloc::string::String,
    enabled: i32,
    value: i32,
}

impl App for Form {
    fn frame(&mut self, ui: &mut Ui) {
        ui.box_begin(0, BOX_V | PAD_16, 12);
        ui.text(TS_SUBTITLE, "Settings");
        ui.textbox(200, Some("Your name"), &mut self.name, 128);
        ui.checkbox(201, "Enable option", &mut self.enabled);
        ui.slider(202, &mut self.value, &Range::new(0, 100, 1, 0));
        ui.progress(203, self.value, 100);
        ui.text(TS_BODY, &alloc::format!("Value: {} %", self.value));
        ui.box_end();
    }
}

ui::main!(ui::launch(&DESC, Form::default(), &style::STYLE));
```

![The form example with text input, a checkbox, a slider, and a progress bar, shown with German labels](docs/images/readme/example-form.png)

Values are updated directly through `&mut`. Checkboxes use `i32` (`0` / `1`), and
widget IDs must be unique within a frame. File operations, dialogs, and larger actions
can be dispatched to `App::command` through `ui.cmd_run(...)`.

New apps can use `src/main.rs`; existing flat `main.rs` projects are also supported.
Each app must have only one of these entry points. A `STYLE` file containing a name
such as `kde` sets the default style. A `TARGETS` file can explicitly list supported
targets, such as `win linux web`.
More examples: [template](apps/template/main.rs), [widget gallery](apps/gallery/main.rs),
and [developer guide](docs/GUIDE.md).

## Styles

The style is selected **at build time**. Light/dark themes, accent colors, and scaling
can change at runtime. The style and operating system are independent of each other.

| Style | Appearance |
| --- | --- |
| `winui` | Modern Windows/Fluent interface with rounded controls |
| `classic` | Classic gray interface with raised and sunken borders |
| `macos` | Controls and window decorations inspired by macOS |
| `kde` | Interface inspired by KDE Plasma/Breeze |
| `jfoffice` | Office style with its own typography, ribbon components, and blue accents |
| `backupcraft` | WinUI layers with KDE Breeze framing, petrol accent, compact controls, own icons |

**The same widget gallery with six styles and two themes:**

| WinUI · light | Classic · light |
| --- | --- |
| ![Widget gallery in the light WinUI style](docs/images/readme/gallery-winui.png) | ![Widget gallery in the Classic style](docs/images/readme/gallery-classic.png) |

| macOS style · light | KDE style · light |
| --- | --- |
| ![Widget gallery in the macOS style on Windows](docs/images/readme/gallery-macos.png) | ![Widget gallery in the KDE style on Windows](docs/images/readme/gallery-kde.png) |

| jfOffice · light | WinUI · dark |
| --- | --- |
| ![Widget gallery in the jfOffice style](docs/images/readme/gallery-jfoffice.png) | ![Widget gallery in the dark WinUI style](docs/images/readme/gallery-winui-dark.png) |

| BackupCraft · light |
| --- |
| ![Widget gallery in the BackupCraft style](docs/images/readme/gallery-backupcraft.png) |

```sh
python build_rs.py app gallery --target win --style winui,classic,macos,kde,jfoffice,backupcraft
```

On Windows, launch `build/win/kde/gallery.exe` or `build/win/macos/gallery.exe`, for example.
For Linux, use the same build command with `--target linux`.

**The apps use the same styles. Here is Paint with Classic:**

![Paint displaying the same JFLIB drawing in the Classic style](docs/images/readme/paint-classic.png)

```sh
python build_rs.py app paint --target win --style classic --out build/paint-classic
python build_rs.py run gallery --target win -- --theme dark --scale 150
```

[STYLES.md](docs/STYLES.md) describes the style contract, creating custom styles, and
the origins of bundled font and icon assets. See [jfOffice](styles/jfoffice/README.md)
for more details about the Office style.

## Screenshots and checks

GUI apps can render directly to a PNG without opening a visible window:

```sh
python build_rs.py run gallery --target win -- --shot build/gallery.png --size 1000x800 --scale 100 --theme light --anim off
```

`--shot` saves the image and exits the app. To prepare particular views or interaction
sequences, use `.uis` scripts with commands such as `click`, `type`, `theme`, `expect`,
and `shot`. Run the existing template test with:

```sh
python build_rs.py run template --target win -- --script apps/template/tests/smoke.uis --verify-dirty
python build_rs.py check
python build_rs.py test --quick --target win
```

`check` validates the source-code rules. `test --quick` starts the broader test suite
and skips the historical assembly parity checks. Some existing PaintCraft tests still
expect reference plugins under `build/rwref`, which are not included in a fresh checkout.
The full parity run also requires the separate assembly reference.
For rust-analyzer, `python build_rs.py ide` generates `rust-project.json`.

## Documentation

| Guide | Contents |
| --- | --- |
| [Developer guide](docs/GUIDE.md) | App model, layout, widgets, menus, dialogs, files, and scripted tests |
| [API reference](docs/API.md) | Library functions and types |
| [Styles](docs/STYLES.md) | Appearance, style interfaces, and assets |
| [Build Manager](apps/buildManager/README.md) | Profiles, queue, tests, launching, and releases |
| [Packaging](docs/PACKAGING.md) | Windows installers, Debian packages, icons, and package metadata |
| [Android Gallery](apps/androidGallery/README.md) | APKs, Android prerequisites, input, and port status |
| [Web and agent control](docs/AGENT_WEB.md) | Browser host and explicit CLI/JSON/MCP interfaces |
| [PaintCraft plugins](plugins/README.md) | Native plugins and plugin builds |
| [jfWord](apps/jfWord/README.md) | Document editing and current feature coverage |
| [BackupJF](apps/BackupCraft/README.md) | Server, agents, console and bcctl: setup, architecture, security model, tests |

Some older documents still refer to a `RustLib/` directory. In this standalone
repository, that corresponds to the repository root.

| Directory | Contents |
| --- | --- |
| `lib/ui`, `lib/gfx`, `lib/text` | Widgets, runtime, layout, graphics, and text rendering |
| `lib/rt`, `lib/simd`, `lib/plat`, `lib/base` | Runtime, compute kernels, platform integration, and base types |
| `lib/img_*`, `lib/fx` | Image formats, image processing, and effects |
| `lib/doc_*`, `lib/archive` | Document model, layout, rendering, and file formats |
| `lib/agent_core`, `lib/agent_io` | Contracts and transports for optional app control |
| `lib/crypto`, `lib/compress`, `lib/datetime` | Cryptography, LZ4, time zones and cron schedules |
| `lib/netio`, `lib/datastore`, `lib/cas` | HTTP, TLS, SSH, DNS; the journal database; backup repositories |
| `styles/` | The six bundled styles |
| `apps/` | Applications, examples, and their tests |
| `plugins/`, `modules/` | Native extensions and loadable modules |
| `web/`, `android/` | Browser host and shared Android adapter |
| `tools/` | Build helpers, packaging, tests, and documentation tools |
| `docs/` | References, guides, and the screenshots used in this README |
| `build/` | Generated programs, library components, and test output |
