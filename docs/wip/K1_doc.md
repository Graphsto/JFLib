# K1 – Zwischenablage-Familie + Datei-Drag-&-Drop (M4 „clip+DnD“)

Docs: `ASMLIB/docs/modules/CLIPBOARD_DND.md` (library), `apps/PaintCraft/docs/CLIPBOARD.md` (app).

## Library
* **CLIP_IMAGE** set/get/has on Windows (`win_clip.s`: "PNG" + CF_DIBV5 + CF_DIB; get "PNG" →
  "image/png" → CF_DIBV5 → CF_DIB as BMP file), Wayland (`wl_clip.s`: image/png + image/bmp),
  X11 (`x11_clip.s`: TARGETS, image/png, image/bmp, INCR as before).  New `plat_clip_has(fmt)`
  (Linux: `disp.s` entry DSP_CLIP_HAS).  Descriptor CLIPIMG (`CI_*`, jfgui.inc).
* Shared `plat/clipdrop.s`: `clip_dib_to_bmp` / `clip_bmp_fix` (all DIB headers, masks, palettes,
  top-down/bottom-up, 64-bit checked; paint.net alpha rule: 32-bit alpha all 0 → opaque), drop
  list `plat_drop_*` incl. text/uri-list parser (file://, localhost, %XX).
* **Drops**: Windows WS_EX_ACCEPTFILES + WM_DROPFILES (shell32 DragQueryFileW/DragQueryPoint/
  DragFinish loaded at the first drop); Wayland wl_data_device enter/motion/drop (accept
  text/uri-list, set_actions copy, non-blocking pipe in the wait loop, finish); X11 XDND 5 in new
  `plat/linux/x11_dnd.s`.  E_DROP → UIC_DROP; API `ui_drop_count/path/list/pos/window` (`ui/ui_drop.s`).
* `ui/ui_clipimg.s`: `ui_clip_set_image(sf)`, `ui_clip_get_image(sf)` (own object: no codecs in
  apps that do not use it).  `ui_clip_has(fmt)`.
* Headless: image store (PNG + BMP) beside ui_edit's text store; script commands `drop X Y FILE…`,
  `clip text|file|dib|clear|save|expect`.  Test aid `jfgui_TEST_DROPFILES` (Windows, real HDROP
  posted to the own main window).

## App (PaintCraft)
`clip.s` (replaces the stub): all commands 160–171 with paint.net names/history, states via
`ui_clip_has`, paste as floating pixels (`tmove_paste`, K1 block in `t_move.s`), canvas question
(Leinwand vergrößern / beibehalten / Abbrechen), In neue Ebene / In neues Bild, Auswahl kopieren /
einfügen (5 modes, also from an image), drops (open directly without image, else question Öffnen /
Ebene(n) hinzufügen / Abbrechen).  Hooks 172–174, internal 175/176, records #0x82200/#0x82201,
widgets 0x82210/11/20/21, windows WINID_DLG0 + 0x70 / 0x71.

## Tests (build/k1)
* `apps/PaintCraft/tests/clipboard.uis`, `clipboard_dib.uis`, `drop.uis`: win + linux, also
  `--verify-dirty`: pass.  `python build.py test --out build/k1`: everything passes (skeleton.uis
  adjusted: Ausschneiden/Kopieren are enabled with an image now).
* `tools/clip_check.py` headless both targets: 15 DIB variants + 4 BMP files + 10 invalid headers
  (refused, no crash) + copy round trip (PNG, V5 BMP, back through image/bmp): all pass.
* Real: `clip_check.py real-win` (real clipboard CF_DIB → Einfügen → Kopieren → "PNG"/CF_DIBV5/
  CF_DIB; WM_DROPFILES self-test + „Drag & Drop“ window) pass; `real-x11` (private selection +
  XDND by a Python X client: XdndAware 5, Status accept/copy, Finished success/copy, E_DROP) pass;
  `real-wl` (WSLg Windows↔Wayland clipboard bridge) pass.  Wayland DnD: not testable without a
  real pointer grab (code reviewed only).
* asmcheck on every touched file: 0 errors.
