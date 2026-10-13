# U1 – UI widgets for the paint.net clone (decimal values, colour tracks, angle, pan, colour wheel, reorderable lists, menu lists, multi-line text) — fragment for DESIGN §7 / GUIDE

All new code is in the library's UI modules; no per-window state was added outside the
existing per-window blocks (the only new per-window variables, the list drag state
`g_ldrag_*`, live in `ui_list_wb0..wb1`). New modules keep their state in state slots
(ids are unique across windows) or in shared caches keyed by widget id.

| file | content |
|---|---|
| `ui/ui_value.s` (new) | `ui_slider_f`, `ui_track_slider`, `ui_prop_slider(_f)`, `ui_angle`, `ui_pan`, `ui_part_id`, `ui_fmt_f64`, `ui_parse_f64`, `ui_decimal_sep`; internals `rf_*`, `sld_*`, `track_cb`, `grad_at`, `trk_alloc`, `vl_whole` |
| `ui/ui_wheel.s` (new) | `ui_color_wheel` (image cache per widget) |
| `ui/ui_edit.s` | `ui_number_box_f`, `TB_DECIMAL`, PgUp/PgDn in number boxes, `TB_MULTI` / `TB_WRAP`, `nb_live` |
| `ui/ui_color.s` | `ui_color_picker_ex(id, &argb, flags)` with `CP_ALPHA`, `CP_FIELDS` |
| `ui/ui_list.s` | `LF_REORDER` (drag, insertion line, autoscroll, drop event) |
| `ui/ui_popup.s` | command states in a hash table, `MI_LIST`, two-column menu rows, flat / icon-only drop-down, F4 rule, `ui_accel_order`, key names |
| `ui/ui_basic.s` | `TX_ELLIPSIS`; `ui_canvas` records its rectangle for damage |
| `ui/ui_dirty.s` | `ui_canvas_damage` |
| `ui/ui_auto.s` | key names `[ ] , . - + /`, `num0..num9 numadd numsub nummul numdiv numdec`, `focus S`, `expect S max N` |
| `inc/ui.inc` | `RANGE_F`, `TRACK`, `TK_*`, `ANG_360`, `PAN_UNIT`, `CW_HSV`, `HSV_*`, `CP_*`, `PART_BASE`, `LF_REORDER`, `TB_DECIMAL`, `TB_WRAP`, `MIK_LIST`, `MI_MAX`, `MI_LIST` |
| `apps/gallery` | sections "Werte & Farben", colour wheel/picker, "Listen", "Text", tool row; tool window "Werte"; 6 new scripts |

## 7.x Decimal values

```
RANGE_F min, max, step, decimals, flags     32-byte record: f64 min (RF_MIN 0), f64 max (8),
                                            f64 step (16; 0 = continuous), u32 decimals (24; 0..6),
                                            u32 flags (28; RG_VERT)
ui_slider_f(id, &f64, range_f) -> eax EV    EV_ACT while the value changes, EV_COMMIT on release
                                            and on key steps; keys: arrows = 1 step (step 0: 1 % of
                                            the range), PgUp / PgDn = 10 steps, Home / End = min / max
ui_number_box_f(id, &f64, range_f) -> EV    text box; shows the range's decimals with ui_decimal_sep;
                                            accepts ',' and '.' (and '-' when min < 0); Up / Down =
                                            1 step, PgUp / PgDn = 10; the typed text is taken on Enter /
                                            focus loss (EV_COMMIT; EV_ACT when *v changed)
ui_decimal_sep                              u32 data, default ',' (44); '.' for English UIs
ui_fmt_f64(buf, &v, decimals) -> eax len    "−12,50" (buf >= 32 bytes; -0 shows as 0)
ui_parse_f64(str, &out) -> eax 1 / 0        blanks, sign, digits, one ',' or '.', digits
```
Every value is clamped, snapped to `min + k·step` and rounded to the decimals (`rf_fix`).
Semantic records of f64 widgets: `value = round(v · 10^decimals)`, `max = round(max · 10^decimals)`
(scripts: `expect #id value 125` for 12.5 with one decimal). The int `ui_number_box` also got
PgUp / PgDn (10 steps).

### Property rows (paint.net effect dialogs)
```
ui_prop_slider(id, label, &i32, range, default) -> eax EV       default 0x80000000 = no reset
ui_prop_slider_f(id, label, &f64, range_f, &default) -> eax EV  &default 0 = no reset
```
Label (TS_BODY) over a row: slider (fill) + number box (88 DIP) + reset button (IC_UNDO,
tooltip "Zurücksetzen", disabled while the value is the default). Parts: slider = `id`,
number box = `ui_part_id(id, 1)`, reset = `ui_part_id(id, 2)`; the label selects the slider in
scripts (`expect "Radius" value 25`). EV_ACT while dragging **and while typing** (the text
typed so far is previewed through `nb_live` without being reformatted), EV_COMMIT on release,
Enter / focus loss, key steps and reset.

`ui_part_id(id, k) -> eax` = `PART_BASE (0x60000000) | (id << 4 | k) & 0x0FFFFFFF`: ids of the
parts of composite widgets (decimal in dumps, e.g. `#0x6001E161` for part 1 of 7702).

### Gradient tracks (Colors window H S V R G B A)
```
ui_track_slider(id, &i32, range, track) -> eax EV
TRACK flags, argb0, argb1, ...              TK_FLAGS 0, TK_N 4, TK_STOPS 8 (u32 straight ARGB,
                                            1..16 stops, evenly spread min .. max)
TK_CHECKER 1   checkerboard under the gradient (alpha)
TK_CUSTOM  2   drawn by TK_FN (+8) fn(user, &rect) with TK_USER (+16), TK_VERSION (+24)
TK_RECT    4   square ends, gradient over the whole rectangle (internal: picker alpha bar)
```
Same behaviour and keys as `ui_slider_f` (int steps), horizontal or `RG_VERT` (max at the top).
Look: 12 DIP pill with the gradient (antialiased round ends, hairline frame), the standard
knob with the colour under the thumb as its inner dot. The app may rewrite the stops every
frame (they are hashed into the command, so a changed gradient repaints); the record must stay
valid until the frame is rendered. The `RANGE` / `RANGE_F` / `TRACK` macros align to 4 bytes:
put the label on an aligned address.

### Angle dial and pan control
```
ui_angle(id, &f64_degrees, flags) -> EV     natural 64 DIP; 0 = right, counter-clockwise;
                                            -180 .. 180 (ANG_360: 0 .. 360); drag sets the angle,
                                            Shift snaps to 15°; keys: arrows 1° (Shift 15°),
                                            PgUp / PgDn 15°, Home 0; value outside wraps;
                                            semantic value = degrees · 100, max 18000 / 36000
ui_pan(id, &f64_x, &f64_y, thumb, flags)    thumb = surface or 0 (fitted into 160 DIP, else
                                            160 × 120); the point -1 .. 1 (PAN_UNIT: 0 .. 1), y down;
                                            drag sets it, arrows 1 % (Shift 10 %), Home centres;
                                            crosshair + ring; value = x · 1000, max = y · 1000
```
(image coordinates = unit value × image size, computed by the app). Both use x87
`fpatan` / `fsincos` (`deg_atan2`, `deg_sincos`).

### Colour wheel and picker
```
ui_color_wheel(id, &color, flags) -> EV     hue around (red at the right, counter-clockwise),
                                            saturation from the centre; &color = 0xAARRGGBB
                                            (alpha kept; HSV kept in a slot so greys keep the hue)
                                            or CW_HSV: HSV record {HSV_H 0..3599, HSV_S 0..1000,
                                            HSV_V 0..1000}; the value is a separate track; the
                                            wheel is drawn at that value.  Keys: Left / Right hue
                                            ±1° (Shift 10°), Up / Down saturation ±1 % (Shift 10 %).
                                            Semantic value = hue, max = saturation.  Natural 160 DIP.
ui_color_picker_ex(id, &argb, flags)        ui_color_picker + CP_ALPHA (alpha bar id|0x30000000
                                            over a checkerboard) + CP_FIELDS (number boxes
                                            R G B A = ui_part_id(id, 3..6), H S V = (7..9), hex text
                                            box (10): RRGGBB keeps the alpha, AARRGGBB sets it)
ui_color_picker(id, &argb)                  unchanged (= _ex with flags 0)
```
The wheel image is cached per widget (4 entries, LRU): a premultiplied image at full value
for the current size (antialiased rim) and a copy scaled to the current value; only a change of
size or value re-renders it; it is drawn as an image command (version in SF_AUX).
`ui_color_picker` is a separate name from `_ex` because existing callers (paint, colortest)
do not pass flags. The picker's semantic record now covers the whole picker.

## 7.x Lists: reordering
`ui_list_begin(id, &sel, count, item_h | LF_REORDER)` (flags above bit 16 of the row height).
Dragging a row more than 4 DIP starts moving it: the list takes the pointer, the row looks
pressed, a 2 DIP accent insertion line follows the pointer (slot = nearest row boundary),
within 24 DIP of the view's top / bottom edge (or beyond) the view scrolls (2 + depth/4 px per
16 ms, `g_animating` keeps frames coming), Esc cancels. On release `ui_list_end` returns
`EV_DRAG` (| EV_ACT when *sel changed) with **edx = from | to << 16**: the app moves item
`from` so that it ends at index `to`; `*sel = to` (the moved item stays selected). A drop on
its own place reports nothing. Widgets inside custom rows (check boxes, buttons) keep their
clicks and never start a drag (layers window: thumbnail, name, visibility check box).

## 7.x Menus and commands
* `ui_cmd_state`: hash table of 2048 entries, up to 1536 commands (PaintCraft ~320); more print
  one `UIWARN ui_cmd_state: more than 1536 command states (id N ignored)`. `ui_cmd_flags` is
  O(1).
* `MI_LIST base_cmd, strlist, max, "placeholder"` (record: kind MIK_LIST 6, u16 max at
  MI_MAX 2, base at MI_ARG, placeholder at MI_LABEL, strlist pointer at MI_MENU): one row per
  string of the STRLIST buffer (the app rewrites it at any time), at most `max`; row k has the
  widget id `base_cmd + k` and runs `on_command(base_cmd + k, 0)`; an empty list shows the
  placeholder (default "(leer)") disabled. '&' in the strings marks a mnemonic ("&&" = '&').
  Arrow keys reach the rows; they have no mnemonics.
* Checked rows with an icon (WinUI): a flyout that has such a row gets a check column and an
  icon column (`PPF_TWOCOL`, decided in the previous pass); plain checked rows put the check in
  the check column; labels stay aligned.
* Accelerator text: "Strg+Umschalt+Alt+X" (unchanged: Ctrl, Shift, Alt, as requested);
  `ui_accel_order` (u32 data) = 1 gives "Strg+Alt+Umschalt+X". New key names in accelerator
  texts: "[", "]", "Num *", "Num /", "Num ,", "Num 0".."Num 9".
* F4: drop-down buttons, split buttons and combo boxes open on F4 only when the command table
  has **no** F4 accelerator (paint.net's F4 = layer properties); Alt+Down always opens.
* Flat drop-down: `ui_next_flags(ST_SUBTLE)` before `ui_dropdown_button` = tool bar look
  (transparent, subtle hover / pressed, hover shade while open); `ST_ICON_ONLY` shows the icon
  and the chevron only, the label becomes the tooltip (paint.net tool chooser).

## 7.x Text
* `TX_ELLIPSIS` for `ui_text`: a text wider than its placed width (fixed / fill size) is cut at
  a character boundary (trailing blanks dropped) and ends with "…"; the semantic record keeps
  the whole text (selectors match it), value 1 when cut.
* `TB_MULTI` text boxes: natural height 5 lines (+ 2 × 6 DIP), Enter inserts '\n', Ctrl+Enter
  commits (EV_COMMIT, focus kept), Esc drops the focus (commit); Up / Down (and Shift) keep the
  preferred x over short lines, PgUp / PgDn move a view, Home / End = the visual line, Ctrl+Home
  / End = the text; mouse click / drag / double click in 2-D; selection over lines (highlight
  per line, a selected line break shows 4 DIP); paste keeps the lines (CR LF → LF); the view
  scrolls to the caret when it moves or the text changes, the wheel scrolls it (3 lines per
  notch; at the end the wheel goes to the page); a 2 DIP bar shows the position.
  `TB_WRAP`: word wrap at the width (after the last blank that fits, long words by character;
  a caret on a soft break shows at the next line's start), no horizontal scrolling; without
  it the box scrolls horizontally to the caret. Up to 4096 visual lines; TB_PASSWORD is ignored.
  The semantic label is the text with '\n' (`expect #id text "a\nb"`).
* `TB_DECIMAL` (with TB_DIGITS): also ',' and '.'.

## 7.x Explicit damage
`ui_canvas_damage(id, x, y, w, h)` (h = stack argument 5; px relative to the canvas like
UIINPUT): the part of canvas `id` is redrawn with the next render even when its draw command
(version) did not change — marching ants that only repaint their outline. The canvas must have
been emitted before (`ui_canvas` records its last rectangle and window, 16 canvases); the
rectangle is cut to the canvas; `ui_invalidate()` is called. `dirty_compute` appends the damage
of the live window to the dirty rectangles (full redraw when they do not fit) and consumes it.
Pending damage: 32 rectangles over all windows (more merge into the last one of the window).

## 7.x Automation (GUIDE §6)
* Key names: `[ ] , . - + /` (US layout keys VK_OEM_4/6/COMMA/PERIOD/MINUS/PLUS/2),
  `num0`..`num9`, `numadd numsub nummul numdiv numdec`; e.g. `key ctrl++`, `key ctrl+numadd`.
* `focus S` — `ui_focus(id of S)` (focus ring shown) and a frame.
* `expect S max N` — the record's max (f64 widgets: max · 10^decimals; pan: y · 1000; colour
  wheel: saturation).

## Implementation notes
* **Whole-widget repaint** (`vl_whole(key)`): the dial, pan, wheel and the new sliders emit an
  invisible rectangle (alpha 0, never drawn) over the widget + 4 DIP whose colour bits hash the
  widget's state. When anything in the widget changes, every cell under it is redrawn, so its
  antialiased shapes are never rasterized in parts: a clipped rasterization of a circle can
  differ from a full one by 1 in single pixels (seen with `--verify-dirty` when a moving knob
  dirtied only one cell of the dial). The rasterizer issue itself is in gfx (see open issues).
* Focus rings of round widgets are round frames (`ui_draw_frame` with radius = half the size)
  like every other focus visual.
* Shared (not per window): command states, `g_dmg` / `g_cvs` (damage), `g_wc` (wheel cache),
  `g_trk_pool` (64 internal TRACK records, a ring), `g_ml_*` (multi-line layout of the box
  being built), `ui_decimal_sep`, `ui_accel_order`.

## GUIDE snippets

```asm
# a paint.net property row with an f64 value (effect dialog); live preview on EV_ACT
        mov     ecx, ID_RADIUS
        LSTR    rdx, "Radius"
        lea     r8, [rip + g_radius]            # .double 2.0
        lea     r9, [rip + rf_radius]           # RANGE_F 0, 200, 0.1, 1
        lea     rax, [rip + c_radius_def]       # .double 2.0 (0 = no reset button)
        mov     [rsp + 32], rax
        call    ui_prop_slider_f
        test    eax, EV_ACT
        jz      1f
        ...re-render the preview...
1:
# a reorderable layers list
        mov     ecx, ID_LAYERS
        lea     rdx, [rip + g_layer_sel]
        mov     r8d, [rip + g_nlayers]
        mov     r9d, 48 | LF_REORDER
        call    ui_list_begin                   # rows first .. end-1 with ui_list_item_begin
        ...
        call    ui_list_end
        test    eax, EV_DRAG
        jz      2f
        movzx   ecx, dx                         # from
        shr     edx, 16                         # to
        ...move the layer...
2:
# recent files in a menu:  MI_LIST CMD_RECENT0, g_recent_strlist, 10, "Keine Dateien"
```

## Tests (apps/gallery/tests, all pass on Windows and Linux, also with --verify-dirty)
`values.uis` (slider_f + number_box_f, typing with ',' '.', steps, property rows incl. reset
and live typing, angle drag/Shift/keys/wrap, pan drag/keys, 7 colour tracks + vertical track,
a property row and a dial in the tool window "Werte"), `colorwheel.uis` (wheel drag/keys, value
track, picker fields R/G/B/A/H/S/V/hex, alpha bar, hue bar), `reorder.uis` (`#!args
--verify-dirty`: click vs drag, check box in a row, drop positions, no-op drop, Esc, autoscroll,
selection follows; `ui_canvas_damage` marching ants — without the damage the same script reports
300 missed pixels), `menulist.uis` (MI_LIST rows, run-time changes, keyboard, placeholder,
accelerator text order, two-column rows, F4 vs combo box, key names + - [ ] numpad, `focus`,
flat icon-only drop-down, 1000 command states + overflow warning, decimal separator),
`ellipsis.uis`, `multiline.uis`. Existing gallery / template / paint scripts unchanged and
passing.

## Open issues
* gfx rasterizer: a shape rendered under a clip that cuts through it can differ by 1 in single
  pixels from the unclipped rendering (float accumulation relative to the clip origin) —
  worked around in the new widgets (`vl_whole`); older widgets could hit it in rare layouts.
* Keys of a WF_TOOL window go to its owner unless a text box is focused (multi-window design):
  sliders / dial / wheel in tool windows work with the mouse only (or use WF_KEYS).
* DESIGN §5.3 still lists `ui_color_picker(id,&argb,flags)`: the flags variant is
  `ui_color_picker_ex`.
* MI_LIST labels treat '&' as the mnemonic marker (file names with '&' need "&&").
* TB_MULTI: no draggable scroll bar (wheel / caret only), undo records texts < 2 KB (as
  before), no rich IME composition across lines.
* PaintCraft can now drop its `pn_cmd_set` / 10 hidden recent rows / "focus on the canvas for
  F4" workarounds (A0_doc §7 items 1, 2, 4, 5).
