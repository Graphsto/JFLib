# T4 – Formen, Linie / Kurve (SHAPE-001..020, DRAW-013/014)

Docs: `apps/PaintCraft/docs/SHAPES.md` (tools), `ASMLIB/docs/modules/DASH_PATHDATA.md` (library).

## Files
* Lib (new): `ASMLIB/src/gfx/dash.s` (dash_pattern, dash_stroke: dashes, caps flat/round/
  square/arrow/filled arrow, joins mitre/round/bevel → closed contours for the non-zero fill),
  `ASMLIB/src/gfx/pathdata.s` (PDATA builder, SVG/XAML path parser M L H V C S Q T A Z + F0/F1,
  pd_emit/pd_bounds, pd_rrect, pd_arc); T4 block at the end of `ASMLIB/inc/gfx3.inc`
  (prefixes LCAP_, DASH_, DSS_, PDA_, PDO_, PDE_ – DS_/PD_ collided with fxb_int.inc / img.inc).
* Tests lib: `apps/rstest/src/ops_dash.s` (ops pdraster, dash, pdfuzz), registration in
  `apps/rstest/src/main.s` (marked "T4", RT_NOPS 13 → 16), `tools/pathdata_check.py`,
  `tools/dash_check.py`.
* App: `apps/PaintCraft/src/t_shapes.s` (replaces the stub), `apps/PaintCraft/src/shapes_lib.s`,
  tests `tests/tools_line.uis`, `tests/tools_shapes.uis`, fixtures `tests/fixtures/shapes/`.
* `python tools/gendoc.py` run (API.md regenerated).

## Results (build/t4, both targets)
* pathdata_check: 19 raster + 13 error cases + 2000 fuzz lines + 40 mutated XAML files: ok
  (win + linux).  dash_check: 22 cases ok (win + linux).
* tools_line.uis, tools_shapes.uis: pass on Windows and Linux, also with --verify-dirty.
* All other PaintCraft scripts pass on both targets except clipboard.uis / skeleton.uis
  (clipboard work of another agent, in progress; unrelated).  rstest selftest ok.
* asmcheck (dash.s, pathdata.s, t_shapes.s, shapes_lib.s, ops_dash.s, rstest main.s):
  0 errors (10 BOUNDS warnings, not provable only).

## Unverified / deviations
History names = tool names; dash proportions (GDI+ 3 1 / 1 1 …), arrow size (L = 3w + 4),
default corner radius 20, "Gefüllte Form zeichnen" = primary colour, rotation band 24 DIP,
move handle position; geometry of several shapes drawn after the paint.net icons; custom
shapes: path geometry only.  raster.s over-covers single pixels where separate contours
overlap (arrow junctions).  The Abgerundetes Rechteck row is wider than 1280 px.

## Needed from others (nothing blocking)
* chrome.s (integrator): the tool row could scroll / wrap when wider than the window.
* raster.s owner: an exact union of overlapping contours would need per-contour coverage
  (only cosmetic).
