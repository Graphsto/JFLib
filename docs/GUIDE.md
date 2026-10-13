# Writing an app with jfguiLib (Rust)

This guide is for developers and AI models writing desktop apps on the Rust version of
jfguiLib. Start from `apps/template`, look up functions in [API.md](API.md) (generated from
the sources), and look at `apps/gallery` for every control in use. How widgets look is the
business of the linked style ([STYLES.md](STYLES.md)); packaging is in
[PACKAGING.md](PACKAGING.md). The library itself is described in `README.md`.

The examples below were compiled and run against the library (R13); identifiers such as
`ID_*`, `CMD_*` and the fields of `self` are the example app's own.

## 1. Rules of the code base

* **`#![no_std]`, no crates, no Cargo.** Only `core` and `alloc` may be `extern crate`d;
  `build_rs.py` calls `rustc` per crate and refuses `std` or a foreign dependency
  (`python build_rs.py check` runs the lint alone). Edition 2024, so `gen` is a keyword.
* **`unsafe` only in `rt`, `simd`, `plat` and `plugin`.** Every other library and style crate
  has `#![forbid(unsafe_code)]`; apps have `#![deny(unsafe_code)]`, whose only exception is
  the `ui::main!` entry point. The safe interfaces of those four crates are all an app needs.
* **No system libraries** beyond kernel32 / user32 / gdi32 on Windows (other DLLs only through
  `GetProcAddress`, inside `plat`), nothing but syscalls on Linux. Fonts, icons and codecs are
  the library's own.
* **Warnings fail the build** (`-D warnings`).
* **Float code keeps a fixed operation order** where results must be reproducible: no FMA,
  `rt::fx` for the SSE conversions (`cvtt`, `cvtd`, `minsd`, ...), `rt::x87` for the x87 steps.
  This matters for library code that must stay bit-identical to the assembly reference; app
  code only needs it when it promises reproducible output.
* Strings are UTF-8 everywhere; the apps' UI texts are German.
* Colours in the API are `0xAARRGGBB`, straight alpha. Images and layers are premultiplied
  (`gfx::FMT_PARGB`); effects work on straight `FMT_ARGB`.

## 2. Skeleton

`apps/template/main.rs`, complete:

```rust
#![no_std]
#![no_main]
#![deny(unsafe_code)]

extern crate alloc;

use ui::*;

const ID_HELLO: u32 = 100;
const ID_RESET: u32 = 101;
const CMD_RESET: u32 = 1;

const DESC: AppDesc = AppDesc {
    title: "jfgui Template",
    flags: AF_RESIZABLE,
    w: 480,
    h: 320,
    min_w: 320,
    min_h: 240,
    ..AppDesc::NONE
};

#[derive(Default)]
struct Template {
    count: i32,
}

impl App for Template {
    // one vertical stack: title, text, two buttons
    fn frame(&mut self, ui: &mut Ui) {
        ui.box_begin(0, BOX_V | pad(24), 12);
        ui.text(TS_TITLE, "Hallo, Assembly!");
        ui.text(TS_BODY | TX_SECONDARY, &alloc::format!("Klicks: {}", self.count));
        ui.box_begin(0, BOX_H | BOX_XCENTER, 8);
        if ui.accent_button(ID_HELLO, "Klick mich") & EV_ACT != 0 {
            self.count += 1;
        }
        if ui.button(ID_RESET, "Zurücksetzen") & EV_ACT != 0 {
            ui.cmd_run(CMD_RESET, 0); // commands run after the frame
        }
        ui.box_end();
        ui.box_end();
    }

    fn command(&mut self, _ui: &mut Ui, id: u32, _arg: u32) -> bool {
        if id != CMD_RESET {
            return false;
        }
        self.count = 0;
        true
    }
}

ui::main!(ui::launch(&DESC, Template::default(), &style::STYLE));
```

* `AppDesc` holds the title, `AF_*` flags (`AF_RESIZABLE`, `AF_CUSTOM_TITLEBAR`, `AF_DARK`,
  `AF_LIGHT`), the initial and minimum client size in DIP, an accent colour (`0xRRGGBB`, 0 =
  the style's), the command table `cmds` and the menu bar `menubar` (section 6). Write
  `..AppDesc::NONE` so new fields keep their defaults.
* `ui::launch(desc, app, style)` owns the app. Native targets parse the command line and run
  headless (`--shot`, `--script`) or open a window; web targets retain it in the shared worker
  runtime. `ui::run` remains available for existing borrowed native apps. `style::STYLE` is
  whatever style crate the build linked as `style` (section 11).
* `ui::main!` expands to the `jfgui_main` symbol the platform entry point calls.

**Where an app's files live:**

| Path | Content |
|---|---|
| `apps/NAME/src/main.rs` | new modular crate root; keep app, UI and workflows in separate modules; existing flat `main.rs` projects stay supported |
| `apps/NAME/tests/*.uis` | test scripts (`apps/NAME/tests/*.uis` for scripts that only the Rust build has) |
| `apps/NAME/STYLE` | one word: the app's style (default `winui`) |
| `apps/NAME/TARGETS` | e.g. `win linux web`: explicit supported targets; owned GUI apps default to native plus web, native CLI probes to Windows/Linux |
| `apps/NAME/res/app.ico`, `app_{48,128,256}.png` | exe icon, Linux icons (`tools/mkicon.py`) |
| `apps/NAME/PACKAGE` | packaging metadata (PACKAGING.md) |
| `apps/NAME/parity.txt` | parity settings, only for apps that also exist in `legacy/` |

The same source entrypoint is used by build/dependency discovery, hashes, lint and IDE/API
tools. Two competing roots (`main.rs` plus `src/main.rs`, or their library equivalents) are
an error. Reference material under `context` or `books` is never discovered as app source.
New reusable crates use `src/lib.rs`; existing app/library/style trees need no migration.

The common [agent and browser guide](AGENT_WEB.md) explains opt-in `--agent`, headless
JSON/MCP, `App::actions` and capability checks. `--target web` builds the same GUI into
`build/web/NAME/`. Use `plat::capabilities()` and asynchronous web requests for services that
depend on the host; never report a VFS write as durable before its completion arrives.

**Build and run:**

```
python build_rs.py app NAME                    # build/win/NAME.exe and build/linux/NAME
python build_rs.py app NAME --target win       # one target
python build_rs.py run NAME -- --shot out.png  # build and start (Linux: --target linux, via WSL)
python build_rs.py app NAME --style kde --out build/kde
python build_rs.py app NAME --release          # opt-level 2 + LTO
python build_rs.py app NAME --profile size     # smaller release variant (packages)
```

Every app crate gets every library crate (`ui`, `gfx`, `text`, `plat`, `base`, `rt`, `img_core`,
`img_*`, `img_all`, `fx`, ...) as an `--extern`, plus its style as `style`. Development builds
are incremental (about 1.5 s after an edit in `ui`). Use `--out build/<name>` when several
agents build at the same time.

**Platforms need no app code.** The same source builds a Windows exe and a static Linux ELF.
On Linux the library picks the display backend at start-up: Wayland when `WAYLAND_DISPLAY` is
set, else X11 via `DISPLAY` (`jfgui_BACKEND=x11|wayland` forces one). Apps call only `ui`,
`gfx`, `img_core`, `text`, `fx` and the neutral `plat` helpers (files, clock, threads). The only
platform-related choice is `AF_CUSTOM_TITLEBAR` (section 7); without it the system frame is used,
and on Wayland compositors without server-side decorations the library draws a title bar itself
(`jfgui_CSD=1` forces that).

## 3. The model: immediate mode

* `App::frame` runs for every pass and describes the whole UI by calling widget methods on
  `ui`. Nothing is retained between frames except your own data and small per-id state inside
  the library (animations, scroll positions, measurements).
* Every widget returns **`EV_*` flags**: `ui.button(id, "OK") & EV_ACT != 0` = clicked /
  changed / selected. Others: `EV_COMMIT` (value committed: drag released, Enter, focus lost),
  `EV_HOVER`, `EV_HELD`, `EV_FOCUS`, `EV_DOUBLE`, `EV_RCLICK`, `EV_OPEN`, `EV_DRAG`.
* Value widgets edit your variables through `&mut`: `ui.checkbox(id, "Label", &mut self.flag)`.
  Booleans of the API are `i32` (0 / 1, checkboxes also 2 = indeterminate), as in the assembly.
* After an action the library builds the frame once more, so everything shows the new state.
  Keep `frame` free of side effects other than reacting to `EV_*` flags.
* Heavy or structural work (file I/O, opening dialogs, big edits) goes through
  `ui.cmd_run(cmd, arg)`; it reaches `App::command(ui, id, arg)` between frames. Return `true`
  when you handled it. The library also delivers:

  | Id | When |
  |---|---|
  | `UIC_CLOSE` | the user closes the main window; return `true` to keep it (ask "save changes?" first) |
  | `UIC_TIMER` | `arg` = timer id of `ui.timer(id, ms)` (one-shot; library timers arrive too: ignore unknown ids) |
  | `UIC_ESCAPE` | Esc that no widget consumed |
  | `UIC_DPI`, `UIC_THEME` | the dpi / the theme changed |
  | `UIC_WINCLOSE` | `arg` = window id: the user closes a secondary window; `true` keeps it |
  | `UIC_DROP` | `arg` = window id: files were dropped (`ui.drop_list()`, `ui.drop_pos()`) |
  | `UIC_WAKE` | `plat::wake()` from another thread (a background job finished) |
  | `UIC_EXIT` | the window loop ends (only after a real window ran: save settings here) |

* `App::init(ui)` runs before the first frame (register codecs, read arguments, load
  settings); `false` ends the program with exit code 2.
* **Ids** are plain `u32` constants, unique among the widgets of a frame (duplicate ids break
  hover / press tracking). Repeated items: `BASE + i`. Bits 28..31 are reserved for the
  library (`part_id`, split-button chevrons). A box id may be 0 (automatic). Some widgets also
  take the ids after their own: a segmented control, tabs, wizard steps and breadcrumbs one
  per item (`id + 1 + i`), an infobar `id + 1` (its close button), list and grid rows
  `id + 1 + i` -- leave that room before the next widget's id.

## 4. Layout

```rust
ui.text(TS_TITLE, "Hallo");
ui.box_begin(0, BOX_H | BOX_XCENTER, 8); // a row, children centred vertically, 8 DIP gap
ui.text(TS_BODY, "Name:");
ui.next_size(-1, 0); // the text box takes the remaining width
ui.textbox(ID_NAME, Some("Name eingeben"), &mut self.name, 64);
ui.next_size(120, 0); // 120 DIP wide, natural height
ui.button(ID_OK, "OK");
ui.box_end();
// a card with a 16 DIP padding
ui.box_begin(0, BOX_V | BOX_CARD | PAD_16, 4);
ui.text(TS_BODY_STRONG, "Karte");
ui.text(TS_CAPTION | TX_SECONDARY | TX_WRAP, "Karten gruppieren Inhalte.");
ui.box_end();
```

* `box_begin(id, flags, gap)` / `box_end()`: children flow along the main axis (`BOX_H` left to
  right, `BOX_V` top to bottom). Padding: `| PAD_8`, `| pad(24)` (DIP).
* `next_size(w, h)` sizes the next widget or box in DIP: `> 0` fixed, `0` natural, `< 0` fill
  weight (`-1` = take the remaining space). The cross axis stretches by default;
  `BOX_XCENTER` / `BOX_XSTART` / `BOX_XEND` give children their natural cross size.
* Main axis: `BOX_CENTER`, `BOX_END`; `spacer(dip)` (`-1` = flexible); `BOX_WRAP` starts a new
  row when a horizontal box is full.
* Absolute placement: `next_rect(x, y, w, h)` in DIP relative to the current box (overlays,
  floating panels over a canvas); `next_rect_px` in window pixels.
* Decoration and behaviour: `BOX_CARD`, `BOX_LAYER` (command bars), `BOX_BG`, `BOX_CLIP`,
  `BOX_BLOCK` (swallows clicks), `BOX_DISABLED` (everything inside disabled). `BOX_FIXED` for
  fill boxes whose size never depends on their content (fewer layout passes when deeply nested).
* Sizes are measured over passes: a new layout settles within 2-3 internal passes; you never
  see intermediate states.
* `last_rect()` is the physical-pixel rectangle `[x, y, w, h]` of the last widget,
  `box_rect()` the content rectangle of the current box.
* Units: everything you pass to layout and widgets is DIP (96 dpi = 100 %); drawing calls and
  rectangles the library returns are physical pixels. `ui.dp(dip)` converts, `ui.hair()` is a
  one-line width for the current dpi. Test with `--scale 150`.

## 5. Widgets

All of them are `Ui` methods (API.md, sections Widgets, Text boxes, Lists, Values). Labels are
copied, so they may come from temporary `String`s.

**Text and buttons**

```rust
ui.box_begin(0, BOX_H | BOX_XCENTER, 12);
if ui.accent_button(ID_APPLY, "Übernehmen") & EV_ACT != 0 {
    ui.cmd_run(CMD_RESET, 0); // the work runs in App::command after the frame
}
ui.next_flags(ST_DISABLED);
ui.button(ID_APPLY + 1, "Deaktiviert");
ui.next_icon(IC_ADD);
ui.button(ID_APPLY + 2, "Mit Icon");
ui.icon_button(ID_APPLY + 3, IC_SAVE, "Speichern"); // the label becomes the tooltip
ui.cmd_button(CMD_SAVE, ST_ICON_ONLY | ST_SUBTLE);   // a button for a command (section 6)
ui.tooltip("Speichert das Bild");                     // tooltip of the last widget
ui.box_end();
```

* `text(TS_* | TX_*, s)`: text styles `TS_CAPTION`, `TS_BODY`, `TS_BODY_STRONG`, `TS_SUBTITLE`,
  `TS_TITLE`, `TS_TITLE_LARGE`, `TS_DISPLAY`; flags `TX_SECONDARY`, `TX_TERTIARY`, `TX_ACCENT`,
  `TX_WRAP` (wrap at the box width), `TX_ELLIPSIS` (cut with "…"; with `TX_WRAP` at the height
  of `next_size`), `TX_CENTER`, `TX_RIGHT`.
* Buttons: `button`, `accent_button`, `icon_button(id, IC_*, label)`, `toggle_button(id, label,
  &mut v)`, `tool_button(id, IC_*, label, &mut sel, mine)` (tool palettes), `cmd_button(cmd, ST_*)`.
* Modifiers for the next widget: `next_flags(ST_DISABLED | ST_SUBTLE | ST_ACCENT | ST_CHECKED |
  ST_LARGE | ST_COMPACT | ST_NOFOCUS | ST_ICON_ONLY)`, `next_icon(IC_*)`. Post-modifiers for the
  last widget: `tooltip(s)`, `context_menu(menu)`.
* `icon(IC_*, size_dip)`, `separator()`, `progress(id, v, max)`.

**Selection and ranges**

```rust
ui.checkbox(ID_SHOW, "Anzeigen", &mut self.show);
for (k, label) in ["Rot", "Grün", "Blau"].iter().enumerate() {
    ui.radio(ID_MODE + k as u32, label, &mut self.mode, k as i32);
}
ui.switch(ID_SHOW + 1, "WLAN", &mut self.wlan);
ui.next_size(200, 0);
if ui.slider(ID_SIZE, &mut self.size, &RG_SIZE) & EV_COMMIT != 0 {
    // released: a good moment for expensive work
}
ui.number_box(ID_SIZE + 1, &mut self.size, &RG_SIZE);
```

with `const RG_SIZE: Range = Range::new(1, 100, 1, 0);` (min, max, step, `RG_*` flags:
`RG_VERT`, `RG_TICKS`, `RG_TIP`). A vertical slider needs a height: `next_size(60, 100)`.

**Text input**

* `textbox(id, Some("placeholder"), &mut string, cap | TB_*)` -> `EV_ACT` = changed,
  `EV_COMMIT` = Enter / focus lost. `cap` is the byte capacity; flags `TB_PASSWORD`,
  `TB_READONLY`, `TB_DIGITS` (+ `TB_SIGNED`, `TB_DECIMAL`), `TB_MULTI` (+ `TB_WRAP`; give it a
  height with `next_size`).
* `number_box(id, &mut i32, &Range)`, `number_box_f(id, &mut f64, &RangeF)` with
  `RangeF::new(min, max, step, decimals, flags)`; `ui.fmt_f64(v, decimals)` /
  `ui.parse_f64(s)` use the UI's decimal separator (default `,`; `set_decimal_sep`).
* `clip_set` / `clip_get` / `clip_set_image` / `clip_get_image` reach the clipboard (headless
  runs use a private one).

**Values and colours**

```rust
const RF_ZOOM: RangeF = RangeF::new(10.0, 800.0, 0.0, 1, 0);

ui.next_size(200, 0);
ui.slider_f(ID_ZOOM, &mut self.zoom, &RF_ZOOM);
ui.number_box_f(ID_ZOOM + 1, &mut self.zoom, &RF_ZOOM);
ui.angle(ID_ANGLE, &mut self.angle, 0);
ui.swatch(ID_COLOR, self.color, false);
ui.color_picker(ID_COLOR + 1, &mut self.color);
```

Also `prop_slider(id, label, &mut v, &range, default)` / `prop_slider_f` (paint.net-style
property rows with a reset button), `track_slider(id, &mut v, &range, Some(&track))` (a slider
over a gradient: `Track::new(TK_*, &stops)`), `pan(id, &mut x, &mut y, thumb, ver, flags)`,
`color_picker_ex(id, &mut argb, CP_ALPHA | CP_FIELDS)`, `color_wheel(id, &mut hsv_or_argb,
CW_HSV)`; `hsv_to_rgb` / `rgb_to_hsv` convert (hue in tenths of a degree, S / V 0..1000).

**Containers**

```rust
ui.tabs(ID_TABS, &mut self.tab, &["Allgemein", "Erweitert"]);
if self.tab == 0 {
    ui.next_size(0, 200);
    let (first, end) = ui.list_begin(ID_LIST, &mut self.sel, 1000, 0); // 1000 rows, default height
    for i in first..end {                                              // only the visible rows
        if ui.list_item(i, &format!("Eintrag {}", i + 1)) & EV_DOUBLE != 0 {
            ui.cmd_run(CMD_OPEN, i as u32);
        }
    }
    let (_ev, _v) = ui.list_end(&mut self.sel);
} else {
    if ui.expander_begin(ID_EXP, "Erweiterte Einstellungen", &mut self.exp) != 0 {
        ui.checkbox(ID_AUTO, "Automatisch speichern", &mut self.auto);
    }
    ui.expander_end();
}
```

* `scroll_begin(id, SC_V | SC_H | SC_NOBAR | BOX_* flags)` / `scroll_end()`: a scroll view (give it
  a size, e.g. `next_size(0, -1)`); `scroll_to`, `scroll_pos`, `scroll_extent`.
* `list_begin(id, &mut sel, count, row_h | LF_REORDER)` -> `(first, end)`: emit rows
  `first..end` with `list_item(i, label)` or `list_item_begin(i)` ... any widgets ...
  `list_item_end()`; `list_end(&mut sel)` -> `(EV_*, value)`: `EV_ACT` selection changed,
  `EV_DOUBLE`; with `LF_REORDER`, `EV_DRAG` and value = `from | to << 16` (the gallery's
  "Listen" section moves the item).
* `tabs(id, &mut sel, names)`, `expander_begin(id, header, &mut open)` (non-zero: emit the
  content) / `expander_end()`, `scrollbar(id, &mut pos, content, view, vertical)` for custom
  canvases.

**Images, custom drawing, canvases**

* `image(id, &Rc<Surface>, ver)` shows a surface at its natural size (or `next_size`); bump
  `ver` when its pixels change (the dirty-region renderer compares it).
* Drawing between widgets, in physical pixels on the current layer: `draw_color(argb)`,
  `draw_rect`, `draw_rrect`, `draw_frame`, `draw_text(x, y, s, FONT_* | size_fx << 8)`,
  `draw_icon(IC_*, x, y, size_fx)`, `draw_circle` / `draw_line` (24.8 fixed point = px × 256),
  `draw_image(&rc, ver, x, y, w, h)`, `draw_clip` / `draw_unclip`. Colours of the theme come
  from `ui.color(T_*)`.

  ```rust
  let r = ui.last_rect();
  let c = ui.color(T_ACCENT);
  ui.draw_color(c);
  ui.draw_rect(r[0], r[1] + r[3] - ui.hair(), r[2], ui.hair());
  ```

* `canvas(id, Some(draw), user, version)` is a custom area with full pointer input. The draw
  object implements `ui::Draw` and runs at render time with the clip set to the widget; it gets
  a `Gfx` (the draw context `dc`, the target surface `sf`, the text engine; `g.shapes()` for
  anti-aliased rounded rectangles, circles and lines). Share the data it draws through `Rc` /
  `RefCell` / `Cell`:

  ```rust
  struct Board {
      img: RefCell<gfx::Surface>,
  }

  impl Draw for Board {
      fn draw(&self, g: &mut Gfx, r: [i32; 4]) {
          g.dc.set_color(0xFFFF_FFFF);
          g.dc.fill(g.sf, r[0], r[1], r[2], r[3]);
          g.dc.blit(g.sf, &self.img.borrow(), r[0], r[1]);
          g.dc.set_color(0xFF00_78D4);
          g.shapes().stroke_rrect(r[0] + 4, r[1] + 4, r[2] - 8, r[3] - 8, 6, 2);
      }
  }

  // in frame (board: Rc<Board>, board_ver: u32):
  ui.next_size(320, 200);
  let d: DrawRef = self.board.clone();
  let inp = ui.canvas(ID_CANVAS, Some(d), 0, self.board_ver).clone();
  if inp.ev & EV_HELD != 0 && inp.btn & (1 << BTN_LEFT) != 0 {
      let mut img = self.board.img.borrow_mut();
      let mut dc = gfx::Dc::new(img.w, img.h);
      dc.set_color(self.color);
      for p in &inp.pts {                       // every move point since the last frame
          dc.fill(&mut img, p[0] - 2, p[1] - 2, 5, 5);
      }
      self.board_ver += 1; // the content changed: the canvas must be redrawn
      ui.repass();         // ... in this frame (its command carries the old version)
  }
  ```

  `UiInput` has the position relative to the widget (`x`, `y`, `press_x`, `press_y`), buttons
  (`btn`, `pressed`, `released`, bit `1 << BTN_*`), `wheel_x` / `wheel_y` (120 per notch),
  `mods`, all move points `pts`, the first unconsumed key, and pen data (`pressure`, `ptype`,
  `ppress`, tilt; `legacy/ASMLIB/docs/modules/PEN_INPUT.md`). The cursor shape comes from
  `next_flags(CUR_* << HB_CURSOR_SHIFT)`. `ui.canvas` returns a borrow of `ui`: clone the
  input before calling more `ui` methods. `canvas_damage(id, x, y, w, h)` redraws a part
  without a version change.
* Direct pixel work (paint programs): `gfx::Surface::new(w, h, FMT_PARGB)`, a `gfx::Dc`
  (`fill`, `blit`, `blit_scaled`, clip and origin), `gfx::path` / `raster` / `stroke` for
  anti-aliased vector paths, `gfx::blendmode`, `gfx::tmask` (selection masks), `gfx::flood`,
  `gfx::gradient`; the text engine in the field `ui.text` (`text::Text`) draws text into any
  surface (`set_font`, `draw`, `measure`).
  `img_core::ops` rotates, flips, scales and crops, `img_core::resample` resizes with filters,
  `img_core::xform` applies affine transforms.

## 6. Commands, menus, accelerators, popups

Put repeated actions into a **command table** and reference them from menus, buttons and
accelerators; the library then does shortcuts, disabled / checked states and tooltips:

```rust
const CMD_OPEN: u32 = 1;
const CMD_SAVE: u32 = 2;
const CMD_UNDO: u32 = 3;
const CMD_GRID: u32 = 4;
const CMD_RESET: u32 = 5;
const CMD_RECENT0: u32 = 20; // .. 27

static CMDS: [Command; 4] = [
    //           id         label           icon          mods      key            CF_* state
    Command::new(CMD_OPEN, "Ö&ffnen…", IC_OPEN_FILE, MOD_CTRL, b'O' as u32, 0),
    Command::new(CMD_SAVE, "&Speichern", IC_SAVE, MOD_CTRL, b'S' as u32, 0),
    Command::new(CMD_UNDO, "&Rückgängig", IC_UNDO, MOD_CTRL, b'Z' as u32, CF_DISABLED),
    Command::new(CMD_GRID, "&Raster", IC_GRID, MOD_CTRL, b'G' as u32, 0),
];
static MENUBAR: [Mi; 2] = [Mi::Menu("&Datei", &M_FILE), Mi::Menu("&Ansicht", &M_VIEW)];
static M_FILE: [Mi; 5] = [
    Mi::Cmd(CMD_OPEN),
    Mi::Cmd(CMD_SAVE),
    Mi::Sep,
    Mi::Sub("&Zuletzt geöffnet", &M_RECENT),           // a submenu
    Mi::Item(CMD_RESET, "&Zurücksetzen", 0),           // a plain row: App::command(id, 0)
];
static M_RECENT: [Mi; 1] = [Mi::List(CMD_RECENT0, 8, "Keine Dateien")];
static M_VIEW: [Mi; 1] = [Mi::Cmd(CMD_GRID)];

const DESC: AppDesc = AppDesc { title: "Mein Werkzeug", flags: AF_RESIZABLE, w: 900, h: 640,
    cmds: &CMDS, menubar: &MENUBAR, ..AppDesc::NONE };

// in frame, before the widgets:
ui.cmd_state(CMD_GRID, if self.grid { CF_CHECKED } else { 0 });
ui.menu_list(CMD_RECENT0, &self.recent); // the rows of Mi::List (Vec<String>)
ui.menubar(None);                        // the menu bar of the AppDesc
```

* A chosen menu row, a `cmd_button` and an accelerator all queue `App::command(cmd, 0)`.
  `&` marks the mnemonic (`&&` = a literal `&`). Keys are `VK_*` (Win32 numbering on every OS,
  `ui::keys`), modifiers `MOD_CTRL | MOD_SHIFT | MOD_ALT`.
* `cmd_state(cmd, CF_DISABLED | CF_CHECKED | CF_HIDDEN)` is persistent; set it every frame from
  your model. Accelerators fire only when no widget consumed the key first (`ui.key(VK_*)`).
  `accel_text(cmd)` gives the shortcut as shown in menus ("Strg+S").
  An editor that owns the whole document history can call `capture_shortcut(vk, mods)`
  before its widgets to reserve an exact chord, including one normally owned by a textbox.
  Guard modal draft/file dialogs so their text-editing shortcuts retain priority. jfWord
  uses this opt-in API for document undo/redo and inserting a paragraph.
* `textbox_selection(id)` returns the focused textbox's UTF-8 byte offsets `(anchor, caret)`.
  Read it after emitting that textbox. `textbox_select(id, text, anchor, caret)` restores a
  selection on an already focused textbox; pass its exact current text. Invalid byte
  boundaries and inactive fields return `false`. Document editors can use these opt-in APIs
  to format a selection or highlight a search result without changing ordinary textbox undo.
* `textbox_surface(id, &mut text, cap_flags, &lines)` adds input over externally rendered
  rich text. Each `TextSurfaceLine` supplies UTF-8 start/end offsets, an absolute pixel
  rectangle and one caret stop per scalar boundary. Lines can span multiple pages and
  have different alignment and font metrics. The caller draws text and owns scrolling;
  the widget draws selection/caret and provides keyboard, clipboard and IME handling.
  `textbox_surface_change()` returns the original replacement range and inserted text,
  including the actual insertion position when neighboring characters repeat. Apply
  this to the document model atomically, reject protected fields there, and rebuild layout
  after `EV_ACT`. `apps/jfWord/rich_ui.rs` shows integration with `doc_layout` source ranges.
  The buffer may span an entire story, with paragraph offsets from `doc_core::StoryText`.
  Surface paste normalises CRLF and lone CR to LF; the model decides which inserted breaks
  create paragraphs. Surface replacements exceeding capacity are rejected before deletion,
  not truncated. Set capacity for the displayed projection, including expanded fields and
  the terminator. These opt-in rules leave ordinary textbox behaviour unchanged.
* `clip_set_many(&[(format, bytes), ...])` publishes up to four representations of one
  selection: `CLIP_TEXT`, `CLIP_HTML`, `CLIP_RTF` and `CLIP_DOCX`. It returns `0` on success
  and `-1` on failure; only remove a cut selection after success. Text and HTML must be
  NUL-free UTF-8; RTF and the private document payload are binary. Combined data is bounded
  to 64 MiB. The platform handles CF_HTML framing on Windows and MIME offers on Linux;
  callers supply raw HTML fragments. `clip_has` probes and `clip_get` retrieves each format.
  `CLIP_DOCX` carries `doc_io`'s versioned document fragment, not an arbitrary DOCX file.
  `doc_core::Document::copy_story_range` and `Editor::replace_story_fragment` preserve
  formatting, sources and note bodies with atomic document history. `doc_io` also provides
  the bounded RTF reader/writer and HTML writer. See `apps/jfWord/rich_ui.rs` for shortcut
  interception and format preference. Ordinary textboxes continue to paste plain text;
  plain-text/image writes clear older rich representations. Scripts use an isolated
  in-process clipboard and never change the user's system clipboard.
* `textbox_presented(id, placeholder, &mut text, cap_flags, presentation)` uses the same
  native editing, selection, clipboard and IME path with an explicit `TextboxPresentation`.
  It draws a borderless field with the supplied font, physical-pixel font size (24.8), line
  height, baseline, foreground/background and underline. The app owns page geometry and
  document history; ordinary `textbox` calls retain their existing style and rendering.
* `RibbonGroup::new(id, caption, preferred_width, compact_width)` describes a command group
  in DIP; reserve `id..=id+3`. Start with `ribbon_begin(id, &groups)`, then call
  `ribbon_group_begin(&layout, index, &group)` for each group. Emit controls only when it
  returns `Some(mode)`, followed by `ribbon_group_end(mode, caption)`. Finish with
  `ribbon_end()`. Groups first shrink, then collapse into labeled flyouts from the right.
  `ribbon_button` provides compact icon actions; `ribbon_tile` adds typography previews.
  The app still provides real command availability and actions. See `apps/jfWord/ribbon_ui.rs`.
* Flyouts of your own: a button's `EV_ACT` -> `popup_open(id)` (drop-down buttons do this
  themselves); then `flyout_begin(id, FL_*)` -> `true`: emit `menu_item(id, label)`,
  `menu_cmd(cmd)`, `menu_sep()` or any widgets; always `flyout_end()`:

  ```rust
  ui.combobox(ID_SHAPE, &mut self.shape, &["Kreis", "Quadrat", "Dreieck"]);
  ui.dropdown_button(ID_MORE, "Mehr");
  if ui.flyout_begin(ID_MORE, FL_BELOW) {
      if self.more != 0 {
          ui.next_flags(ST_CHECKED);
      }
      if ui.menu_item(ID_MORE_A, "Option A") & EV_ACT != 0 {
          self.more ^= 1;
      }
      ui.menu_sep();
      ui.menu_cmd(CMD_SAVE);
  }
  ui.flyout_end(); // always, also when flyout_begin returned false
  ui.button(ID_RCLICK, "Rechtsklick hier");
  ui.context_menu(&M_CTX); // right click / menu key / Shift+F10 on the last widget
  ```

* Drop-downs: `dropdown_button`, `split_button` (main part `EV_ACT`, chevron opens popup `id`),
  `combobox(id, &mut sel, list)`; `menu_popup(menu)` opens a menu at the pointer.
* Timers: `ui.timer(id, ms)` -> `App::command(UIC_TIMER, id)` once; start it again for a
  periodic timer. Library timers (caret, tooltips) arrive as well: ignore ids you did not start.

## 7. Dialogs, windows, title bar, files

**Content dialogs** are drawn on the window (smoke layer, modal). The library keeps the `open`
flag (`Shared<i32>`: closing by Esc or a button clears it):

```rust
// state: dlg_open: Shared<i32> (Shared::new(0)); open it with self.dlg_open.set(1)
let dlg = self.dlg_open.clone();
if ui.dialog_begin(ID_DLG, "Änderungen speichern?", &dlg) {
    ui.text(TS_BODY | TX_WRAP, "Möchten Sie die Änderungen speichern?");
    match ui.dialog_buttons(Some("Speichern"), Some("Nicht speichern"), Some("Abbrechen")) {
        DLG_PRIMARY => ui.cmd_run(CMD_SAVE, 0), // do the work in App::command
        DLG_SECONDARY => ui.quit(),
        _ => {}                                 // DLG_NONE, DLG_CLOSE
    }
}
ui.dialog_end(); // always
```

* Emit dialogs and windows at the end of `frame` (on top of everything else).
  `ui.modal_active()` tells whether a dialog is open (for shortcuts of your own).
* **Unsaved changes**: in `App::command(UIC_CLOSE)` return `true` to keep the window, open your
  "save?" dialog, and call `ui.quit()` when done:

  ```rust
  UIC_CLOSE => {
      if !self.modified {
          return false; // not handled: the window closes
      }
      self.dlg_open.set(1); // ask first; true keeps the window open
  }
  ```

* Info bars: `infobar(id, IB_INFO | IB_SUCCESS | IB_WARNING | IB_ERROR, title, msg)` ->
  `EV_ACT` when its close button was clicked. Status bar: `statusbar_begin()` ... texts,
  `separator()` ... `statusbar_end()` at the end of the main box.
* **Secondary windows** (tool windows, dialog windows) are real OS windows, built by the same
  `frame`, each with its own UI state (`legacy/ASMLIB/docs/modules/WINDOWS.md` describes the
  model; the API is the same):

  ```rust
  // state: win_open: Shared<i32>, win_rect: Shared<WinRect> (WinRect::new(0, 0, 260, 0, 0))
  let (wo, wr) = (self.win_open.clone(), self.win_rect.clone());
  if ui.window_begin(ID_WIN, "Werkzeuge", Some(&wo), WF_TOOL | WF_AUTOHEIGHT, Some(&wr)) {
      ui.box_begin(0, BOX_V | PAD_12, 8);
      ui.slider(ID_WIN_SIZE, &mut self.size, &RG_SIZE);
      ui.box_end();
  }
  ui.window_end(); // always
  ```

  Flags `WF_TOOL`, `WF_DIALOG` (+ `WF_MODAL`; `dialog_buttons` then make the footer),
  `WF_RESIZABLE`, `WF_NOCLOSE`, `WF_AUTOHEIGHT`, ...; the library writes the user's placement
  back into the `WinRect`. `window_place`, `window_state`, `window_activate`, `window_destroy`.
* **Title bar**: `AF_CUSTOM_TITLEBAR` in the `AppDesc`, then as the first row of the main box
  (caption buttons, dragging, snap layouts on Windows, resize grips on Linux):

  ```rust
  let t = String::from(ui.title()); // title() borrows ui
  ui.titlebar_begin(&t, IC_BRUSH);
  // optional widgets in the title bar (search box, buttons)
  ui.titlebar_end();
  ```

  `ui.set_title(s)` changes the window title.
* **File dialog**: `filedlg(id, FD_OPEN | FD_SAVE | i << FD_EXT_SHIFT, &open, &mut path, cap,
  exts)` -> `EV_ACT` when a file was chosen; `path` then holds an absolute path with `/`
  separators (Windows accepts them). It shows the system dialog (Explorer on Windows, the
  desktop's file chooser through xdg-desktop-portal on Linux) and falls back to the library's
  own dialog when there is none, in headless runs, or with `jfgui_FILEDLG=builtin`. Ids
  `id .. id + 0x3FFF` belong to it. `i` preselects a file type of `exts`:

  ```rust
  static EXTS: [&str; 2] = ["png=PNG-Bild", "jpg;jpeg=JPEG-Bild"];
  const ID_FILEDLG: u32 = 0x0010_0000; // .. + 0x3FFF

  // in frame:
  let open = self.fd_open.clone();
  if ui.filedlg(ID_FILEDLG, self.fd_mode, &open, &mut self.path, 1040, &EXTS) & EV_ACT != 0 {
      ui.cmd_run(CMD_FILE_DONE, self.fd_mode); // load / save in App::command
  }
  // CMD_OPEN in App::command: self.fd_mode = FD_OPEN; self.fd_open.set(1);
  ```

## 8. Files, settings, images

* **Files** (`plat`): `read_file(path) -> Option<Vec<u8>>`, `write_file(path, data) -> bool`
  (atomic: a temporary file + rename), `file_exists` (0 / 1 file / 2 directory), `mkdir`
  (with parents), `delete`, `rename`, `dir_list(path, &mut |name, is_dir, size| ...)`,
  `plat::File` for random access; `config_dir()`, `temp_dir()`, `cwd()`, `args()`,
  `arg_value("--open")`, `env(name)`, `time_us()`, `date()`.
  `file_exists_checked` distinguishes a missing path (`Some(0)`) from access errors
  (`None`); Linux also rejects special files. `FileLock::acquire(path)` returns a
  nonblocking exclusive sidecar guard or `None` for busy/error. Keep the guard until
  the read/compare/write transaction completes and leave the sidecar on disk: deleting
  it can let another process lock a different file at the same pathname.
* **Document sources**: `doc_core::SourceLibrary` stores an independent bibliography and
  its ID watermark; `SourceLibraryEditor` adds atomic edits and bounded undo/redo.
  `doc_io::LibraryStore` owns the strict native XML codec, bounded reads, lock/conflict
  checks and atomic backup/write sequence. `prepare_recovery` captures a reviewed
  main/backup snapshot; its token restores only if both files remain unchanged.
  Profile paths, confirmation dialogs and document-to-library selection belong in the app.
* **Document notes**: `doc_core::Note` owns rich paragraphs; an `Inline::NoteReference`
  anchors it exactly once in the main body. Use `Edit::InsertNote`, `SetNoteKind` and
  `RemoveNote` for atomic history. Paragraph edits, search and citation/source operations
  also accept note-body paragraph IDs. `note_number` derives separate decimal sequences
  from anchor order; never store that display number as the note's identity.
  `doc_io` owns the bounded DOCX note parts and their package relationships, while
  `doc_layout` reserves footnotes at page bottoms, continues long notes and appends
  endnotes. Render each `Page.separators` item alongside its lines. Note bodies have
  normal paragraph hit-test IDs, so apps can navigate from the preview to their editor.
* **Settings**: `base::Ini` reads and writes `[section]` / `key=value` files and keeps comments
  and unknown lines:

  ```rust
  let path = format!("{}/MeinWerkzeug/settings.ini", plat::config_dir().unwrap_or_default());
  let mut ini = base::Ini::load(&path);       // a missing file is empty
  let size = ini.get_int("view", "size", 10);
  ini.set_int("view", "size", size + 1);
  ini.save(&path);                            // atomic, creates the folder
  ```

  Save settings in `App::command(UIC_EXIT)` (sent only after a real window ran) and never in
  test runs.
* **Images** (`img_core::io`): the runtime registers PNG, JPEG, BMP and GIF at start-up and
  connects the file functions. For every other format call `img_all::use_all_codecs()` once in
  `App::init` (TIFF, TGA, WebP, AVIF, HEIC, JPEG XL, JPEG XR, DDS, PDN).

  ```rust
  match img_core::io::load_ex(path, None, None) {   // any registered format -> FMT_PARGB
      Ok(sf) => {
          self.photo = Some(Rc::new(sf));
          self.photo_ver += 1;
      }
      Err(e) => self.error = Some(format!("Die Datei konnte nicht geöffnet werden ({}).", e)),
  }

  let mut opt = img_core::io::opt_init(img_core::io::format_from_path(path)); // paint.net's defaults
  opt.quality = 90;
  if img_core::io::save_opt(path, sf, Some(&opt)) != 0 { /* ERR_* */ }
  ```

  * Classic calls: `load(path)`, `save(path, &sf)`, `decode(data)`, `save_ex(path, &sf, quality)`
    (PNG / JPEG / BMP / GIF by extension).
  * Extended: `detect(data, hint)`, `probe(data, Some(&mut meta))` (header only),
    `decode_ex(data, Some(&dec), Some(&mut meta))`, `encode(&sf, Some(&opt))`, `save_opt`.
    `ImgDec` chooses straight output (`IMGD_STRAIGHT`), the frame / page, a pixel budget and
    which metadata blobs to keep; `ImgMeta` receives size, format details, frames, ICC / EXIF /
    XMP; `ImgOpt` carries quality, effort, flags (`IOF_*`), bit depth, chroma, DDS / TIFF
    choices and the metadata to embed. Each codec's options are documented in its section of
    API.md (e.g. `img_jxl`: effort, progressive, photon noise).
  * `format_info(fmt)`, `format_from_path(path)`, `register_dyn(...)` (runtime formats of
    plugins). Errors are the `ERR_*` codes of `img_core`.
  * Metadata and colour: `img_core::meta` (conversions `convert` / `surf_convert` between
    `FMT_PARGB` and `FMT_ARGB`, `all_opaque`), `img_core::exif`, `img_core::icc` /
    `icc_xform` (colour management, `legacy/ASMLIB/docs/modules/COLOR_MANAGEMENT.md`).

## 9. Threads

`plat` has a worker pool (`legacy/ASMLIB/docs/modules/THREADS.md` describes the design; the
Rust pool is the same). Results must not depend on the number of threads: give every item a
fixed piece of work.

```rust
/// brighten every pixel of a straight-ARGB surface on all cores
fn brighten(sf: &mut gfx::Surface, add: u8) {
    let stride = sf.stride as usize;
    let w = sf.w as usize;
    // one item per band of 64 rows; each item locks only its own band
    let bands: Vec<SpinLock<&mut [u8]>> = sf.px.chunks_mut(stride * 64).map(SpinLock::new).collect();
    let item = |i: usize, job: &plat::JobCtl| {
        let mut band = bands[i].lock();
        for row in band.chunks_mut(stride) {
            if job.cancelled() {
                return;
            }
            for px in row[..w * 4].chunks_exact_mut(4) {
                for c in &mut px[..3] {
                    *c = c.saturating_add(add);
                }
            }
        }
    };
    let _ = plat::parallel_for(bands.len(), &item); // returns when every item is done
}
```

(`SpinLock` is `rt::sync::SpinLock`.)

* `parallel_for(count, &f)` runs `f(index, job)` on the workers and the calling thread; nested
  loops are fine; `job.cancel()` skips the remaining items -> `Err(Cancelled)`.
* Background work: `plat::Job` -- `set(count, f)` with a `'static + Send + Sync` closure (share
  data through `alloc::sync::Arc`), `start()`, `done()`, `wait()`, `cancel()`,
  `items_done()`. When it finishes the pool calls `plat::wake()`, which arrives as
  `App::command(UIC_WAKE, 0)`:

  ```rust
  struct Loader {
      job: plat::Job,
      result: Arc<SpinLock<Option<Result<gfx::Surface, i32>>>>,
  }

  impl Loader {
      fn start(&mut self, path: String) {
          let slot = self.result.clone();
          self.job.set(1, move |_, _| {
              let r = img_core::io::load_ex(&path, None, None);
              *slot.lock() = Some(r);
          });
          self.job.start(); // UIC_WAKE when it is done
      }
      // in App::command(UIC_WAKE)
      fn poll(&mut self) -> Option<Result<gfx::Surface, i32>> {
          if !self.job.done() {
              return None;
          }
          self.result.lock().take()
      }
  }
  ```

* `thr_count()` workers, `thr_self()` a per-thread slot index; `--threads N` on the command line
  sets the pool size. Test scripts wait for jobs with `sync` once the app installs
  `ui.sync_hook_set(Some(f))` (`f() -> true` while work is pending).

## 10. Effects

The `fx` crate holds paint.net's effects and adjustments behind one contract (its crate header
in API.md; `legacy/ASMLIB/docs/modules/EFFECTS_*.md` describe the algorithms):

```rust
fx::use_all(); // once (App::init): register every built-in effect

let Ok(src) = img_core::meta::surf_convert(sf, gfx::FMT_ARGB) else { return }; // straight ARGB
let Some(desc) = fx::find("gaussian_blur") else { return };
let mut params = fx::Params::default();
fx::defaults(desc, &mut params);
params.set_i(0, 8); // "radius": property 0 lives in slot 0
let roi = [0, 0, src.w, src.h];
let args = fx::FxArgs::new(&src, roi, &params);
let aux = match desc.prepare {
    Some(prepare) => match prepare(&args) {
        Ok(a) => a,
        Err(_) => return,
    },
    None => None,
};
let args = fx::FxArgs { aux: aux.as_deref(), ..args };
let mut out = gfx::Surface::new(src.w, src.h, gfx::FMT_ARGB);
let mut dst = fx::Dst::of_surface(&mut out, roi);
if (desc.render)(&args, &mut dst) == 0 {
    img_core::meta::convert(&mut out, gfx::FMT_PARGB);
    // show / store `out`
}
```

* `FxDesc` describes an effect: `name` (ASCII key), `flags` (`FXF_*`), `props` (`FxProp`: type
  `FP_*`, slot, range, default, choices), `prepare` (statistics, LUTs; once) and `render`.
* `render` may be called for many disjoint rectangles from several threads at once
  (`fx::tiles` splits one surface into `Dst`s; `plat::JobCtl` is a `fx::Cancel` for
  `FxArgs::job`); `FXF_BANDS` wants full-width bands, `FXF_WHOLE` one call. The output never
  depends on the split. Pass a selection mask as `FxArgs::sel` and blend the result back through
  it with `gfx::straight::lerp_mask_row`.
* `fx::register(&GROUP)` adds effects of your own; PaintCraft plugins add effects through the
  plugin ABI (section 14).

## 11. Styles, theme, fonts

* The look comes from one style crate linked at build time: `winui` (default), `classic`,
  `macos`, `kde`. `apps/NAME/STYLE` sets an app's style, `build_rs.py ... --style S` overrides
  it. Only the linked style is in the executable. [STYLES.md](STYLES.md) has the model and how
  to write one.
* Colours of your own drawing come from the theme tokens: `ui.color(T_TEXT)`, `T_ACCENT`,
  `T_CTRL`, `T_BG`, ... (`ui::consts`), so they work in every style, light and dark.
  `ui.theme_set(0 | 1 | 2)` (light, dark, system), `ui.theme_dark()`, `ui.accent_set(rgb)`;
  command line `--theme light|dark`, `--accent RRGGBB`.
* Fonts: `FONT_REG`, `FONT_SEMI`, `FONT_BOLD`, `FONT_ICON` of the style; icons are `IC_*` code
  points (`ui::keys`), mapped by every style's icon font. `ui.ts_font(TS_*)` selects a text
  style in the text engine `ui.text` (for measuring with `ui.text.width(s)`).

## 12. Testing -- always verify

* **Command line of every app**: `--shot FILE.png` (headless render, no window system needed),
  `--script FILE.uis` (headless UI test, exit code 0 = every `expect` met), `--dump` (widget tree
  as JSON lines), `--verify-dirty` (compare every incremental redraw with a full redraw),
  `--size WxH` (DIP), `--scale PERCENT`, `--theme light|dark`, `--accent RRGGBB`, `--anim off`,
  `--log-events`, `--auto-close MS`, `--threads N`, `--simd sse2` (no AVX2 paths). Environment:
  `jfgui_DEBUG=1` (layout / render trace), `jfgui_CSD=1`, `jfgui_FILEDLG=builtin`,
  `jfgui_THEME`, `jfgui_SCALE` (Linux), `jfgui_BACKEND`.
* **Never start an app without `--shot`, `--script` or `--auto-close`**: a plain start opens a
  real window on the user's desktop. PaintCraft saves its settings when a real window closes:
  every real-window run needs `--config build/<name>/cfg` (the capture tools set
  `PaintCraft_CONFIG_DIR`).
* Linux binaries run through WSL:
  `wsl.exe -d Ubuntu-24.04 --cd <repo> -e ./build/linux/NAME --script apps/NAME/tests/x.uis`
  (`jfgui_BACKEND=x11` through `-e env jfgui_BACKEND=x11 ./build/linux/NAME ...`).
* Look at screenshots (`shot`) with an image viewer or the Read tool; test dark and light and
  150 %. Real windows: `python tools/wincap.py build/win/NAME.exe --out cap.png`,
  `python tools/wslcap.py --title T --out cap.png -- build/linux/NAME`.

**The script language** (`ui/auto.rs`; one command per line, `#` starts a comment, coordinates
in DIP):

| Command | Effect |
|---|---|
| `click S [l\|r]`, `rclick S`, `dblclick S`, `move S`, `down S [l\|r\|m]`, `up [S] [l\|r\|m]` | pointer |
| `drag S1 S2 [steps] [shift] [ctrl] [alt] [right\|middle]` | drag with modifiers / other buttons |
| `drag ... pen P0 P1`, `drag ... eraser P0 P1` | a pen stroke, pressure P0 -> P1 (%) |
| `pointer mouse \| pen [P [TX TY]] \| eraser [P ...] \| touch` | pen data of the following pointer events |
| `wheel S DY` | raw wheel units (120 = one notch) |
| `key CHORD...` | `key ctrl+s`, `key tab shift+tab enter esc left f4`, also `[ ] , . - + /`, `num0`..`num9`, `numadd` ... |
| `type "text"` | typed text (`"\n"` presses Enter) |
| `focus S` | keyboard focus to a widget (focus ring shown) |
| `cmd ID [ARG]` | run a command as if a menu row was chosen |
| `wait MS` | advance the virtual clock (16 ms frames) |
| `size W H`, `theme dark\|light`, `scale P [window ID]` | window size, theme, dpi |
| `expect S exists\|missing\|hover\|focused\|checked\|unchecked\|disabled\|enabled\|selected\|open\|pressed` | widget states |
| `expect S value N`, `expect S max N`, `expect S text "s"`, `expect S rect X Y W H` | values, texts, rectangles (DIP) |
| `shot FILE.png [window ID\|desktop] [X Y W H]`, `dump [FILE]`, `print TEXT`, `bench N`, `quit` | output |
| `window ID\|main\|any` | selectors only look into that window |
| `expect window ID open\|closed\|visible\|active\|modal`, `expect window ID rect X Y W H` | secondary windows |
| `close window ID`, `movewin ID X Y`, `resizewin ID W H`, `activate ID` | the user moves / closes windows |
| `sync [MS]` | wait until no background job is pending (`sync_hook_set`) |
| `expect_warn`, `expect_nowarn` | a `UIWARN` was / was not printed since the last check |
| `drop X Y FILE...` | files dropped at (X, Y) of the scoped window (`UIC_DROP`) |
| `clip text "s"`, `clip file F`, `clip dib F`, `clip clear`, `clip save F [bmp]`, `clip expect none\|text\|image\|png` | the headless clipboard (never the real one) |
| `clip utf8 F` | exact UTF-8 text clipboard bytes from a file (up to 4 MiB), including CR/LF and long paste payloads; opt-in Rust extension |
| `clip data FORMAT F [FORMAT F ...]` | publish one to four representations together; formats: `text`, `html`, `rtf`, `docx`; opt-in Rust extension |
| `clip save F text\|html\|rtf\|docx`, `clip expect html\|rtf\|docx\|text-only` | save exact bytes or check rich-format availability; `text-only` also checks absence of rich representations |

**Selectors** `S`: `#123` (widget id), `"Label"`, `kind:"Label"` (`button:"OK"`, `dialog:"Öffnen"`,
`text:"Klicks: 2"`), `X,Y` (a point in DIP). A label must match exactly one widget (plain text is
ignored when a control with the same label exists; with matches in several windows the topmost
window wins, then the topmost layer, so an ambiguous label resolves to the dialog or popup).
Failed expectations print `FAIL line N: <command> -> <detail>` and the script goes on.

```
# apps/template/tests/smoke.uis
click "Klick mich"
click "Klick mich"
expect text:"Klicks: 2" exists
click "Zurücksetzen"
expect text:"Klicks: 0" exists
shot build/template.png
```

* A first line `#!args --open file.png` adds program arguments when `build_rs.py test` runs
  the script (pass them yourself when you run it by hand).
* `--dump` / `dump` print the widget tree (kind, id, label, rect in DIP, states, value): use it
  to find ids and positions without looking at pixels. Prefer `#id` and labels over coordinates.
* Popups of secondary windows may leave them: their dump rectangles can be negative or beyond
  the window; `shot F desktop` shows them (`WINDOWS.md` §7.9, §7.10). `UIC_EXIT` is never sent
  in `--script` / `--shot` runs.

**Test commands**

```
python build_rs.py test --quick     # lint, build every app (both targets), every .uis script
python build_rs.py test             # ... plus parity with the assembly reference
python build_rs.py check            # the rules lint alone
python tools/gendoc.py --check      # docs/API.md matches the doc comments
```

* Run new scripts with `--verify-dirty` too (`build_rs.py test` does not add it): it reports
  missed pixels with their bounding box.
* **Parity** (apps that also exist in `legacy/`): `python tools/parity.py app NAME` runs the
  shot matrix (light / dark × 100 / 150 %, with dumps) and every script on the assembly
  reference and the Rust build and compares pixels, dumps, stdout and exit codes;
  `apps/NAME/parity.txt` can mark pending scripts or masked areas. The reference binaries live
  in `build/rwref` (`jfgui_REF=<main checkout>/build/rwref` from a worktree);
  `parity.py ... --style S` compares another style (STYLES.md).
* Library check tools: `tools/*_check.py` run against the Rust build with `--out build`
  (codec, kernel and effect checks: `codec_check`, `png_check`, `jxl_check`, `blend_check`,
  `fx_check`, ...); `python tools/diffcheck.py TOOL` additionally compares every result byte
  for byte with the assembly build.

## 13. Pitfalls

* **Ids must be unique** in a frame. Two widgets with the same id share hover / press state:
  clicks seem to get lost, or a click on one acts on the other (a text box below a segmented
  control whose segments take `id + 1 ..`). Scripts (and `jfgui_DEBUG=1`) print a `UIWARN`
  for an id two widgets of one pass share, with both positions; `expect_nowarn` fails on it.
* `ui.canvas(...)` records the version *before* you react to its input. When handling the
  returned `UiInput` changes the content, bump the version and call `ui.repass()`; otherwise
  the dirty-region renderer shows the change one frame late (`--verify-dirty` reports it).
* `ui.canvas`, `ui.title()` and a few others return borrows of `ui`: clone / copy the value
  before the next `ui` call.
* A surface shown with `image` / `draw_image` / a `Draw` object is compared by its version:
  bump it on every pixel change.
* `flyout_begin` / `flyout_end`, `dialog_begin` / `dialog_end`, `window_begin` /
  `window_end`, `box_begin` / `box_end`, `list_begin` / `list_end` always come in pairs, also
  when the begin call returned "closed".
* Do structural work in `App::command`, not in `frame`: `frame` may run several times per
  input event (settle passes).
* Overlays over a canvas: wrap them in `ui.clip_push(rect)` / `ui.clip_pop()` so they never
  spill onto neighbouring panels; boxes placed with `next_rect` are fine next to normal children.
* Panics end the program with exit code 3 and a crash line on stderr; decoders return `ERR_*`
  for bad input instead of panicking.
* `shot` / `dump` in a script do not create folders: `build_rs.py test` creates them for the
  scripts it runs; create them yourself for runs by hand.
* Windows paths: `/` works everywhere; the file dialog returns `/` separators.
* Python helper scripts that patch sources: `Path.write_text` writes CRLF on Windows (use
  `write_bytes`), shell heredocs turn `\\n` into newlines.

## 14. Plugins

PaintCraft loads effects, file types and shapes from plugins (DLL / .so). Plugins written in Rust
use the crate `lib/plugin` (safe `Effect` / `FileType` traits over the PNAPI ABI) and are
built with `python build_rs.py plugin NAME`; see `plugins/README.md` and
`apps/PaintCraft/docs/PLUGINS.md`.

## 15. Data views and app shells

Business apps (BackupCraft's console is the example in the repository) are made of a few
recurring parts. They live in `lib/ui/data.rs`, `shell.rs`, `chart.rs`, `notify.rs` and
`datepick.rs`, and every style draws them: the default painters use the theme tokens, a style may
bring its own (`StyleData`, `StyleShell` in [STYLES.md](STYLES.md)).

**Grids** are virtualized tables with sorting, resizable columns and multiple selection by key.
The app keeps a `GridState`, passes the row keys in display order and emits only the rows the
grid asks for:

```rust
let cols = [GridCol::new("Name", 240).flags(GC_FILL), GridCol::new("Größe", 110)];
let keys: Vec<u64> = self.rows.iter().map(|r| r.key).collect();
let (first, end) = ui.grid_begin(ID_GRID, &mut self.grid, &cols, &keys);
for i in first..end {
    let r = &self.rows[i as usize];
    ui.grid_row(&mut self.grid, i);
    ui.grid_text(&self.grid, 0, &r.name);
    ui.grid_text(&self.grid, 1, &r.size);
}
let ev = ui.grid_end(&mut self.grid);   // ev.ev: EV_ACT selection, EV_DOUBLE / EV_RCLICK on ev.row
if ev.sort_changed { /* sort self.rows by self.grid.sort */ }
```

`grid_cell(st, c)` opens a cell for any content (an icon and a text, a badge). The selection
survives reloads because it is kept by key (`self.grid.sel`, `selected_in_order`).

**Trees**: the app flattens its model into `TreeRow`s (key, depth, children, optional tri-state
check box, icon, label, detail) using `TreeState::expanded`; `ui.tree(id, &mut st, &rows,
row_h)` reports opened and closed nodes (load children lazily) and toggled checks.

**Status**: `badge(kind, label)`, `status_dot(kind)`, `meter(id, value, max, kind)` with the
`BADGE_*` kinds (neutral, info, ok, warn, crit, accent) in the token colours (`T_OK`, `T_WARN`,
`T_CRIT`, `T_INFO` and their `_BG`).

**Shell parts**:

| Function | Part |
|---|---|
| `nav_begin(id, &mut NavState)`, `nav_section`, `nav_item(id, icon, label, selected, badge)`, `nav_footer`, `nav_end` | navigation pane, wide or compact (icons only) |
| `command_bar(id, &[CmdItem]) -> Option<u32>` | primary and secondary commands; what does not fit goes into an overflow menu |
| `split_begin(id, pos, vertical)`, `split_next`, `split_end(&mut pos)` | two panes with a draggable splitter (master / detail) |
| `page_header`, `breadcrumb`, `key_value`, `empty_state`, `link` | page titles, paths, label / value rows, empty pages with a call to action, text links |
| `segmented(id, &mut sel, items)` | segmented control (filters, schedule kinds) |
| `wizard_steps(id, steps, current, states)` | the step list of a full-page wizard |
| `toast(kind, title, msg, ms)`, `toast_host()` | notifications; call `toast_host` once per frame, last |
| `spinner(id, size)`, `progress_indeterminate(id)` | waiting |
| `date_picker`, `time_box`, `weekday_toggles` | dates, times, weekday masks (schedules) |

**Charts**: `sparkline(id, values, color)`, `bar_chart(id, labels, &[BarSeries], stacked)`,
`donut(id, &[DonutSlice], center, sub)` and `legend(items)`; colours 0 take the style's chart
palette.

**Languages**: `ui::i18n::set_language(ui::i18n::LANG_EN)` translates the library's own texts
(file dialog, shortcuts, dates); `ui::i18n::Catalog::from_tsv(include_str!("locales/ui.tsv"))`
gives an app a German-source catalog of its own (BackupCraft: `src/tr.rs`).

## 16. Several programs per app, app-local crates

An app folder builds one GUI program from `src/main.rs`. `apps/NAME/BINS` adds further
programs, one line each -- name, entry file, `cli` (console program without a style) or `gui`:

```
bc-server   server/main.rs   cli
bc-agent    agent/main.rs    cli
bcctl       ctl/main.rs      cli
```

They are built with the app (`build/<target>/bc-server[.exe]`), started with
`python build_rs.py run BackupCraft --bin bc-server -- --data D`, and packaged next to the GUI
program (`tools/package.py`). Code that several programs share goes into app-local crates,
`apps/NAME/crates/<crate>/lib.rs`: they are built, linted (`#![no_std]`,
`#![forbid(unsafe_code)]`) and linked like the library crates, and their dependencies are the
crates their sources name.

Console programs use `rt::main!(run())` instead of `ui::run`, write with `plat::write_stdout` /
`write_stderr` and read arguments with `plat::args()` / `plat::arg_value`. For background
services, `plat::service::run_service(name, run)` runs under the Windows service control
manager (and as a plain process elsewhere), `install` / `start` / `stop` / `uninstall` / `state`
manage the Windows service or the systemd unit, and `stop_requested()` tells the program to
finish.

## 17. Network programs

The building blocks of BackupCraft's server, agent and command-line client are library crates
that any program can use (all `no_std`, no `unsafe`, see [API.md](API.md)):

* `netio::Server::start(&addr, ServerConfig, handler)` -- an HTTP/1.1 server with a worker pool,
  keep-alive and limits; `start_with(.., Some(wrap))` puts TLS (or anything else) under it and
  passes the identity it proved (a client certificate) to the handler.
* `netio::Client` / `client::Session` -- requests with timeouts and size limits, kept-alive
  sessions; `with_connector(netio::trust::connector(..))` for https to a server with its own CA
  (a CA file, a pinned fingerprint, or trust on first use).
* `netio::tls` -- TLS 1.3 (ChaCha20-Poly1305, X25519, Ed25519 certificates, mutual
  authentication); `netio::x509` builds and checks the certificates of a private CA.
  `Trust::System(extra)` connects to public servers: TLS 1.3 (AES-GCM or ChaCha20, X25519 or
  P-256) or TLS 1.2 with ECDHE, RSA / ECDSA / Ed25519 certificates, the chain built and checked
  against the system's authorities (Windows CryptoAPI, Linux CA bundle) and extra CAs;
  `netio::trust::system_connector(extra)` gives the connector for `Client`.
* `netio::smtp::send(&SmtpConfig, &Mail)` -- e-mail (STARTTLS, TLS, or plain on loopback;
  AUTH PLAIN / LOGIN only over TLS; text and HTML parts, RFC 2047 subjects).
* `netio::ssh::Client` -- run commands on a machine over SSH (curve25519, ssh-ed25519,
  chacha20-poly1305, strict key exchange), host key pinned by fingerprint.
* `netio::dns::resolve` -- names to addresses (Linux: hosts file, resolv.conf, UDP and TCP).
* `datastore::Store` -- JSON collections in a crash-safe journal database with transactions;
  `Journal` for append-only logs.
* `cas::Repo` -- content-addressed, deduplicated, encrypted repositories with signed snapshots.
* `crypto` -- hashes (SHA-2 incl. SHA-384, BLAKE2b, BLAKE3), MACs, KDFs, Argon2id, AEAD
  (ChaCha20-Poly1305, XChaCha, AES-GCM), X25519, P-256 key exchange, Ed25519, RSA (PKCS#1 v1.5,
  PSS) and ECDSA P-256 / P-384 verification (randomness comes from the caller:
  `plat::random_bytes`).
* `compress` -- LZ4 and zstd (`compress::zstd::compress(data, level)` / `decompress`; levels
  1 to 19, single-segment frames with checksums); `Codec` picks one by name.
* `plat::watch::Watcher::new(&roots)` / `take(ms)` -- the changed paths below folders
  (ReadDirectoryChangesW / inotify, recursive; `overflow` when events were lost).
* `plat::net::UnixListener::bind(path)` / `connect_unix(path, ms)` -- Unix domain sockets (Linux,
  Windows 10 1803 and later) with the stream calls of `TcpStream`.
* `plat::process::Command { program, args, cwd, env }.spawn()` -- a child process with pipes
  (no shell); dropping the `Child` ends its process tree.
