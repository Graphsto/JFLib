# G2: resampling, affine transforms, display mips, gradients — documentation fragment

For `ASMLIB/docs/DESIGN.md` §2 / §7 ("As built") and `ASMLIB/docs/API.md`. New library files:
`ASMLIB/src/img/resample.s`, `ASMLIB/src/img/xform.s`, `ASMLIB/src/img/mip.s`,
`ASMLIB/src/gfx/gradient.s`. Public constants and records (`RS_*`, `XF_*`, `XM_*`, `XA_*`, `MC_*`,
`MIP_MAX`, `GT_*`, `GR_*`, `GF_*`, `GD_*`) live in the new include file `ASMLIB/inc/gfx3.inc`
(guarded with `.ifndef GFX3_INC`, `=` definitions only). Until `jfgui.inc` includes it, sources
write `.include "gfx3.inc"` after `jfgui.inc` (all four modules and apps/rstest do). No build.py
change is needed (the files live in existing library directories). Test harness: `apps/rstest`
+ `tools/resample_check.py`, `tools/xform_check.py`, `tools/gradient_check.py`.

Dependencies: `mem_alloc` / `mem_free` / `mem_copy` / `mem_zero` (core), `surf_alloc` / `surf_free`
(mip chains), `k_premul_row` / `k_unpremul_row` (gfx/straight.s, G1) for format changes on the
copy / nearest paths. Nothing else; no global state anywhere.

---------------------------------------------------------------------------------------------------
## Threading contract (all four modules)

* No mutable global state: the lookup tables (sRGB LUTs, Catmull-Rom phase table, Bayer matrix,
  f64 constants) are `.rodata`; scratch memory comes from `mem_alloc` per call or from the stack
  (every frame < 4 KB). Every function may run on any thread.
* Heavy work has a row-range entry point for `thr_parallel_for`-style splitting:
  `rs_resize_rows(ctx, y0, y1)`, `rs_affine_rows(xa, y0, y1)`, `mip_down_rows(src, dst, x0, x1,
  dy0, dy1)`; gradients are per span (`grad_span`). Disjoint destination row ranges may run
  concurrently; the context / XFORM record is read-only while they run; results are identical
  for any split (tested: 1 / 3 / 4 / 7 / 8 bands).
* `rs_prepare` / `rs_free` and `rs_affine_prepare` run on one thread (the owner).
* Deterministic across OSes: integer arithmetic plus IEEE f64 / f32 operations in a fixed order
  (no FMA, SSE2 only, MXCSR default rounding), verified bit-identical on Windows and Linux.

---------------------------------------------------------------------------------------------------
## Resize — img/resample.s

```
rs_resize(src, dst, filter, flags) -> eax 0 / ERR_*         whole image (prepare + rows + free)
rs_prepare(src, dst, filter, flags, &ctx) -> eax 0 / ERR_*  weight tables, *ctx
rs_resize_rows(ctx, y0, y1) -> eax 0 / ERR_NOMEM            dst rows y0 .. y1-1 (clamped)
rs_free(ctx)                                                 (0 ignored)
rs_axis_weights(S, D, filter, info) -> eax 0 / ERR_*        one axis' table (tests / reuse)
rs_to_work(W, src, n, fmt, flags)                            8-bit row -> working row
rs_from_work(dst, W, n, fmt, flags)                          working row -> 8-bit row
```
* `src`, `dst`: SURFACE, `FMT_ARGB` (straight) or `FMT_PARGB`, any combination
  (`ERR_UNSUPPORTED` otherwise); sizes 1 .. 65535 (`ERR_DIMS`); the dst surface is allocated by the
  caller and its size is the target size; dst must not overlap src. All address arithmetic is
  64-bit (tested with row offsets beyond 4 GB).
* Filters (`RS_*`): NEAREST 0, BILINEAR 1, BILINEAR_LQ 2, BICUBIC 3, BICUBIC_SMOOTH 4, LANCZOS3 5,
  FANT 6, ADAPTIVE 7. Flag `RS_GAMMA` 1 = filter in linear light.

Weight tables (per axis S -> D, per output i, IEEE f64 in this order):
* scale = S / D; fs = max(scale, 1) for widened filters (BILINEAR, BICUBIC, BICUBIC_SMOOTH,
  LANCZOS3, ADAPTIVE) else 1; R = support * fs; center = ((2i + 1) S) / (2D);
  taps j = floor(center - R) .. ceil(center + R): w_j = K(((j + 0.5) - center) / fs).
* RS_FANT (exact area average): lo = iS / D, hi = (i + 1) S / D, j = floor(lo) .. ceil(hi) - 1,
  w_j = min(hi, j + 1) - max(lo, j).
* Kernels (x = |x|): triangle max(0, 1 - x) (support 1); cubic a: x < 1: ((a+2)x - (a+3)) x x + 1,
  x < 2: ((a x - 5a) x + 8a) x - 4a (support 2; Catmull-Rom a = -0.5); Mitchell-Netravali
  B = C = 1/3: x < 1: (7/6 x - 2) x x + 8/9, x < 2: ((-7/18 x + 2) x - 10/3) x + 16/9; Lanczos-3:
  sinpi(x) sinpi(x/3) / (pi x (pi x / 3)) for x < 3 (own sinpi: reduction by floor(t + 0.5), degree
  13 Taylor polynomial). RS_BILINEAR_LQ = triangle never widened (plain 2 x 2 at any scale).
  RS_ADAPTIVE per axis: Lanczos-3 (widened) when S > D, else Keys cubic a = -0.75 (sharper than
  Catmull-Rom).
* Clamp to edge: taps outside 0 .. S-1 are added to the edge pixel (no darkening at borders).
* Normalisation: q = floor(w / sum(w) * 16384 + 0.5); 16384 - sum(q) is added to the first
  largest q, so every output's weights sum to exactly 1 << 14; zero taps at both ends dropped.
  Table records: {i32 x0, i32 taps, pad, n/2 x 16-byte pmaddwd weight pairs}.
* Same size with an interpolating filter (every one except BICUBIC_SMOOTH) = exact copy; NEAREST
  copies pixel sx = ((2x + 1) S) / (2D) without premultiplication (format change via
  k_premul_row / k_unpremul_row).

Pipeline (other filters): working values = 4 x i16 premultiplied, full = 16320 (255 * 64):
* PARGB: P = v << 6; ARGB: P_a = a << 6, P_c = ((c a + 2) 16448) >> 16;
  RS_GAMMA: straight s (PARGB: min(255, (v 255 + a/2) / a)), lin = rs_lin16[s] =
  round(16320 srgb_to_linear(s / 255)), P_c = (lin a 257 + 32768) >> 16.
* Horizontal pass H = sat16((sum w P + 8192) >> 14) into a ring of source rows (not clamped
  between the passes), vertical V = sat16((sum w H + 8192) >> 14); SSE2 pmaddwd over 4 channels
  at once; rows of the ring are reused by consecutive output rows; more vertical taps than the
  ring budget (64 MB) are summed in groups through an i32 accumulator.
* Output (rs_from_work): A = max(P_a, 0), colours clamped to 0 .. A, a = (min(A, 16320) + 32) >> 6;
  the colour RATIO P_c / A is kept, so ringing that overshoots alpha never shifts colours and
  fully transparent neighbours never bleed colour (premultiplied filtering):
  ARGB c = rint(f32(P_c 255) / f32(max(A, 1))); PARGB c = (P_c + 32) >> 6, or the straight c
  (alpha 255) when A > 16320; RS_GAMMA: idx = (P_c 8190 + A) / (2A), s = rs_srgb8[idx] =
  round(255 linear_to_srgb(idx / 4095)), premultiplied c = ((s a + 128) 257) >> 16. The gamma
  LUT round trip is lossless for opaque pixels.

---------------------------------------------------------------------------------------------------
## Affine transforms — img/xform.s

```
rs_affine(src, dst, m, filter, flags, clip) -> eax 0 / ERR_*
rs_affine_prepare(xa, src, dst, m, filter, flags, clip) -> eax 0 / ERR_*   fills an XFORM record
rs_affine_rows(xa, y0, y1) -> eax 0 / ERR_NOMEM                          dst rows y0 .. y1-1
xf_identity(m)            xf_mul(out, a, b)            xf_invert(m, out) -> eax 1 / 0 singular
xf_translate(m, tx, ty)   xf_scale(m, sx, sy)          xf_rotate(m, angle, cx, cy)
xf_apply(m, xy)           xf_sincos(angle, out)
```
* Matrix m = 6 x f64 {a, b, c, d, e, f} (`XM_*`): x' = a x + b y + c, y' = d x + e y + f.
  rs_affine maps DESTINATION pixel centres (x + 0.5, y + 0.5) to continuous SOURCE coordinates
  (source pixel (i, j) covers [i, i+1) x [j, j+1)); to draw an image moved by a forward matrix F
  pass xf_invert(F). `xf_mul(out, a, b)` = a * b (b applied first); `xf_translate / xf_scale /
  xf_rotate` post-multiply (m = m * T); `xf_rotate` turns about (cx, cy); with y down a positive
  angle is clockwise on screen. f64 arguments are passed as IEEE bit patterns in the integer
  argument registers / stack slots (movq rdx, xmm0). xf_sincos: k = rint(angle 2/pi),
  Cody-Waite reduction with two constants, degree 15/16 polynomials, remainders below 1e-12
  snapped to 0 (quarter turns are exact 0 / +-1; |error| < 2e-16 otherwise).
* Filters: RS_NEAREST, RS_BILINEAR, RS_BICUBIC (Catmull-Rom 4 x 4, 256-phase table
  `xf_cubic_lut`), RS_MULTISAMPLE 8 (16 bilinear samples at offsets 1/8, 3/8, 5/8, 7/8 per axis,
  mean; paint.net's "bilinear multisample" for rotations). flags = RS_GAMMA | edge << XF_EDGE_SHIFT
  (8), edges XF_TRANSPARENT 0 (outside = 0, antialiased borders), XF_CLAMP 1, XF_WRAP 2,
  XF_REFLECT 3 (period 2n, edge pixels repeated). clip = {i32 x0, y0, x1, y1} (exclusive) or 0;
  dst pixels outside clip are untouched.
* Sampling (f64 in this order): qx = a xc + (b yc + c), qy = d xc + (e yc + f); coordinates are
  clamped to +-2^30 (NaN -> -2^30, so degenerate matrices never crash). NEAREST takes pixel
  floor(q). BILINEAR / BICUBIC use 24.8 fixed-point phases U = floor((q - 0.5) 256 + 0.5); phase 0
  in both axes copies the source pixel exactly (identity, integer translations and 90-degree
  turns are bit exact; the rotate-90 matrix built with xf_translate / xf_rotate / xf_invert
  equals img_rotate90 with NEAREST and BILINEAR); otherwise working values as in resample.s:
  T = (P00 (16384 - 64 fx) + P10 64 fx + 8192) >> 14, B likewise, P = same over T / B; bicubic:
  rows first, sat16 after each pass. Integer translations take a row-copy fast path.
* Straight or premultiplied in / out in any combination; working values and output conversion are
  rs_to_work / rs_from_work (same rounding and ratio-preserving output as resize).

---------------------------------------------------------------------------------------------------
## Display mips — img/mip.s

```
mip_down(src, dst, x0, y0, x1, y1) -> eax 0 / ERR_*        dst part covered by the source rect
mip_down_rows(src, dst, x0, x1, dy0, dy1) -> eax 0 / ERR_*   dst rows dy0 .. dy1-1 (worker split)
mip_chain_alloc(w, h, levels) -> rax chain or 0             levels 0 = down to 1 x 1 (<= 16)
mip_chain_update(chain, src, rect) -> eax 0 / ERR_*          propagate a dirty rect (0 = all)
mip_chain_level(chain, k) -> rax SURFACE*                    level k = 1 .. MC_N
mip_chain_free(chain)
```
* src FMT_ARGB or FMT_PARGB, dst FMT_PARGB of (w + 1) / 2 x (h + 1) / 2; odd edges repeat the last
  column / row. PARGB: v = (p00 + p01 + p10 + p11 + 2) >> 2; ARGB (premultiplied on the fly at
  full precision): a = (sum a + 2) >> 2, t = (sum c a + 2) >> 2, c = min(a, ((t + 128) 257) >> 16).
* Dirty rect x0, y0, x1, y1 in source pixels (exclusive, clamped); the next level's rect is
  (x0 >> 1, y0 >> 1, (x1 + 1) >> 1, (y1 + 1) >> 1). Chain record `MC_*` (gfx3.inc): u32 levels,
  base w, h, then MIP_MAX owned SURFACEs.

---------------------------------------------------------------------------------------------------
## Gradients — gfx/gradient.s

```
grad_span(dst, n, x, y, g)            n straight ARGB pixels, row y, columns x .. x+n-1
grad_alpha_span(dst_a8, n, x, y, g)   n bytes: alpha ramp A.a -> B.a (transparency-mode mask)
grad_t_span(dst_i32, n, x, y, g)      n positions T = 0 .. GT_ONE (tests / tools)
```
GRAD record (48 bytes, gfx3.inc): +0 u8 GD_TYPE (GT_LINEAR 0, GT_LINEAR_REFLECTED 1,
GT_DIAMOND 2, GT_RADIAL 3, GT_CONICAL 4, GT_SPIRAL_CW 5, GT_SPIRAL_CCW 6), +1 u8 GD_REPEAT
(GR_NONE 0, GR_WRAP 1, GR_REFLECT 2), +2 u16 GD_FLAGS (GF_ALPHA_MODE 1, GF_DITHER 2), +4 u32
GD_COLA, +8 u32 GD_COLB (straight 0xAARRGGBB at t = 0 / 1), +12 reserved, +16 f64 GD_SX, GD_SY
(start), +32 f64 GD_EX, GD_EY (end), document pixel coordinates.

* Pixel centres p = (x + i + 0.5, y + 0.5); d = end - start, L2 = d.d, v = p - start,
  dot = vx dx + vy dy, cross = vy dx - vx dy (positive = clockwise from d on screen),
  r = |v|. linear t = dot / L2; linear reflected |dot| / L2; diamond (|dot| + |cross|) / L2;
  radial r / L; conical |atan2(cross, dot)| / pi (0 along d, 1 opposite: symmetric cone);
  spiral CW frac(r / L + atan2 / 2pi), CCW frac(r / L - atan2 / 2pi). "/ L2", "/ L" are
  multiplications by 1 / L2 and 1 / sqrt(L2). atan2 = Abramowitz-Stegun 4.4.49 polynomial
  (max error 1.15e-5 rad = 0.002 / 1024 turn).
* Repeat: GR_NONE clamp to [0, 1]; GR_WRAP frac(t); GR_REFLECT triangle wave 1 - |2 frac(t/2) - 1|
  (frac after clamping to +-2^30). Degenerate L2 = 0 / not finite: colour B everywhere; unknown
  types are treated as linear.
* Colour: T = floor(t 16384 + 0.5); every channel INCLUDING alpha on straight values:
  c = (A (16384 - T) + B T + 8192) >> 14 (exact rounding); GF_DITHER replaces 8192 by the ordered
  4 x 4 Bayer threshold 1024 k + 512 at ((x + i) & 3, y & 3). GF_ALPHA_MODE in grad_span keeps
  the pixel colours and sets a' = ((a ramp + 128) 257) >> 16 with ramp = lerp(A.a, B.a) (paint.net
  transparency mode); grad_alpha_span writes the ramp itself.

---------------------------------------------------------------------------------------------------
## Tests and measurements

`apps/rstest` (headless, runs from on_init): `rstest --shot x.png --op NAME ARGS...` or
`--ops FILE` (one op per line) or `--selftest` (`apps/rstest/tests/selftest.uis` runs it in
`build.py test`). Ops: resize, weights, mip, mipchain, mipinc, affine, xfrot, rot90, xfmat,
sincos, cubiclut, grad, stride. Images: `.raw` ("RSR1" w h fmt + packed rows) or any image file
(img_load_ex straight / img_save_opt). Every surface is guarded (padding bytes 0xCD checked after
the op, last row ends at a page end); `stride N` forces huge strides (row offsets > 4 GB).

The Python checkers replicate the assembly bit for bit (same f64 operation order, same integer
pipeline) and require |d| <= 1 (<= 2 with RS_GAMMA); measured max |d| = 0 everywhere, both targets:
* `python tools/resample_check.py [--target win|linux|both] [--bench]` — 1244 cases: weight tables
  bit-exact (11 S/D pairs x 7 filters, up to 65535 -> 3 and 2 -> 65535), all filters x formats x
  gamma over 16 size pairs (1x1, 1xN, Nx1, odd, up / down / mixed), identity exactness,
  nearest integer replication, Fant 2x = box, transparent pixels with random colours (no colour
  bleeding), 65535-wide / tall images, vertical taps beyond the ring budget (grouped sums),
  strides > 4 GB, thread bands 1 vs 4, mip_down /
  mip_chain / incremental dirty rects.
* `python tools/xform_check.py` — 758 cases: matrix helpers and xf_sincos bit-exact, cubic table,
  identity / integer translations exact for every edge mode, rotate 90 = img_rotate90, 6 general
  matrices x 4 filters x 4 edge modes x formats x gamma, clip rects, bands 1 / 3 / 7, NaN / inf /
  huge / zero matrices, 1x1 / 1xN / Nx1 sources, strides > 4 GB.
* `python tools/gradient_check.py` — 487 cases: 7 types x 3 repeats x colour / dither / alpha
  mode / A8 / T, 5 geometries, offsets, widths 1 .. 129, degenerate start == end.

Timings (4000 x 3000 straight ARGB, single thread, measured on Windows while the machine was
quiet; later runs under 85-100 % CPU load from parallel agents were 2-3x slower on both targets,
e.g. Linux/WSL Lanczos-3 169 ms):
resize to 2000 x 1500: nearest 2.9 ms, bilinear 40 ms, bilinear LQ 36 ms, bicubic 50 ms, bicubic
smooth 49 ms, Lanczos-3 56 ms (target < 250 ms), Fant 37 ms, adaptive 75 ms, Lanczos-3 gamma 144 ms,
Lanczos-3 on 8 pool bands 14 ms; upscale to 6000 x 4500: bilinear 132 ms, bicubic 149 ms, Lanczos-3
176 ms; mip_down 13.6 ms. rs_affine rotate 30 degrees: nearest 96 ms, bilinear 345 ms, bicubic
594 ms (145 ms on 8 bands), multisample 4.5 s; integer translation 9 ms. Gradients: linear 334,
reflected 285, diamond 283, radial 271, conical 119, spiral 95-98 Mpx/s (target >= 60 for
linear / radial), A8 radial 304 Mpx/s.
