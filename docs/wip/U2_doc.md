# U2 – TX_WRAP, number boxes, clip-deterministic rasterizer, open folder, SSE2 k_a8_over

Fragment for DESIGN.md §7 / GUIDE.md / WIDGETS.md.  All five items are done and tested on
Windows and Linux (headless scripts, --verify-dirty, selftests, a fake D-Bus portal, one real
Explorer / portal call).

## Files

| file | change |
|---|---|
| `ASMLIB/src/gfx/raster.s` | rasterizer core rewritten (integer accumulation, absolute per-row geometry, fold of clipped cells), SSE2 `k_a8_over` |
| `ASMLIB/src/ui/ui_basic.s` | `ui_text` + `TX_WRAP` (`text_wrap`, `wrap_lines`, `wrap_break`) |
| `ASMLIB/src/ui/ui_layout.s` | `ui_avail_width(iw) -> eax px, edx fixed` |
| `ASMLIB/src/ui/ui_edit.s` | number boxes follow `*v` while unedited, no stale commit (`nb_sync`, `nb_edited`, `nb_follow`); `ui_open_folder` |
| `ASMLIB/src/ui/ui_value.s` | `vl_whole` is now a no-op (the workaround for the rasterizer is no longer needed) |
| `ASMLIB/src/plat/win/win_shell.s` (new) | `plat_open_folder` (ShellExecuteW, shell32/ole32 at run time) |
| `ASMLIB/src/plat/linux/portal.s` | additive: `plat_open_folder`, NEGOTIATE_UNIX_FD (only when requested), UNIX_FDS header field + SCM_RIGHTS send (only when requested) |
| `apps/gfxtest/src/selftest.s` (new), `main.s` | `--selftest`, `--bench`, `--open PATH` |
| `apps/gfxtest/tests/selftest.uis`, `openfolder_bus.py` (new) | rasterizer / kernel selftest; fake-portal test (Linux) |
| `apps/gallery/src/u2demo.s` (new), `main.s` (2 calls) | section "Umbruch, Zahlen, Ordner" at the end of the page; test hooks `cmd 9110` / `9111` |
| `apps/gallery/tests/wrap.uis`, `numbox.uis` (new) | regression tests (both with `--verify-dirty`) |

## DESIGN §7 bullets

UI core
* `TX_WRAP` (ui_text): lines break after the last blank that fits the width the layout gives
  the text (`ui_avail_width`: fixed `ui_next_size` width, fill share, the box width when
  stretched; `BOX_X*`-aligned vertical box: at most the box width; natural size in a horizontal
  box: at most the space left on the row), inside words longer than a line (UTF-8 character
  boundary, at least one character per line) and at `\n`.  Trailing blanks of a line are not
  measured/drawn.  Natural height = lines × line height; natural width = the unwrapped width
  where the placed width does not depend on it (stretch / fixed / fill: content dialogs grow to
  their 548 DIP maximum first, like WinUI), else the widest line.  `TX_CENTER` / `TX_RIGHT` align
  every line; with more height than lines the block is centred vertically; `TX_ELLIPSIS` + a
  fixed height: only the lines that fit, the last one cut with "…".  Record value = number of
  lines of the whole text.  At most 256 lines (the rest is dropped); no allocation (static line
  table in ui_basic.s, shared: it is only used inside one ui_text call).  When the width is not
  known yet (0) the text is not wrapped; the settle loop fixes it in the next pass.
* `ui_avail_width(iw) -> eax px, edx 1 when the width does not depend on iw` (ui_layout.s): the
  width `ui_place` will give the next widget; no side effects.
* Number boxes (`ui_number_box`, `ui_number_box_f`): the text formatted from `*v` is remembered
  (`g_nb_orig`, `g_nb_shown`).  While focused and unedited (text equal to it) a changed `*v` is
  shown at once (caret at the end; a whole-text selection stays whole); Enter / focus loss /
  another box taking the buffer store the text only when it was edited (EV_COMMIT is still
  reported, EV_ACT only when `*v` changed).  A typed text wins over outside changes.
* `ui_open_folder(path) -> eax 0 / -1`: `plat_open_folder`, except in headless runs, which print
  `ui_open_folder: PATH` and return 0 (scripts never open windows).
* `vl_whole` (ui_value.s) is a no-op now (see Rasterizer).

Platform
* `plat_open_folder(path) -> eax 0 / -1` (UTF-8, '/' or '\', relative to the cwd):
  Windows (`win_shell.s`): folder -> `ShellExecuteW(0, "open", folder)`; file ->
  `ShellExecuteW(0, "open", "explorer.exe", "/select,\"file\"")` (the file selected in its
  folder); shell32 / ole32 via LoadLibraryA / GetProcAddress, COM initialised once; missing path
  -> -1.  Linux (`portal.s`): O_PATH descriptor of the path, D-Bus with NEGOTIATE_UNIX_FD, Hello,
  AddMatch on the request's Response, `org.freedesktop.portal.OpenURI.OpenDirectory(parent, h,
  {handle_token})` with the descriptor as SCM_RIGHTS + UNIX_FDS header field (a folder opens, a
  file is shown in its folder); on an error reply or without descriptor passing
  `OpenURI.OpenURI(parent, "file:///percent%20encoded", {handle_token})`.  Waits up to 1.5 s for
  the Response (window serviced as for the file dialog): 0 -> 0, 1 / 2 -> -1; no Response yet
  (e.g. an application chooser is open) -> the connection stays open until the next call so the
  request lives on, 0.  No bus / portal / path -> -1.  Parent window "x11:XID" under X11.
  `jfgui_OPEN_DRYRUN=1` (both OSes): print the call, start nothing, return 0.
  The file dialog's messages are unchanged (fd negotiation and UNIX_FDS only when requested).

Rasterizer (replaces the `.rps_clamp` bullet)
* Clip determinism: every pixel inside the clip is bit-identical whatever the clip is (dirty
  rectangles vs full redraw, tm_raster_path chunks).  Causes of the old ±1 differences: f32
  accumulation relative to the clip origin, x at row boundaries accumulated from the band start,
  projecting clipped line parts onto the clip edge (other float sums) and the 4-wide prefix sum
  grouped from the row's first touched cell.  Now: absolute target coordinates (clamped to
  ±2^22); per row the line part is computed from the line's end only (x(y) = xa + (y−ya)·dx/dy,
  clamped to the line's x range); the accumulator is i32 in 1/65536 coverage: a row part's
  cumulative deposit through cell c is F(c) = round(A(c)·dy·65536), the last cell gets exactly
  round(dy·65536) − F(prev); cells left of the clip fold into the first clip cell (the sum
  telescopes to F), cells right of it into a spill cell that is never emitted; integer prefix
  sums are exact, so row start / grouping no longer matter.  Coverage: non-zero
  min(|s|, 65536), even-odd 65536 − |s mod 131072 − 65536|, then (v·255 + 32768) >> 16.
  Middle cells of long row parts are done 4 at a time (same float ops per lane as the scalar
  path, so group boundaries do not change values).
* Accuracy against an exact area reference (tmask_check cases): max |Δ| 0.50 (old 0.52), mean
  identical; gfxtest page shots differ from the old rasterizer in 5 / 25 pixels by 1.
* `k_a8_over` SSE2: 16 px per step in 16-bit lanes (all intermediates ≤ 65407), zero-coverage
  groups skipped, full coverage at alpha 255 stored as 255, scalar tail; bit-identical.

Automation / tests
* `gfxtest --selftest` (tests/selftest.uis, run by build.py test): 400 random paths (fills with
  both rules, strokes with every join / cap, ellipses, rounded rectangles, coordinates up to
  ±3000 px) × 24 random clips (half of them runs of 32 px dirty cells) into PARGB and A8
  targets, every pixel inside the clip equal to the unclipped rendering, outside untouched;
  `k_a8_over` for all 256³ (dst, cov, alpha) and n = 0..67 at every dst / cov alignment with
  guard bytes.  ~3 s.  (Before the fix: 226 / 400 PARGB and 183 / 400 A8 paths differed.)
* `gfxtest --bench` timings, `gfxtest --open PATH` (plat_open_folder, honours
  jfgui_OPEN_DRYRUN).
* `apps/gfxtest/tests/openfolder_bus.py` (Linux / WSL: `python3 apps/gfxtest/tests/openfolder_bus.py
  build/u2/linux/gfxtest`): fake session bus + portal; checks OpenDirectory with an O_PATH
  descriptor of a folder and of a file (via /proc/self/fd), the OpenURI fallback on an error
  reply and without descriptor passing ("file:///tmp/…/Ordner%20mit%20%C3%84"), Response 2 ->
  -1, no Response -> 0, missing path -> -1 without bus traffic, no bus -> -1.  Not run by
  build.py test (Python, Linux only).

## Tests run (build/u2)
* All `apps/*/tests/*.uis` of every app on win + linux (124 runs): pass, except PaintCraft
  `skeleton.uis` (effects menu items expected disabled — A4–A6 are enabling effects right now;
  unrelated to U2).  With `--verify-dirty` (gallery, template, paint, PaintCraft): no missed pixels
  except PaintCraft `history.uis` (29 px at 19,36..24,41, frames 191 / 199) — pre-existing: the
  same with every U2 change reverted (raster.s, ui_basic / ui_edit / ui_layout, vl_whole);
  history.uis does not run with --verify-dirty normally (owner A2).
* asmcheck on U2's files: 0 errors (warnings = unprovable bounds, mostly pre-existing).
* New: `gallery/tests/wrap.uis`, `gallery/tests/numbox.uis` (both `--verify-dirty`),
  `gfxtest/tests/selftest.uis`; `tools/tmask_check.py --target both --out build/u2`: all passed.
* Real calls: Windows – folder and file (/select, the file selected) opened once in Explorer and
  closed again via Shell.Application; WSL – the real xdg-desktop-portal 1.18 accepted the
  OpenDirectory call (no file manager installed there, no Response within 1.5 s -> 0; no
  window left).
* Proof in the real scenario: with `vl_whole` disabled, `gallery/tests/values.uis --verify-dirty`
  reports "dirty regions missed 1 pixels" with the old rasterizer and nothing with the new one.

## Measurements (min of several rounds; noisy machine with other agents)
| | old | new |
|---|---|---|
| k_a8_over (4096 px rows, alpha 200) | 320–610 Mpx/s | 3500–5200 Mpx/s (Linux 4850) |
| ras_fill circle r 480, A8 1024² | 1.6–2.8 ms | 0.57–1.05 ms |
| ras_fill circle r 480, PARGB 1024² | 1.3–2.1 ms | 1.2–1.6 ms |
| tm_raster_path 4000×3000 zig-zag lasso, 3000 pts | 26–55 ms | 14.7–16.7 ms (Linux 49 → 18 ms) |
| tmask_check lasso (2000 pts) | ≈20 ms (G1) | 8.7–10.9 ms |

## App workarounds that can go
* PaintCraft `wnd_colors.s`: the stale-text guard (`wc_guard`, `wc_blur`, `wc_text_ev` undo of
  a stale commit, `g_wc_guard*`, `g_wc_tfocus`, `g_wc_tf_cur`) for the R/G/B/A/H/S/V number
  boxes; keep something only for the hex box, which is a plain `ui_textbox` on an app buffer
  (the app can simply rewrite that buffer while it is unedited).
* PaintCraft `palette.s` error dialog: one `TS_BODY | TX_WRAP` text instead of the two-line split;
  `wnd_history.s` (Verlauf) can drop its own wrapping; `fileio.s` dialogs now wrap as written
  (COLORS.md §8.1, HISTORY.md §8 (1)).
* PaintCraft "Palettenordner öffnen…": `ui_open_folder(path)` instead of opening the Open dialog
  in that folder (COLORS.md §8.4).
* Library: `vl_whole` callers (ui_value.s, ui_wheel.s) no longer need it (now a no-op).

## Open / not done (outside U2's files)
* Popups / tooltips clipped at tool-window edges (ui_popup / platform: needs popup OS windows)
  and title text of narrow tool windows not shrinking (ui_chrome.s, W2's file: title with
  TX_ELLIPSIS + fixed close button) — not in U2's files, not changed.
* API.md not regenerated (integrator: `python tools/gendoc.py`).
