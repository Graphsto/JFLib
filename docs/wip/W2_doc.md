# Real OS windows for secondary UI windows (agent W2, Strang L steps 5-7) -- fragment for DESIGN.md §7, API.md, GUIDE.md

Status: tool windows and dialogs made with `ui_window_begin` are real OS windows on
**Windows** (owned Win32 windows), **X11** (transient X windows) and **Wayland** (xdg
toplevels with `set_parent`). Headless runs (`--script`, `--shot`, `--dump`) are unchanged:
the UI's own window manager (agent W, §7) still draws every window into the main
framebuffer there, so all scripts and screenshots stay byte-identical.

## 1. Platform API (`plat_win_*`, window-indexed)

Every platform window has an index `0..PW_MAX-1` (`PW_MAX = 8`); index 0 is the main window.
All functions exist on Windows (`plat/win/win_window.s`) and Linux (`plat/linux/disp.s`
dispatches to `wl_plat_win_*` / `x11_plat_win_*` through its backend table).

```
plat_win_create(title, w_dip, h_dip, flags, owner, x, y) -> eax index / -1
        owner PW_NOOWNER (-1): a top-level window (the main window, index 0);
        else the index of the owning window.  Created HIDDEN.  x, y: client origin in
        screen px, PW_DEFAULT (0x80000000) = system / owner-relative default.
plat_win_destroy(w)                   close the window (w = 0: ends the message loop)
plat_win_show(w, mode)                PWS_HIDE 0 / PWS_SHOW 1 (activates) / PWS_SHOWNA 2
plat_win_activate(w)                  keyboard focus + top of the stacking order
plat_win_enable(w, on)                disabled windows take no input (modal dialogs)
plat_win_set_rect(w, x, y, cw, ch)    client origin (screen px) + client size (px);
                                      PW_KEEP (0x80000000) leaves a field unchanged;
                                      not reported back as E_WINPOS / E_RESIZE
plat_win_get_rect(w, &i32[4]) -> eax  1: x, y, w, h filled; 0: position unknown
                                      (Wayland always; X11 before the WM placed it)
plat_win_work_area(w, &i32[4]) -> eax 1: work area of the window's monitor (px)
plat_win_set_title(w, utf8)
plat_win_state(w) -> eax              WST_* (WST_ACTIVE = one of the APP's windows is
                                      active, so the owner keeps an active title bar)
plat_win_cmd(w, WCMD_*)               minimize / maximize / restore / close
plat_win_begin_move(w)                system move (call on a button press)
plat_win_begin_resize(w, edges)       system resize from an edge
plat_win_size(w) -> eax w, edx h      client px
plat_win_dpi(w) -> eax                96 = 100 %
plat_win_set_min_size(w, w_dip, h_dip)  (also before the window exists)
plat_win_present(w, surface, rects, n)  rects = i32 x,y,w,h records, 0 = whole surface
plat_win_set_cursor(w, shape)         CUR_* while the pointer is over w
plat_win_hit_hook(fn)                 fn(w, x, y) -> eax HB_* flags (UI hit boxes; used by
                                      WM_NCHITTEST / WM_MOUSEACTIVATE on Windows)
plat_win_use(w) / plat_win_current()  the window the legacy per-window calls act on
```

Flags (`ui.inc`, end block "platform windows (agent W2)"; the same bits as `WF_*`):
`PWF_TOOL 1, PWF_DIALOG 2, PWF_MODAL 4, PWF_RESIZABLE 8, PWF_NOCLOSE 16,
PWF_SYSFRAME 32` (system caption; else the library draws the frame), `PWF_NOACTIVATE 64`.

Events: every event carries its window in `EV_WIN` (u8 at offset 3, 0 = main window).
`E_WINPOS` (16) reports a moved window: x, y = client origin in screen px (for windows
with a library frame this is also the outer top-left). On Windows it is coalesced during a
modal move loop (one event at `WM_EXITSIZEMOVE`).

Legacy single-window API (`plat/pwin0.s`, shared by all OSes): `plat_window_create`
(creates index 0 from AF_* flags and shows it), `plat_set_min_size`, `plat_window_size`,
`plat_dpi`, `plat_set_title`, `plat_present` act on window 0; `plat_set_cursor`,
`plat_window_state`, `plat_window_cmd`, `plat_begin_move`, `plat_begin_resize` act on the
window set by `plat_win_use` (the UI sets it whenever it makes a window live, so chrome
code such as the title bar drag works unchanged in secondary windows).

## 2. UI glue (`ui_win.s`, `ui_main.s`, `ui_chrome.s`)

* `ui_main` registers `uiw_plat_hooks` with `ui_win_hooks_set` after the main window
  exists and `uiw_hit_pw` with `plat_win_hit_hook`. `uiw_os()` = not headless and hooks set.
* UI window index <-> platform index: `g_uiw_u2p` / `g_uiw_p2u` (0xFF = none).
  `ui_process_events` translates `EV_WIN` from platform to UI index (events of unknown
  windows are dropped) before logging / dispatch.
* Hooks: `uwp_create` (hidden OS window as large as the window framebuffer; tool windows get
  PWF_TOOL, `WF_*` bits are passed through), `uwp_show` (tool and WF_NOACTIVATE windows:
  PWS_SHOWNA), `uwp_rect`, `uwp_title`, `uwp_activate`, `uwp_enable`, `uwp_present`,
  `uwp_destroy`. `uiw_sync` runs them after the settled pass; `uiw_sync_show` after
  `ui_render`, so a window is shown only with its first frame presented.
* Placement: a secondary window's WINRECT stays "DIP relative to the owner's client origin"
  (as headless). `uwp_rect` converts it with the owner's screen position
  (`uiw_owner_origin` -> `plat_win_get_rect`). While that position is unknown (X11 before
  the first ConfigureNotify, Wayland always) the OS default is kept and the window is placed
  as soon as the owner's position arrives (`g_uiw_placed`, `uiw_follow_owner`).
  `E_WINPOS` of a secondary window is converted back into its WINRECT (so the app can
  persist it); when the owner moves, the owned windows keep their screen position (as on
  Windows / X11) and their offsets are re-read from the platform.
* Modal dialogs: the UI's inert mask is mirrored with `plat_win_enable`: windows that stop
  being inert are enabled BEFORE the modal window is hidden (phase 1 in `uiw_sync`), newly
  inert ones are disabled AFTER the modal window was shown (phase 2 in `uiw_sync_show`) --
  Windows would otherwise activate some other app's window.
* `WF_SYSFRAME` gets a system frame unless the platform reports `WST_CSD` (Wayland without
  server-side decorations): then the library chrome is drawn (`uiw_sysframe_ok`).
* `ui_chrome`: the active title bar look follows `uiw_app_active` (the whole app, not the
  window); a press on an HB_DRAG title bar calls `plat_begin_move` (OS windows); resizable
  secondary windows without a system frame get edge grips (`ui_edge_grips`, ids per window).
* `ui_window_state` reports `UWS_MINIMIZED` from the platform; `ui_window_place(0, ...)`
  places / maximizes the real main window; `uiw_resize` keeps `WRF_MAX` up to date.

## 3. Windows (Win32)

* One window class, registered once. Records `g_ww[PW_MAX]` (`g_hwnd` = record 0's HWND,
  same address). `GWLP_USERDATA = index + 1`, set in `WM_NCCREATE` from `lpParam` (messages
  before it use the "creating" slot). The window procedure saves / restores the current
  event window, because it re-enters (SendMessage, SetWindowPos).
* Owned windows: `WS_POPUP | WS_CAPTION | WS_SYSMENU` (+ `WS_THICKFRAME` when resizable),
  `WS_EX_TOOLWINDOW` for tools (no taskbar button, not in Alt+Tab), `WS_EX_NOACTIVATE` for
  PWF_NOACTIVATE. Without PWF_SYSFRAME the caption is removed in `WM_NCCALCSIZE` (DWM keeps
  shadow and rounded corners) and `WM_NCHITTEST` asks the UI: HB_DRAG -> HTCAPTION, caption
  buttons -> HTCLIENT, resize band only for resizable windows.
* The owner stays drawn active while one of the app's windows is active: `WM_NCACTIVATE
  (FALSE)` is answered as TRUE unless the app itself loses the activation
  (`WM_ACTIVATEAPP` repaints every window inactive / active). `WST_ACTIVE` = app active.
* Tool windows are not activated by clicks (`WM_MOUSEACTIVATE` -> `MA_NOACTIVATE`) unless
  the press hits a text box (HB_TEXT) -- the keyboard stays with the main window.
* Modal dialogs: `EnableWindow(owner, FALSE)` via the UI enable sync (see §2).
* `WM_DPICHANGED` per window: E_DPI is pushed before the suggested rect is applied.
  `WM_MOVE` -> E_WINPOS (coalesced inside the size/move loop). Each record keeps a copy of
  the SURFACE of its last present (`WM_PAINT` / PrintWindow repaint the right pixels).
* user32.def: added EnableWindow, IsWindowEnabled, SetActiveWindow, GetActiveWindow,
  IsWindowVisible, Get/SetWindowPlacement, MonitorFromRect, MonitorFromPoint, GetWindow,
  BringWindowToTop, SetFocus, GetFocus.

## 4. X11

* Records `XW[PW_MAX]` (`g_x11_win` = record 0's XID); events are routed by their window
  field (`g_x11_evwin`); presentation state (MIT-SHM segments / PutImage) per window
  (`g_xp`), secondary windows start with 256 KB segments.
* Owned windows get, before MapWindow: `WM_TRANSIENT_FOR = owner`,
  `_NET_WM_WINDOW_TYPE_UTILITY` (tools) or `_DIALOG`, `_NET_WM_STATE {SKIP_TASKBAR,
  SKIP_PAGER, MODAL}`, `WM_NORMAL_HINTS` (USPosition | PPosition | PMinSize, PMaxSize when
  not resizable), `WM_HINTS` (input = False for PWF_NOACTIVATE, window group = main),
  `_MOTIF_WM_HINTS` decorations 0 unless PWF_SYSFRAME. **Weston's XWM (WSLg) never shows
  UTILITY windows**, so under Weston tools are typed `_DIALOG`
  (`jfgui_X11TOOL=utility|dialog` overrides the detection).
* Positions: a synthetic ConfigureNotify is not trusted (Weston sends frame-relative
  coordinates first); the root position is always read with TranslateCoordinates after a
  ConfigureNotify / MapNotify and reported as E_WINPOS (unless it is the result of our own
  request). Positions of unmapped windows (-32736...) are ignored. A window mapped again
  gets its position again (hints + ConfigureWindow).
* Activation: `_NET_ACTIVE_WINDOW` (source 1); `plat_win_enable` does nothing (the UI keeps
  inert windows inert). Minimizing the main window hides its owned windows; FocusIn on it
  brings them back. Work area = the screen.
* After every MapNotify the whole window is presented again 250 ms and 1 s later
  (`x11_present_mapped`, deadlines in `x11_present_service`): on WSLg the first frames of a
  new RAIL window are sometimes lost (§6).

## 5. Wayland

* Records `WLW[PW_MAX]`; every object's udata is the window index; `pointer.enter` /
  `keyboard.enter` select the event window. Present records per window (`g_wp`).
* Owned windows: own `wl_surface` + xdg_surface + xdg_toplevel with
  `xdg_toplevel.set_parent(owner)`, CSD (library frame; server-side decoration only with
  PWF_SYSFRAME and xdg-decoration), `xdg_toplevel.move` for title bar drags,
  `xdg_dialog_v1.set_modal` for PWF_MODAL when the compositor offers `xdg_wm_dialog_v1`.
* **Positions can neither be set nor read**: `plat_win_get_rect` returns 0,
  `plat_win_set_rect` applies only the size; secondary windows are placed by the compositor
  and their WINRECT position stays at its default.
* Show = create the objects + initial commit; the first configure arrives asynchronously and
  presents wait for it (`wl_present_service` re-presents the last surface after it). Hide =
  destroy the objects (WSLg keeps a RAIL window for an unmapped toplevel; only a destroyed
  `wl_surface` removes it). Minimizing the main window through the library hides its owned
  windows; they come back when the main window is activated again (the compositor itself does
  not do this on WSLg).

## 6. WSLg measurements (Weston RAIL 1.0.x, Windows 11, spike clients `wlspike.py` / `x11spike.py`)

Wayland (globals: wl_compositor, wl_shm, wl_seat, wl_output, xdg_wm_base, zxdg_shell_v6,
wp_viewporter, wp_presentation, wl_subcompositor, wl_data_device_manager,
zxdg_output_manager_v1, pointer constraints / relative pointer, text input, weston_rdprail_shell;
**no** xdg_wm_dialog_v1, xdg-decoration, xdg-activation, fractional-scale, cursor-shape):
* Every xdg_toplevel becomes its own Windows window of class `RAIL_WINDOW`, style
  0x96070000 (WS_POPUP|WS_VISIBLE|WS_SYSMENU|min/max box), ex style 0x80100, title + " (Ubuntu-24.04)".
  With `set_parent` it is still **unowned** in Win32 terms (GW_OWNER = 0, no
  WS_EX_TOOLWINDOW) -> own taskbar button, in Alt+Tab.
* A new toplevel is **centred on its parent** (200x150 child at (1573,490) for a 400x300
  parent at (1473,415)) and becomes the foreground window; activating the parent puts the
  parent **above** the child (no owner z-order); minimizing the parent leaves the child
  visible (not iconic).
* `attach(NULL) + commit` and destroying xdg_toplevel / xdg_surface keep the RAIL window
  visible; only `wl_surface.destroy` removes it.
* Configure: first `0x0` without states, then `activated` (state 4) for the focused one.

X11 (Xwayland; WM "Weston WM"; `_NET_SUPPORTED` = MOVERESIZE, WM_STATE, FULLSCREEN,
MAXIMIZED_VERT/HORZ, ACTIVE_WINDOW, FRAME_EXTENTS -- no SKIP_TASKBAR / MODAL):
* `_NET_WM_WINDOW_TYPE_UTILITY` windows are not shown at all; DIALOG and NORMAL become
  unowned RAIL windows (own taskbar button). Weston rewrites `_NET_WM_STATE`.
* Synthetic ConfigureNotify: first frame-relative (38,59 with decorations, 32,32 without),
  then the root position; USPosition/PPosition are honoured (500,1300 requested ->
  500,1300); ConfigureWindow moves work. X root coordinates are Weston's global space, not
  Windows desktop pixels.
* The first frame of a newly mapped X11 window is sometimes never shown: in about half of
  the runs the modal dialog's RAIL window stayed black for > 6 s although the X window had
  the right pixels (GetImage); any later damage, even 20x20 px, made the whole window
  appear. With the delayed repaints (§4) 6 of 6 runs showed it; tool windows (not
  activated) were not affected.
* Input to RAIL windows carries the real pointer position (RDP): posted clicks go wrong
  while the user's pointer is over the window (the test tools wait / move the window away).
* SetWindowPos on a RAIL window from Windows desynchronises it from the X server (the test
  moves windows with an X ConfigureWindow instead).

## 7. Test tools

* `python tools/wincheck.py windows [--exe build/win/gallery.exe] [--out-dir build]`
  (Windows): drives the gallery's "Fenster" section by posting / sending messages to the
  app's own windows only (no SendInput); headless reference run for coordinates and pixels;
  checks: tool window owned + WS_EX_TOOLWINDOW, placed at main client origin + (24,48) DIP,
  size, PrintWindow content = headless render, WM_NCHITTEST, WM_MOUSEACTIVATE (slider:
  NOACTIVATE, text box: ACTIVATE), main caption stays active / goes inactive with
  WM_ACTIVATEAPP, SetWindowPos -> E_WINPOS in `--log-events`, close button hides only the
  tool window, reopened at the moved position, modal dialog owned + centred + main and tool
  window disabled + content + "Abbrechen" re-enables, WM_CLOSE -> exit 0.
* `python tools/x11check.py windows [--backend x11|wayland] [--bindir ...] [--out-dir ...]`
  (WSLg): same gallery flow under WSLg, RAIL windows found by title; X11: a Python X client
  reads WM_TRANSIENT_FOR, window type, `_NET_WM_STATE` (INFO under Weston), WM_HINTS,
  `_MOTIF_WM_HINTS`, WM_NORMAL_HINTS, root position = main client origin + (24,48), move
  round trip (ConfigureWindow -> E_WINPOS); Wayland: `jfgui_WLDEBUG` trace shows
  `set_parent(main)`; both: framebuffer content of tool and dialog (X11: when the RAIL
  capture is black, an X GetImage of the window is compared instead and an INFO line says
  so), close hides only the window. `windows` is part of `all`.

Results (2026-09-30): wincheck windows all PASS; x11check windows 0 failures on x11 (6 runs
in a row after the repaint fix) and wayland; x11check conn / dpi / present / paint pass on
x11, paint and grab also on wayland; all `apps/*/tests/*.uis` pass on win (41) and linux
(all that link); `linuxtest --test synth` / `x11synth` pass; asmcheck 0 errors in these files.

## 8. Limitations / open issues

* Per-monitor DPI of secondary windows is implemented (WM_DPICHANGED, X11 RESOURCE_MANAGER,
  Wayland per-surface scale) but untested: all test machines run at 96 dpi / scale 1.
* WSLg: secondary windows are unowned RAIL windows (taskbar buttons, no owner z-order, not
  hidden with the owner unless minimized through the library); positions of Wayland windows
  are chosen by the compositor (centred on the parent); no xdg_wm_dialog -> modality only in
  the UI (inert windows ignore input).
* X11: no SetInputFocus fallback when the WM ignores `_NET_ACTIVE_WINDOW`; `plat_win_enable`
  is a no-op; work area = whole screen (no `_NET_WORKAREA`).
* Popups / menus / tooltips still live inside their window's framebuffer (not OS popups).
* Integrator: regenerate `ASMLIB/docs/API.md` (`python tools/gendoc.py`); fold §1-§5 into
  DESIGN.md §7 (multi-window) and §1.2 (WSLg); mention `wincheck.py` / `x11check.py windows`
  in CLAUDE.md "Testing".
