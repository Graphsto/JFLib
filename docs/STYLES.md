# Styles -- how the library looks

The `ui` crate is style-neutral: widgets, layout, input, windows, files, codecs, effects.
**How widgets look** comes from exactly one *style*, a crate that is linked into the program at
build time. Nothing is chosen at run time and only the linked style ends up in the executable.

| Style | Crate | Look | Used by |
|---|---|---|---|
| `winui` (default) | `styles/winui` | Windows 11 / WinUI 3 Fluent: rounded controls, Segoe UI, accent pills, Mica colours | paint, PaintCraft, every app without a `STYLE` file |
| `classic` | `styles/classic` | Windows 2000 / Office 97: grey 3D bevels, square controls, Tahoma, navy selection, gradient title bars | `--style classic` |
| `macos` | `styles/macos` | macOS Sonoma: grey window, white push buttons, blue default button and selection, pop-up buttons with the blue chevron square, overlay scrollers, traffic-light title bars (close at the left, centred titles), dialog buttons right-aligned with the default last; Segoe UI stands in for SF Pro | `--style macos` |
| `kde` | `styles/kde` | KDE Plasma Breeze: light buttons with an outline, highlight-blue hover and focus frames, Breeze window decoration (chevron buttons, round red close on hover), dialog buttons right-aligned with the default first; DejaVu Sans | `--style kde` |
| `jfoffice` | `styles/jfoffice` | Teal/paper office palette, full DejaVu Sans cmap, original basic icons; shared KDE control geometry | jfWord; `--style jfoffice` |
| `backupcraft` | `styles/backupcraft` | WinUI layering with KDE Breeze framing: slate window / panel / card layers with soft shadows, framed inputs, outlined push buttons with an accent outline on hover, accent-filled primary buttons and checks, pills for the selected row and navigation item, full-width tab underlines, visible scroll tracks; petrol accent (#0B7285 light, #3BC9DB dark), Inter, compact 32 DIP controls, own vector icons | BackupCraft; `--style backupcraft` |

The installer lets the user pick one of them for paint and PaintCraft ([PACKAGING.md](PACKAGING.md)).
The four Rust styles render pixel-identical to the assembly styles of `legacy/ASMLIB/styles`.
The new `jfoffice` style has no assembly counterpart. Its current scope and asset provenance
are described in [styles/jfoffice/README.md](../styles/jfoffice/README.md). `backupcraft` has no
assembly counterpart either; its fonts (Inter, OFL) and icons (own artwork) may be redistributed.

## Choosing a style

* `apps/NAME/STYLE` -- one word, the style of that app (`winui`, `classic`, `macos`, `kde`). No
  file (or an unknown name) = `winui`.
* `python build_rs.py app NAME --style classic --out build/classic` builds one app with
  another style (use a separate `--out`: the executable name stays the same).
* `build_rs.py` finds every `styles/NAME/lib.rs` and builds it as the crate `style_NAME`
  (it depends on `ui` only). An app is compiled with `--extern style=<out>/<t>/libstyle_NAME.rlib`
  and passes `&style::STYLE` to `ui::run`; the core reaches the style only through that
  `&'static dyn ui::Style` (one indirect call per widget). The core is not recompiled per
  style, so switching styles only relinks the app.

```
python build_rs.py app gallery --style classic --out build/classic
build/classic/win/gallery.exe --shot g.png --theme dark
```

## The contract (`lib/ui/style.rs`)

`ui::Style` is split like the files of a style. A style crate implements the eight traits below
for one unit struct and exports it as `pub static STYLE`; two more families -- data views and
app shell parts -- come with token-based defaults (next section):

| Trait | Content | Style file |
|---|---|---|
| `Style` | theme tokens (light / dark), accent ramp, type ramp, the four fonts and their names | `lib.rs` |
| `StyleBasic` | focus ring, buttons, check boxes, radios, switches, sliders, progress bars, separators | `basic.rs` |
| `StyleValue` | value sliders over gradient tracks, angle dial, pan control | `value.rs` |
| `StyleEdit` | text box metrics, frame and vertical scroll bar | `edit.rs` |
| `StyleList` | focus rectangle, scroll bars, list rows, drop line, tabs, expanders | `list.rs` |
| `StylePopup` | flyout surface, menu rows and separators, menu bar items, drop-down / split buttons, combo boxes, tooltips | `popup.rs` |
| `StyleChrome` | layout metrics (`metric(SM_*)`), dialogs, info bars, status bar, caption buttons, library title bar, window border | `chrome.rs` |
| `StyleMisc` | swatches, colour preview / marker / wheel, box decorations (`BOX_BG`, `BOX_CARD`, `BOX_LAYER`), window background | `misc.rs` |

(`classic` and `kde` keep shared drawing helpers in `common.rs`; API.md, section "Style
contract", lists every function with its parameters.)

**Data views and app shells (`StyleData`, `StyleShell`).** The widgets of `lib/ui/data.rs`,
`chart.rs`, `shell.rs`, `notify.rs` and `datepick.rs` reach their painters through
`Style::data() -> &'static dyn StyleData` and `Style::shell() -> &'static dyn StyleShell`. Both
default to `ui::stock::Stock`, which paints everything from the theme tokens, so every style
shows grids and navigation panes without code of its own. A style that wants its own look
returns its own implementation (and overrides only the methods it cares about):

| Trait | Content | Style file (backupcraft) |
|---|---|---|
| `StyleData` | grid metrics, header strip, header cells, rows; tree twisties; badges; status dots; meters; the chart palette | `data.rs` |
| `StyleShell` | navigation pane, sections and items; command bar; splitter grip; breadcrumb separator; segmented control; wizard steps; toasts; spinner; indeterminate progress; calendar days | `shell.rs` |

```rust
impl Style for BackupCraft {
    // ...
    fn data(&self) -> &'static dyn StyleData { &BackupCraft }
    fn shell(&self) -> &'static dyn StyleShell { &BackupCraft }
}
```

No new tokens: status colours come from `T_OK` / `T_WARN` / `T_CRIT` / `T_INFO` and their `_BG`
entries, so the stock painters fit every style's palette.

**`Style`** (lib.rs):

* `tokens(dark) -> [u32; T_COUNT]` -- the semantic `T_*` colours of `ui::consts` (so an app's own
  drawing with `ui.color(T_TEXT)` works in every style). Accent entries may be 0:
* `accent(&mut theme, dark, rgb)` fills `T_ACCENT`, `_HOVER`, `_PRESSED` and `T_ACCENT_TEXT` from
  the app's accent (`0xRRGGBB`, 0 = the style's default).
* `ts_size()`, `ts_line()`, `ts_font()` -- pixel size, line height and `FONT_*` of the text
  styles `TS_CAPTION .. TS_DISPLAY` at 100 %.
* `fonts() -> [&'static [u8]; 4]` -- the SGF1 data of `FONT_REG`, `FONT_SEMI`, `FONT_BOLD`,
  `FONT_ICON` (`include_bytes!` from the style's `assets/`), `font_names()` -- family, style,
  full name, typographic family / style of each.

**Layout values:** `StyleChrome::metric(SM_*)` gives dialog widths and paddings, bar heights,
caption button widths, title text styles, grips. Conventions: `SM_CAP_LEFT` 1 puts the caption
buttons at the left end in the order close, minimise, maximise; `SM_DLG_BTN_ORDER` right-aligns
the dialog buttons at their natural width with the default button last (1) or first (2);
`SM_TB_ICON` 0 hides the title bar icon. A few values have functions of their own
(`scrollbar_thickness`, `flyout_pad`, `menu_item_h`, `value_slider_thumb`, ...).

**Widgets:** per widget a size function and a paint function, both taking the **`Sp` record**:

```rust
fn button_size(&self, ui: &mut Ui, sp: &mut Sp) -> (i32, i32); // natural size, px
fn button(&self, ui: &mut Ui, sp: &Sp);                         // emit draw commands
```

| `Sp` field | Meaning |
|---|---|
| `x`, `y`, `w`, `h` (`rect()`) | the placed rectangle (px), filled before the paint call |
| `state` | 0 rest, 1 hover, 2 pressed, 3 disabled |
| `ev` | `EV_*` of the core's `behave` (hover, held, focus) |
| `flags`, `icon`, `label` | widget flags (`ST_*`, `MIF_*`, `RG_*`, ...), icon code point, label |
| `value`, `value2` | widget values (check state, thumb offset, colour, ...) |
| `s[0..4]` | kept by the style from its size call to its paint call; a few widgets report values back to the core here (documented per function, e.g. the slider's thumb length) |
| `label2`, `f`, `img` / `img_ver`, `track` | a second text (menu accelerator, info bar message), an angle, a thumbnail, a gradient track |

Paint functions only emit drawing commands -- `ui.draw_color`, `draw_rect`, `draw_rrect`,
`draw_frame`, `draw_text`, `draw_icon`, `draw_circle`, `draw_line`, `draw_image`,
`ui.cmd_rrect` (fill + border + bottom band in one record), `cmd_text`, `emit_text`,
`cmd_shadow`, `cmd_custom` (a `Draw` object), colours from `ui.color(T_*)`. Layout, input, focus
handling, semantic records and the dirty-region bookkeeping stay in the core, so a style cannot
break them, and `--verify-dirty` keeps working because the commands are hashed as before.
Example (classic, abridged):

```rust
impl StyleBasic for Classic {
    fn button_size(&self, ui: &mut Ui, sp: &mut Sp) -> (i32, i32) {
        button_size(ui, sp) // 23 DIP high, 8 DIP padding, at least 72 DIP; s[0], s[1] for paint
    }
    fn button(&self, ui: &mut Ui, sp: &Sp) {
        let face = pal(ui, CP_FACE);
        fill(ui, sp.rect(), face);
        frame(ui, sp.rect(), if sp.state == 2 { FR_PRESSED } else { FR_RAISED });
        let tc = ui.color(if sp.state == 3 { T_TEXT_DISABLED } else { T_TEXT });
        // ... icon and label with ui.draw_icon / ui.emit_text in tc
    }
    // ...
}
```

Still drawn by the core with the tokens (a style changes them through its token values): the
text of `ui.text`, text-box content (selection in `T_ACCENT`, caret, placeholder), list / menu
labels where a style uses the shared helpers, gradient tracks, images.

The trait methods have empty default bodies (a leftover of the port, when the widget families
arrived one by one); a finished style overrides every one of them.

## Fonts and assets

Each style embeds its own SGF1 fonts from `styles/NAME/assets/` (format and rendering rules:
`legacy/ASMLIB/docs/FONTS.md`, `legacy/ASMLIB/docs/DESIGN.md` §3):

| Style | Fonts | Icons |
|---|---|---|
| `winui` | Segoe UI regular / semibold / bold (`font_reg.bin`, `font_semi.bin`, `font_bold.bin`) | Segoe Fluent Icons (`icons.bin`) |
| `classic` | Tahoma regular / bold (`FONT_SEMI` = bold) | winui's `icons.bin` |
| `macos` | winui's faces (no copy) | winui's `icons.bin` |
| `kde` | DejaVu Sans regular / bold | winui's `icons.bin` |
| `jfoffice` | Inter regular / medium / semibold (TrueType) | own basic icons (`icons.bin`) |
| `backupcraft` | jfOffice's Inter files (licence copied to `assets/OFL-Inter.txt`) | own vector icons (`icons.rs`, made by `mkicons.py`) through `paint_icon`; the rest from jfOffice's `icons.bin` |

A style may use another style's file with `include_bytes!("../winui/assets/icons.bin")`;
`build_rs.py` rebuilds a style when its own or winui's assets change. Icon code points (`IC_*`,
`ui::keys`) are semantic: a style's icon font must map all of them.

Convert a TrueType font with
`python tools/ttf2bin.py --font F.ttf --out styles/NAME/assets/font_reg.bin` (stdlib + numpy;
every written file is re-read and compared with the source). `ttf2bin.py --all` still writes the
winui assets and `icons.inc` of the frozen `legacy/` tree; for the Rust styles name `--out`
explicitly, and add new icon constants to `lib/ui/keys.rs` by hand.

Licences: Segoe UI / Segoe Fluent Icons (winui, macos) and Tahoma (classic) are Microsoft fonts
that may be embedded in private builds only; a public style needs openly licensed fonts
(DejaVu, Selawik -- `legacy/ASMLIB/docs/FONTS.md`).

## Writing a new style

1. Copy `styles/classic/` (small, self-contained) or `styles/winui/` to
   `styles/NAME/`. Rename the unit struct; the crate root keeps `#![no_std]` and
   `#![forbid(unsafe_code)]` (the build lint checks both) and exports
   `pub static STYLE: Name = Name;`.
2. Tokens, accent ramp and type ramp in `lib.rs`; fonts in `assets/` (above). Keep the token
   order of `ui::consts` (`T_*` are byte offsets / 4 into the array).
3. Implement every trait method. Keep each size function consistent with its paint function:
   the core places the widget with the size, then paints it into the placed rectangle.
4. Add an `INFO` file: line 1 the name shown by the installers, line 2 a one-line description
   (German, like the others). `tools/package.py` reads it ([PACKAGING.md](PACKAGING.md)).
5. Build an app with it and look at the renders (light / dark, `--scale 150`):

   ```
   python build_rs.py app gallery --style NAME --out build/NAME
   build/NAME/win/gallery.exe --shot build/NAME/g.png --theme dark --scale 150
   build/NAME/win/gallery.exe --script apps/gallery/tests/basic.uis
   ```

   The gallery scripts (`apps/gallery/tests/*.uis`) should pass, except expectations that use
   coordinates or rectangles (those depend on the style's sizes). Check popups, dialogs, tool
   windows and the library title bar (`jfgui_CSD=1`) too, and run scripts with
   `--verify-dirty`.
6. `python build_rs.py check` (rules) and `python build_rs.py test --quick`.

## Checking a style against the assembly

The four styles exist in both versions, so the acceptance test is parity: every render of the
Rust build must be pixel-identical to the assembly reference built with the same style.

```
# reference (assembly, built once in the main checkout by whoever has the legacy toolchain):
python legacy/build.py app gallery --style kde --target win --out build/rwref_kde
# comparison (shot matrix light / dark x 100 / 150 % with dumps, every gallery script):
python tools/parity.py app gallery --style kde [--target win|linux|both]
```

`parity.py --style S` takes the reference from `build/rwref_S` (`$jfgui_REF` + `_S` when that
is set, e.g. from a worktree) and builds the Rust side into `build/rs_S`. Without the `--style`
option it compares each app's own style against `build/rwref`. The kde style has extra state
scripts (hover, pressed, focus, menus, windows, CSD title bar, custom accent) with their own
runner: `python styles/kde/states/states.py gallery --linux`.

`tools/style_golden.py` is the assembly-era tool that recorded SHA-1 hashes of 185 renders per
target for the style-layer refactor; it drives `legacy/build.py` and is kept for the reference
only. For the Rust styles, `parity.py` is the check.

A new style has no assembly counterpart: its check is the gallery scripts, the renders and
`--verify-dirty`.
