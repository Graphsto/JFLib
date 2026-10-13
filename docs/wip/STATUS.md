# PaintCraft work status (snapshot 2026-09-30, usage limit reached)

Plan: C:\Users\jakob\.claude\plans\in-dem-zootycoon2kopie-ordner-wild-treehouse.md
Rule: at most 5 agents at the same time (user request).

## Done and committed (see git log)
M0 foundations, multi-window (headless + real OS windows), threads/pool, kernels (blend
modes, tmask, flood), resampling/xform/mip/gradients, UI widgets U1, runtime fonts,
codecs: infrastructure, PNG/JPEG/BMP/GIF upgrades, TIFF, TGA, DDS reader + BC decode, .pdn,
fx: adjustments/quantize/stylize + efftest; app M1 (documents, compose, view, tabs, files,
history, tools window, colours window), T1 tool framework + brush/eraser/pencil/bucket/
picker/zoom/pan (committed in the WIP snapshot together with T2).

## In progress when the snapshot was taken (files partially written, may not build)
- T2 selection (apps/PaintCraft/src/sel.s, t_select.s, t_wand.s, t_move.s, inc/pn_sel.inc,
  tests/sel_*.uis) -- T1+T2 share pn.inc/diag.s; T2's sel_basic.uis had missed dirty pixels.
- FXB blur/artistic/noise/photo/object (ASMLIB/src/fx/fx_blur.s ... fx_conv.s, fx_lochist.s)
- FXC distort/render (fx_distort.s, fx_render.s, fx_warp.s, fx_perlin.s)
- CW WebP decoders (img/webp*.s), CB BC encoders + DDS writer (img/bc_enc.s, bc7_enc.s,
  bc6h_enc.s, dds.s), CX JPEG XR decoder (img/jxr*.s)

## Paused (not started or reverted; see docs/wip/paused/README.md)
A4 layers window/dialogs, A5 image ops, A6 effects framework (no changes yet);
U2 UI/raster fixes and Z1 zlib state (patches in docs/wip/paused/).

## Next steps
1. Build + asmcheck + tests; fix/finish the in-progress packages (check each file builds).
2. Resume A4/A5/A6, U2, Z1 (max 5 agents at once).
3. Later phases: M5 (gradient tool, shapes, line/curve, text, clone, recolor), M7 settings,
   M8 ICC/print/pen/plugins, codecs P2-P5 (WebP encoders, JXR encoder, AVIF, HEIC, JXL).
Integrator follow-ups: docs/wip/_todo.md. Agent docs: docs/wip/*_doc.md.

## Update at the usage limit
- Finished since the snapshot: T2 selection (all 17 PaintCraft scripts pass on both targets),
  FXB (23 effects), FXC (13 effects).
- Stopped at the limit (work on disk and committed, not finished):
  - CW WebP decoders: VP8L + VP8 nearly done (only animations differed), perf suite pending.
  - CB BC encoders + DDS writer: writer suites passed on Linux, perf scaling check pending.
  - CX JPEG XR decoder: in progress (table learner / overlap tests).
- Resume these agents by message (they keep their context) or start fresh from this note.

## Session 2 (resumed)
- Integration check at 34d24c8: everything builds on both targets; all .uis scripts pass on
  both targets; asmcheck: 6 errors, all in CW's unfinished WebP files.
- Running (max 5): A6 effects framework, A4 layers, A5 image ops, CW WebP (fix asmcheck +
  finish), CB BC encoders/DDS writer (finish).
- Queue: CX JPEG XR, U2 UI fixes (patch), Z1 zlib state (patch), then M5 packages
  (gradient tool, shapes/line/curve + dashes, text tool, clone stamp + recolor, clipboard),
  M7 settings, M8 (ICC, print, pen, plugins, diagnostics), codecs P2-P5.
- CB finished and committed (71e62e7). CX resumed (JPEG XR decoder). Running: A4, A5, A6, CW, CX.
- CW finished and committed (aea16e3). U2 resumed (re-applies docs/wip/paused/U2.patch). Running: A4, A5, A6, CX, U2. Next: Z1, then M5.
- A5 finished (fdf677e), U2 finished (388790a). Z1 resumed. Running: A4, A6, CX, Z1 + next M5 package.
- A4 finished (93483ce), CMD_WND_RESET wired (06e4dea), M5 id ranges + stubs (d2646e0).
- Started T3 (gradient tool, t_gradient.s) and T4 (shapes + line/curve, t_shapes.s, shapes_lib.s,
  lib gfx/dash.s + pathdata.s). Running: A6, CX, Z1, T3, T4.
- Queue: text tool (fontdb), clone stamp + recolor, clipboard (clip.s + CLIP_IMAGE), M7, M8, codecs P2-P5.
- skeleton.uis / pn.inc / diag.s are committed together with A6 (shared by A4/A5/A6).
- A6 finished (426d133; all 32 PaintCraft scripts pass on both targets in a clean worktree).
- Text/clone stubs + ids (d596950). Started T5 (text tool). Running: CX, Z1, T3, T4, T5.
- Integrator helper: a clean worktree ../jfguiLib_integ_wt (detached) for isolated package tests.
- Z1 finished (4de8981). Started T6 (clone stamp + recolor). Running: CX, T3, T4, T5, T6. Next: clipboard (clip.s + CLIP_IMAGE), M7 settings, M8, codecs.
- T3 finished (78e868d). Started K1 (clipboard CLIP_IMAGE + file drag & drop, clip.s). Running: CX, T4, T5, T6, K1.
- T6 finished (3980de1). Started S1 (M7 settings dialog + persistence). Running: CX, T4, T5, K1, S1. Next: codecs (WebP encoders, JXR encoder), M8 (ICC, print, pen, plugins, diag).
- T5 finished (3c492d6). Started CE (WebP encoders VP8L + VP8). Running: CX, T4, K1, S1, CE.
- T4 finished (179133d). M5 tools complete except the clipboard (K1). Started I1 (ICC colour management: lib icc_xform + plat_monitor_icc, app colormgmt.s). Running: CX, K1, S1, CE, I1.
- S1 finished (abf7bd0, M7 done). Guard: tools/testenv.py sets PaintCraft_CONFIG_DIR for real-window tools. Running: CX, K1, CE, I1 (+1 free slot).
- Started P1 (plugin system: PNAPI, Windows DLL + own ELF loader, crash guard, sample plugins, build.py plugin). Running: CX, K1, CE, I1, P1.
- K1 finished (833758c; full build.py test green in a clean worktree). M4 complete. build.py test now creates shot folders. Started U3 (UI polish). Running: CX, CE, I1, P1, U3.
- 2026-10-01 00:46: user approved downloading ITU-T T.832 (JPEG XR, 06/2019) -> booksAndContext/specs/ (gitignored). CX was reverse-engineering WIC without the spec (no asm written); redirected to implement from the spec.
- P1 finished (7464eb1), I1 finished (next commit). Started PR1 (printing) and V1 (AVX2 + asmcheck VEX). Running: CX, CE, U3, PR1, V1.
- U3 finished (aad829e). Started PN1 (pen pressure: WM_POINTER, tablet-v2, XI2; brush pressure). Running: CX, CE, PR1, V1, PN1.
- 2026-10-01: PR1 finished (8d903e5). User stopped Q1 (spec audit) and F2 (save options dialog) -- no
  files were changed by them -- and asked for NO new agents (token budget). Remaining running:
  CX (JPEG XR decoder), CE (WebP encoders), PN1 (pen pressure); V1 (AVX2) finished, under test.
  Not started / open: F2 save configuration dialog + fileio follow-ups, spec audit, diagnostics,
  JXR encoder, AVIF/HEIC/JXL codecs (see _todo.md).
- CE finished (WebP encoders). Running only: CX (JPEG XR decoder), PN1 (pen pressure).
- PN1 finished (pen pressure). Only CX (JPEG XR decoder) still running.
- 2026-10-01: CX finished (04bebb5, JPEG XR decoder bit-exact with WIC). No agents running.
  API.md regenerated (1363 functions). Full build.py test green on both targets.
  Open (not started, user stopped new agents for the token budget): save configuration dialog
  + fileio follow-ups (F2), spec audit, diagnostics / crash report, custom cursor images,
  JPEG XR encoder, AVIF (AV1), HEIC (HEVC), JPEG XL; small items in _todo.md.
