# Multi-window runtime (agent W, milestone M0 steps 1-4) -- fragment for DESIGN.md §7 and GUIDE.md

Status: library side complete and tested **headless** (no OS windows yet). Real Win32 / X11 /
Wayland windows plug in later through the hook table (§7.8).

## 7.1 Model (DESIGN §7 "Multi-window")

* One `on_frame` builds every window. `ui_window_begin(id, title, &open, flags, &rect)` /
  `ui_window_end()` bracket a window's content anywhere inside `on_frame` (also inside another
  window's content: nested). The **owner** is the window being built when `ui_window_begin` is
  called; nesting up to 4 deep; `WIN_MAX = 8` windows including the main window (index 0).
* **State swapping instead of a rewrite**: every UI module keeps its per-window `.bss` in one
  block between `<module>_wb0` and `<module>_wb1`; `ui_win.s` lists them in `uiw_blocks`.
  `uiw_use(i)` copies the live blocks into the save area of the window that was live and window
  i's save area into the live blocks (7.6 KB each way, `rep movsb`). Every widget, popup, dialog,
  scroll view and the dirty-region code therefore run unchanged in whichever window is live.
  Invariant: outside the passes and the event dispatch the main window is live.
* A pass = `ui_frame_begin` (shared state: frame counter, semantic list, repass) →
  `uiw_pass_begin` (per window: command lists, hit list, clip, layer 0, background, root box) →
  content → `uiw_pass_end` (accelerators, layout end, Tab, Esc, modal layer, hit-list swap) →
  `ui_frame_end`. The runtime does this for window 0; `ui_window_begin/end` do it for the others
  inside the same pass. Settle loop, commands and repass stay global.
* After the settled pass `uiw_settle` hides windows that were not begun, applies
  `WF_AUTOHEIGHT` heights (another batch) and performs pending platform operations. Then
  `ui_render` renders and presents every shown window (a newly shown window: full redraw).
* Apps that never call `ui_window_begin` only have window 0: the single-window fast paths make
  the runtime identical (dumps and screenshots of template / paint / colortest are byte-identical
  to the pre-change build; bench within noise, see report).

### Per-window blocks (swapped) — everything else is shared

| module | block contents | bytes |
|---|---|---|
| ui_main | `g_fb`, `g_shadow` (verify-dirty), `g_ui_dpi`, `g_present_n` | 80 |
| ui_ctx | last rect, clip + stack, hit list pointers/counts/current, key and text queues, mx/my, buttons, pressed/released, wheel, moved, press x/y, hot + flags, active, ractive, focus + visible, press used, last click, last state, modal_cur/prev, disabled depth, command layer, box_sp | 1024 |
| ui_layout | box stack, `ui_next_*`, place auto/main, `g_root_id` | 4656 |
| ui_list | scroll view / list / expander stacks | 752 |
| ui_cmd | layer buffers, lengths, capacities, current layer, draw colour | 80 |
| ui_dirty | cell hashes (current / previous), cols, rows, full flag | 48 |
| ui_popup | popup stack, flyout nesting, menu-bar mode, Alt, context target, tooltip tracking | 944 |
| ui_chrome | content dialog state, title bar state, dialog-window footer flag | 64 |

Shared: semantic records (+ `SR_WIN` u8 at offset 42, `SEMS_MAX` 8192), state slots, timers,
command queue, command states, keys held, modifiers, clock, animation flag, the text editor
(`g_ed_*`, edits only in the keyboard window), move points (`g_mpts`, tagged with
`g_mpts_win`), dirty rectangles and the dirty clip stack, theme, scratch buffers.
Secondary windows get their own hit lists (2 × 4096 records) and smaller command buffers
(256 / 64 / 128 / 16 KB for L0..L3).

## 7.2 API (ui.inc, ui_win.s)

```
ui_window_begin(id, title, &open, flags, &rect) -> eax 1: emit the content now (always pair
                                   with ui_window_end); rect may be 0 (library-owned)
ui_window_end()
ui_window_place(id, &rect)         copy a WINRECT into the window's rect (applied next frame)
ui_window_rect(id, &rect) -> eax   1 when known
ui_window_state(id) -> eax         UWS_OPEN 1 | UWS_VISIBLE 2 | UWS_ACTIVE 4 | UWS_MINIMIZED 8
ui_window_activate(id)             keyboard + top of the stacking order
ui_window_destroy(id)              hide and free (recreated when shown again)
ui_window_current() -> eax         id of the window being built (0 = main)
ui_window_modal() -> eax           id of the topmost modal window or 0
ui_hit_flags_win(win, x, y) -> eax HB_* flags of window index win (last pass, no swap)
ui_win_hooks_set(table)            platform hooks (UWH_*), see 7.8
ui_sync_hook_set(fn)               fn() -> eax 1 while background jobs run (script `sync`)
```
Flags: `WF_TOOL 1` (owned tool window, 28 DIP title bar, keys go to the owner unless a text box
in it has the focus), `WF_DIALOG 2` (32 DIP title bar, centred on the owner, Esc closes, no
accelerators, `ui_dialog_buttons` footer), `WF_MODAL 4`, `WF_RESIZABLE 8`, `WF_NOCLOSE 16`,
`WF_SYSFRAME 32` (no library title bar / border: OS caption, platform package),
`WF_NOACTIVATE 64`, `WF_AUTOHEIGHT 128` (client height = natural content height),
`WF_KEYS 256` (tool window keeps its keys).
Ids of window `id` must be unique across all windows. Window ids themselves: any u32 != 0.

`WINRECT` (20 bytes, app-owned, persist it): `WR_X 0`, `WR_Y 4` (DIP offset of the outer
top-left from the owner's client origin; main window: screen px with `WRF_ABS`), `WR_W 8`,
`WR_H 12` (client DIP without title bar; 0 = 280 × 400), `WR_FLAGS 16`: `WRF_POS 1` (x, y valid;
otherwise the default placement is computed and written back with WRF_POS), `WRF_MAX 2`,
`WRF_CHANGED 4` (set by the library when the user moved / resized), `WRF_ABS 8`. Data macro
`WINRECT x, y, w, h, flags`. Changing the record from the app re-applies it the next frame.

Built-in commands: `UIC_WINCLOSE` (arg = window id: close button, E_CLOSE of that window, Esc in
a WF_DIALOG window; return 1 keeps it open, 0 = the library clears `*open` and hides it),
`UIC_ESCAPE` (arg = window id; 0 for the main window as before), `UIC_WAKE` (E_WAKE from
`plat_wake`), `UIC_DROP` (arg = window id, queued for E_DROP).

## 7.3 Behaviour

* **Show / hide**: shown while `*open != 0` and `ui_window_begin` is called in the frame; `*open
  = 0` hides at once, not calling it during a whole settled frame hides it after the frame.
  Hidden windows keep their state and buffers; a record is only allocated when a window is
  first opened, and when all 7 records are in use the hidden window seen longest ago is freed
  (else UIWARN "too many windows", `ui_window_begin` returns 0). A window shown again starts
  without focus (so Enter hits a dialog's primary button).
* **Default placement** (every OS and headless): dialogs centred on the owner (kept centred
  while a WF_AUTOHEIGHT height settles, until the user or the app moves it); other windows at the
  owner's client origin + (24 + 32k, 48 + 32k) DIP, k = previously shown non-dialog windows of
  that owner.
* **Chrome** (`win_chrome_begin/end` in ui_chrome.s): 1 px border (`0x66757575`), title bar
  (tool 28 / dialog and plain 32 DIP; title in TS_CAPTION, disabled colour for an inactive
  dialog; close button 40 / 46 DIP with the red hover, `HB_CLOSE`; the bar is an `HB_DRAG`
  region with a K_TITLEBAR record), then the content box (fill, `BOX_V | BOX_FIXED`; dialogs:
  `PAD_24`, gap 12, LayerFillColorAlt background). Chrome ids `0xFFFF7800 | index << 4 | part`
  (0 title, 1 close, 2 content, 3 footer, 4..6 footer buttons); root box `0xFFFF7F00 | index`
  (window 0 keeps root id 1 and its old ids).
* **Dialog footer**: inside a `WF_DIALOG` window (not inside a content dialog)
  `ui_dialog_buttons(pri, sec, close)` closes the content box and draws the WinUI footer at the
  window's bottom; Enter = primary, Esc = close; any result clears the window's `*open`.
  `ui_dialog_begin` still works inside any window (its state is per window).
* **Keyboard**: the keyboard (active) window gets E_KEY/E_TEXT. A key of a WF_TOOL window whose
  focused widget (last pass) is not HB_TEXT, and that has no WF_KEYS, goes to its owner (paint.net
  style: shortcuts work while a tool window is "active"). With a modal window open, keys of any
  inert window go to the modal window. Accelerators fire only in windows without WF_DIALOG. Only
  the keyboard window edits text; a focused text box elsewhere is drawn unfocused (and commits).
  `ui_focus(id)` is resolved in whichever window emits `id` next pass (that window is activated;
  the calling window keeps its old focus); unresolved requests expire after the passes.
* **Modal**: the topmost visible WF_MODAL window takes the keyboard; every window except it and
  the windows it owns is inert (hot = 0, no clicks), but still built and rendered (live previews
  behind effect dialogs keep working). Nested modal dialogs stack.
* **Pointer**: each event carries its window; moving into another window first sends E_LEAVE to
  the previous one; events for windows that are not shown are dropped.

## 7.4 Events

`EV_MODS` is u8 at +2, `EV_WIN` u8 at +3 (window index, 0 = main); `evq_push(type | btn << 8 |
mods << 16 | win << 24, x, y, data)`. The runtime routes E_RESIZE / E_DPI / E_CLOSE / E_WINPOS /
E_FOCUS_IN / E_DROP of window i to that window (E_DPI of window 0 still queues UIC_DPI;
E_FOCUS_IN makes the window the keyboard window; E_WINPOS stores the position in the WINRECT:
main window screen px + WRF_ABS, others DIP relative to the owner). E_WAKE queues
`on_command(UIC_WAKE, 0)`. `--log-events` prints `win=`.

## 7.5 Headless / automation (GUIDE §6)

Every secondary window has its own framebuffer: (client + title bar + 2 × border) × its dpi.
Virtual desktop: the main client at (0, 0), windows at their WINRECT offsets (default rule
above), stacking order = show / activation order, modal on top. A press follows the Windows
rule: it activates the window, except tool / WF_NOACTIVATE windows, which only become the
keyboard window when the press hits an HB_TEXT widget. Script events carry the window of the
resolved selector (keys: the keyboard window).

New script commands:
```
window ID|main|any                 selector scope (default any)
expect window ID open|closed|visible|active|modal
expect window ID rect X Y W H      the WINRECT (DIP)
close window ID                    the user closes it (E_CLOSE of the window)
movewin ID X Y   resizewin ID W H  like the user (WRF_CHANGED); resizewin 0/main = size
activate ID                        ui_window_activate
scale P [window ID]                dpi of the main window (DIP size kept) or of one window
shot F.png [window ID|desktop] [X Y W H]   a window, or all shown windows composited on the
                                   virtual desktop (px at the main window's dpi); crop in DIP
sync [MS]                          wait (real time, default 10 s) until the sync hook reports no
                                   pending jobs (no hook: returns at once), then deliver
expect S rect X Y W H              a widget's rectangle in DIP (its window's dpi)
expect_warn / expect_nowarn        a UIWARN was / was not printed since the last check
```
Selectors: `#id` must be unique across windows — an id found in several windows prints
`UIWARN duplicate id N in several windows` and the topmost window wins. An ambiguous label
resolves to the topmost window (modal first, then stacking order) that has a match, then to the
topmost layer. Point selectors `X,Y` are in the scoped window (main for `any`).
Dump: after the main header one line per shown secondary window
`{"window":ID,"title":"…","flags":"tool autoheight","rect":[x,y,w,h],"size":[W,H],"scale":S,"active":0|1}`
(rect = WINRECT, size = framebuffer in DIP), records of secondary windows end with `"w":ID`
(main window records are unchanged).

Gallery page "Fenster" (`apps/gallery`): tool window "Werkzeuge" (slider, text box, combo box,
button with tooltip; values shown in the main window), modal dialog window "Bild bearbeiten"
(checkbox, footer OK / Abbrechen) that opens a nested modal dialog "Weitere Optionen". Test hooks:
`cmd 9101` duplicate id, `cmd 9102` destroy the tool window, `cmd 9103` ui_focus into it.
Test: `apps/gallery/tests/windows.uis`.

## 7.6 GUIDE snippet — a tool window

```asm
        mov     ecx, ID_TOOLS                   # window id (unique, != 0)
        LSTR    rdx, "Werkzeuge"
        lea     r8, [rip + g_tools_open]        # 1 = shown; the close button clears it
        mov     r9d, WF_TOOL | WF_AUTOHEIGHT
        lea     rax, [rip + g_tools_rect]       # WINRECT 0, 0, 240, 0  (persist it)
        mov     [rsp + 32], rax
        call    ui_window_begin
        test    eax, eax
        jz      1f
        ...content: any widgets, ids unique across all windows...
1:      call    ui_window_end                   # always
```
Call it from `on_frame` (e.g. at its end). Keys typed while a tool window is the keyboard
window reach the main window's shortcuts unless a text box in the tool window has the focus.
Dialog windows: `WF_DIALOG | WF_MODAL | WF_AUTOHEIGHT`, content, then
`ui_dialog_buttons("OK", 0, "Abbrechen")` → DLG_* (closes the window).
In scripts use `expect window ID open`, `shot f.png window ID` and `shot f.png desktop`.

## 7.7 Limits / open

* Popups (menus, combo lists, tooltips) are drawn inside their window's framebuffer and clamped
  to it (an OS window cannot draw outside itself; separate popup OS windows would be a later step).
* Headless windows have no OS shadow; the desktop composite shows windows flat.
* A window's title bar is a drag region, but headless dragging does not move it (use `movewin`).
* Real OS windows, per-window cursors, edge grips of secondary windows on Linux, `WRF_MAX`,
  minimizing and `WF_SYSFRAME` captions need the platform package.
* The record of a window is recycled when all 7 are in use (its focus / scroll state is lost).

## 7.8 Hook points for the platform package (plat_win_*)

* `ui_win_hooks_set(&table)` with `UWH_CREATE(i, flags, owner) / UWH_DESTROY(i) /
  UWH_SHOW(i, show) / UWH_RECT(i, &WINRECT) / UWH_TITLE(i, str) / UWH_ACTIVATE(i) /
  UWH_ENABLE(i, enable) (reserved) / UWH_PRESENT(i, &fb, rects, n)` (ui.inc). Called only when
  not headless, never inside a pass: `uiw_settle` (after the settled frame, only once no window
  changed any more) does create / rect / title / hide / activate; `uiw_sync_show` (after
  `ui_render`) does show + activate of new windows so they appear with content; `ui_render`
  calls UWH_PRESENT per secondary window (window 0 keeps `plat_present`).
* The platform pushes events with `win << 24`: input, E_RESIZE (client px), E_DPI, E_CLOSE,
  E_FOCUS_IN (keyboard window), E_WINPOS (outer top-left screen px; main window: client origin).
* `ui_hit_flags_win(win, x, y)` answers WM_NCHITTEST (HB_DRAG → caption, HB_CLOSE → close) and
  WM_MOUSEACTIVATE (tool windows: MA_NOACTIVATE unless HB_TEXT) without swapping state.
* Inert windows: `g_uiw_inert` bit mask / `ui_window_modal()`; the platform may disable owners
  (EnableWindow) through UWH_ENABLE later.
* `ui_edge_grips` currently only runs for the main window.
