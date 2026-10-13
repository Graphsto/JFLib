# PN1 – Stiftdruck / Tablett (Plan M8, DRAW-020) – Abschlussdoku

Bibliothek: Pen-Daten in EV_DATA von E_MOVE/E_DOWN/E_UP (Druck 1..65535, PTR_MOUSE/PEN/ERASER/TOUCH,
Neigung), UIINPUT um UI_PRESSURE/UI_PTYPE/UI_PPRESS/UI_TILT_X/Y erweitert (104 Byte), ui_pen(),
evq_pen_data, plat_pen_enable/plat_pen_enabled, plat_pen_devices.  Plattformen: win_pen.s
(WM_POINTER*, GetPointerPenInfoHistory), wl_tablet.s (tablet-unstable-v2), x11_xi.s (XInput 2,
Slave-Geräte mit „Abs Pressure“).  Skripte: `drag … pen|eraser P0 P1`, `pointer …`.
PaintCraft: Druck → Pinselbreite (Farbpinsel, Radierer, Klonen, Neu färben; Radius entlang des
Segments interpoliert), Werkzeugleiste „Druckempfindlichkeit“ (am Zeilenende, nur bei bekanntem
Stift), Radierer-Ende wählt temporär den Radierer, Einstellungen ▸ „Stift & Tablett“
(„Windows-Tinte“, erkannte Geräte, `[UI] peninput`).  Details: apps/PaintCraft/docs/TOOLS.md §10,
docs/SETTINGS.md.  Tests: apps/PaintCraft/tests/tools_pressure.uis, plattest --test pen
(apps/plattest/tests/pen.uis), tools/pencheck.py (Proxy in WSLg).

---

# Pen input (pressure, pointer type, tilt) — agent PN1, plan M8, spec DRAW-020

Pens and pen tablets report pressure, the end in use (tip / eraser) and tilt through the normal
pointer events.  A mouse produces exactly the events it produced before (data 0).

## 1. Events

`E_MOVE`, `E_DOWN`, `E_UP` carry the pen data in `EV_DATA` (jfgui.inc):

| bits | field | |
|---|---|---|
| 0..15 | `PD_PRESSURE` | 1..`PRESSURE_ONE` (65535); 0 = no pressure (mouse, touch, pen without pressure axis) |
| 16..17 | type `PTR_MOUSE` 0, `PTR_PEN` 1, `PTR_ERASER` 2, `PTR_TOUCH` 3 | |
| 18..24 | tilt x, i7 degrees (−63..63, + = right) | |
| 25..31 | tilt y, i7 degrees (+ = towards the user) | |

`evq_pen_data(pressure, type, tilt_x, tilt_y) -> eax` builds it (pressure −1 = no axis → 0,
else clamped to 1..PRESSURE_ONE, so a touching pen never reads as "no pressure"; tilts clamped;
`PTR_MOUSE` → 0).  `plat_pen_enable(on)` / `plat_pen_enabled()`: with pen input off,
`evq_pen_data` returns 0 and Windows leaves pens to the system's mouse emulation.
`plat_pen_devices() -> eax` counts pens / tablets (−1 = unknown: no API / no backend).

The UI core (ui_ctx.s) keeps the data of the last pointer event (`ui_pen() -> eax pressure,
edx PTR_*`) and the pressure of every move point.  `E_UP` keeps the last pressure (a lift
reports 0 on most devices).

## 2. UIINPUT (ui_canvas)

| field | |
|---|---|
| `UI_PRESSURE` (+80) | pressure now: 0 = none, 1..`UI_PRESSURE_ONE` (65535) |
| `UI_PTYPE` (+84) | `PTR_*` of the last pointer event |
| `UI_PPRESS` (+88) | ptr to u32 pressure per move point (`UI_NPTS` entries, parallel to `UI_PTS`) |
| `UI_TILT_X` / `UI_TILT_Y` (+96 / +100) | i32 degrees |

`UIINPUT_SIZE` = 104.

## 3. Platforms

**Windows** (win_pen.s): `WM_POINTERUPDATE/DOWN/UP/LEAVE/CAPTURECHANGED`; `GetPointerType`,
`GetPointerPenInfo`, `GetPointerPenInfoHistory`, `GetPointerDevices` via GetProcAddress (user32,
Windows 8+; missing → mouse messages as before).  Only `PT_PEN` pointers are handled;
`EnableMouseInPointer` is never called, touch and unknown pointers go to DefWindowProc (mouse
emulation).  Update: every coalesced sample (history, oldest first, at most 16) → `E_MOVE`.
Down: `BTN_LEFT`, `BTN_RIGHT` when the barrel button is held (`PEN_FLAG_BARREL` /
`POINTER_FLAG_SECONDBUTTON`, like paint.net).  `PEN_FLAG_INVERTED | PEN_FLAG_ERASER` →
`PTR_ERASER`.  Pressure 0..1024 → 0..65535.  Positions: `ptPixelLocation` → `ScreenToClient`.
Counts from the API are clamped.  A handled pen produces no mouse messages.

**Wayland** (wl_tablet.s): `zwp_tablet_manager_v2` (tablet-unstable-v2, version 1) when the
compositor offers it → `get_tablet_seat(seat)`; tools collect `motion / pressure / tilt / down /
up / button / proximity_in|out` until `frame`, then push `E_MOVE`, `E_DOWN` (`BTN_STYLUS` held →
right, `BTN_STYLUS2` → middle), `E_UP`, `E_LEAVE` in that order.  Tool type eraser → `PTR_ERASER`.
Over a popup surface (menus) the events are the owner window's, moved by the popup's offset
(`wl_popup_ptr`, like the pointer).  Tablets are counted (`tablet_added` − `removed`); pads are
ignored.  With wp_cursor_shape_v1 the
tool shows the window's cursor (`get_tablet_tool_v2`).  WSLg's Weston has no tablet protocol.

**X11** (x11_xi.s): XInput 2 (`QueryExtension "XInputExtension"`, `XIQueryVersion 2.0`,
`XIQueryDevice`).  Pens = slave pointers with an `"Abs Pressure"` valuator (atom interned
only-if-exists); `"eraser"` in the device name → `PTR_ERASER`; `"Abs Tilt X/Y"` taken as degrees.
`XISelectEvents` Motion / ButtonPress / ButtonRelease for those slave devices only, on every
window — the master's core events keep coming and stay the event source; the slave event (same
server time, delivered first) only adds its pen data (`x11_xi_data(time)`).  `XI_HierarchyChanged`
on the root window re-reads the devices (hotplug).  Stylus buttons arrive as core buttons 2 / 3.
Every reply / event length is checked.

## 4. Scripts (headless)

* `drag S1 S2 [steps] … pen P0 P1` / `… eraser P0 P1` — a pen stroke, pressure P0 → P1 in %
  (linear per step).
* `pointer mouse | pen [P [TX TY]] | eraser [P [TX TY]] | touch` — pen data of all following
  pointer events (`move`, `down`, `up`, `click`, `drag`); `pen` without P = no pressure axis.
  The data goes through `evq_pen_data`, so `plat_pen_enable(0)` applies to scripts too.

## 5. Tests

* `apps/PaintCraft/tests/tools_pressure.uis` (both targets, also `--verify-dirty`): width scales
  with pressure, the toolbar switch, eraser end ↔ Radierer, Windows-Tinte off.
* `plattest --test pen` (`apps/plattest/tests/pen.uis`, runs in `build.py test`): Windows —
  `SendMessageW` of WM_POINTER* to an own hidden window with a fake pointer API
  (`g_win_pen_api`: state, GetPointerType, GetPointerPenInfo, GetPointerPenInfoHistory,
  GetPointerDevices); Linux — the Wayland tablet handlers and XInput 2 parsing / events with
  synthetic data.
* `python tools/pencheck.py wayland|x11|all` (real windows in WSLg, about 10 s each): a protocol
  proxy plays a tablet between the app and WSLg's server (Wayland: a fake tablet-manager global;
  X11: fake devices in the XIQueryDevice reply, XISelectEvents redirected to real slave
  pointers, XI2 + core events injected); the app's `--log-events` output must show the pen data.
  Nothing is injected into the system.

## 6. Manual checklist (real tablets — not available here)

Windows (Wacom / Surface pen / any Windows-Ink pen): PaintCraft, Farbpinsel width 40, aliased:
1. Light → hard stroke: width grows; hover shows the cursor, no stroke while hovering.
2. Barrel button + tip: secondary colour (right button), as with paint.net.
3. Flip the pen (eraser end) over the canvas: tool row shows Radierer; erase; flip back: the
   previous tool returns.
4. Einstellungen ▸ Stift & Tablett: the pen is counted; "Windows-Tinte" off → full width
   (mouse emulation, press-and-hold right click may appear); on again → pressure.
5. Touch still works as a mouse; a mouse has full width.
6. Fast strokes: no gaps (history samples), `--log-events` shows several E_MOVE per frame.

Wayland (GNOME / KDE / sway with a tablet): `jfgui_BACKEND=wayland`, same steps 1–3 (barrel =
BTN_STYLUS → right); `jfgui_WLDEBUG=1` shows `zwp_tablet_seat_v2.tool_added`; the cursor over
the window follows the window's shape (cursor-shape-v1) or the compositor's default.
X11 (Xorg + wacom / libinput driver, or Xwayland): `jfgui_BACKEND=x11 jfgui_X11DEBUG=1` prints
`x11: XInput 2 op N, K pen device(s)` (K ≥ 1 with a tablet; the eraser is its own device);
steps 1–3 (barrel = the driver's button 2/3); plug the tablet in while the app runs: K updates
(Einstellungen page), strokes work without a restart.
