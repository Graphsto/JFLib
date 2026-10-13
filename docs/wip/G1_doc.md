# G1: graphics kernels (straight alpha, blend modes, tiled masks, flood fill) — documentation fragment

For `ASMLIB/docs/DESIGN.md` §2 / §7 ("As built") and `ASMLIB/docs/API.md`. New library files:
`ASMLIB/src/gfx/straight.s`, `blendmode.s`, `tmask.s`, `flood.s`. Public constants (`BM_*`,
`VC_*`, `TM_*`, `FF_GLOBAL`) live in the new include file `ASMLIB/inc/gfx2.inc` (guarded with
`.ifndef GFX2_INC`, `=` definitions only, safe to include twice). Until `jfgui.inc` includes it,
sources write `.include "gfx2.inc"` after `jfgui.inc` (all four modules and apps/fxtest do).
Test harness: `apps/fxtest` + `tools/blend_check.py`, `tools/tmask_check.py`, `tools/flood_check.py`.

Note for API.md: `tools/gendoc.py` treats the `k_` prefix as internal (INTERNAL_PREFIXES), so the
new `k_*` kernels below would not appear in API.md; either drop `k_` from that list or paste the
kernel table below by hand. `gendoc.py --check` passes for all four files.

---------------------------------------------------------------------------------------------------
## DESIGN.md §2 — additions

### 2.5 Straight-alpha pixels (FMT_ARGB) — gfx/straight.s
Layers are straight (not premultiplied) 0xAARRGGBB = bytes B, G, R, A, like paint.net's
ColorBgra. Premultiplied data only exists internally (view mips, resampling, blur). All kernels
take any n (n <= 0 does nothing), have no alignment requirements, never touch memory past n
pixels, may run in place (dst == src), use SSE2 with xmm0–5 only and keep no state (thread safe).
round(x / 255) is always `((x + 128) * 257) >> 16` (exact for 0 <= x <= 65025; no ties since 255
is odd).
```
k_premul_row(dst, src, n)                 c' = round(c * a / 255), a kept (fast paths a = 255 / 0)
k_unpremul_row(dst, src, n)               bit-identical to png.s unpremul_row:
                                          c = min(255, (c * ceil(255 * 2^24 / a) + 2^23) >> 24),
                                          a = 0 -> pixel 0, a = 255 -> unchanged; the table is
                                          generated at assembly time (no init, thread safe)
k_lerp_mask_row(dst, a, b, m, n)          dst = round((a * (255 - m) + b * m) / 255) for all 4
                                          channels (straight lerp), m = A8 row; m 0 -> a, 255 -> b
k_erase_row(dst, cov, n, strength)        e = round(cov * strength / 255);
                                          alpha = round(alpha * (255 - e) / 255); colour bytes kept
k_view_row(dst, src, n, x0, y0, cfg)      straight src over a checkerboard into an XRGB row:
                                          c = round((s * a + bg * (255 - a)) / 255), alpha byte 255
k_view_row_pm(dst, src, n, x0, y0, cfg)   same for premultiplied src: c = s + round(bg * (255 - a) / 255)
```
Checkerboard: `cfg` = VC record {i32 VC_CELL (>= 1), u32 VC_C0, u32 VC_C1} (colour alpha
ignored); pixel (x, y) lies in cell (floor(x / cell), floor(y / cell)) and gets C1 when the cell
coordinates sum to an odd number, else C0; (x0, y0) are the checkerboard coordinates of the
first pixel and may be negative. The kernel fills the row with the pattern, then composites in
place (dst must not overlap src).
Round-trip rules (verified exhaustively): `premul(unpremul(p)) == p` for every valid
premultiplied p (c <= a); `unpremul(premul(s)) == s` exactly when a = 255, otherwise
|Δc| <= ceil(128 / a).

### 2.6 Layer blend modes — gfx/blendmode.s
```
k_bm_row(dst, lhs, rhs, n, mode | opacity << 8)             layer rhs over layer lhs
k_bm_cov_row(dst, lhs, rhs, cov, n, mode | opacity << 8)    rhs alpha additionally x A8 cov
k_bm_solid_row(dst, lhs, argb, cov, n, mode | opacity << 8) solid straight colour through cov
```
lhs = bottom row, rhs = top row, dst may equal lhs; cov = 0 (null) means full coverage. Modes
(`BM_*`, paint.net LayerBlendMode order as stored in .pdn): Normal 0, Multiply 1, Additive 2,
ColorBurn 3, ColorDodge 4, Reflect 5, Glow 6, Overlay 7, Difference 8, Negation 9, Lighten 10,
Darken 11, Screen 12, Xor 13, plus BM_OVERWRITE 14 (tools). Unknown modes act as Normal.
Arithmetic = paint.net UserBlendOps (generated code of paint.net 3.36 / Pinta), all integers:
```
INT_SCALE(a, b) = t = a*b + 0x80; ((t >> 8) + t) >> 8              (= round(a*b / 255))
rhsA   = INT_SCALE(INT_SCALE(rhs.A, opacity), cov)                 (cov = 255 without a row)
y      = INT_SCALE(lhs.A, 255 - rhsA);   totalA = y + rhsA;        totalA = 0 -> pixel 0
x      = INT_SCALE(lhs.A, rhsA);         z = rhsA - x
c      = floor((lhs.c*y + rhs.c*z + F(lhs.c, rhs.c)*x) / totalA)    c = B, G, R;   A = totalA
F: Normal rhs · Multiply INT_SCALE(l, r) · Additive min(255, l + r) ·
   ColorBurn r == 0 ? 0 : max(0, 255 - (255 - l)*255 / r) · ColorDodge r == 255 ? 255 : min(255, l*255 / (255 - r)) ·
   Reflect r == 255 ? 255 : min(255, l*l / (255 - r)) · Glow l == 255 ? 255 : min(255, r*r / (255 - l)) ·
   Overlay l < 128 ? INT_SCALE(2l, r) : 255 - INT_SCALE(2(255 - l), 255 - r) · Difference |l - r| ·
   Negation 255 - |255 - l - r| · Lighten max · Darken min · Screen r + l - INT_SCALE(r, l) · Xor l ^ r
BM_OVERWRITE: dst = round((lhs*(255 - m) + rhs*m) / 255) on all four channels, m = INT_SCALE(cov, opacity)
```
The coverage kernels leave pixels with cov = 0 bit-exactly unchanged; `k_bm_row` applies the
formula everywhere (so a transparent lhs pixel under a transparent rhs pixel becomes 0, as in
paint.net). Opacity handling (rhsA = INT_SCALE(rhs.A, opacity)) is still to be confirmed
against real paint.net.

Implementation: one loop function per mode (macro `BM_FUNC` + one `F_*` macro per mode, jump
table `bm_ftab`), 2 pixels per step in 8 u16 lanes, the odd last pixel runs through the same
code on a stack copy (so tails are bit-identical to the body). Every numerator is <= 255*255 and
every divisor 1..255, so all divisions (the final one by totalA and those inside ColorBurn /
ColorDodge / Reflect / Glow) use `cvtdq2ps / divps / cvttps2dq`: the correctly rounded float
quotient of n/d is exact when integral and otherwise at least 1/d - q*2^-24 > 0 away from the
next integer, so truncation is exact floor division (verified for all n <= 65535, d <= 255).
No tables, no state: thread safe. Fast paths per pixel pair: coverage 0 (copy lhs), transparent
rhs (lhs with alpha-0 pixels zeroed), transparent lhs at full coverage/opacity (rhs likewise),
Normal with opaque rhs (store rhs), any mode with opaque lhs and rhs (store F, alpha 255).
Measured (Ryzen 5 3600, 1 Mpx rows, random pixels = worst case / opaque / cov + opacity 200):
Normal 206 / 1180 / 200 Mpx/s (Windows), 186 / 972 / 207 (Linux); slowest mode ColorBurn
143 Mpx/s random; Overwrite 870–1040. k_premul_row 1075–1900, k_unpremul_row 220–280 (scalar per
mixed pixel, SSE2 skip for opaque / transparent runs), k_lerp_mask_row 790–1010, k_view_row
620–750 Mpx/s.

### 2.7 Sparse tiled masks (TMASK) — gfx/tmask.s
A TMASK is an A8 coverage mask of 1..65535 x 1..65535 pixels in 64 x 64 tiles:
```
TM_W 0 i32 · TM_H 4 i32 · TM_TW 8 i32 (w+63)>>6 · TM_TH 12 i32 · TM_SLOTS 16 u64 -> TW*TH u64 slots
TM_SLABS 24 u64 slab list · TM_FREE 32 u64 free tiles · TM_NTILES 40 u32 tiles in use · TM_NSLABS 44 u32
TM_SIZE 64 (the slot array follows the record in the same mem_alloc block)
slot = TM_EMPTY 0 (all 0) | TM_FULL 1 (all 255) | pointer to 4096 bytes (64 rows x 64, stride 64)
```
Tiles are 64-byte aligned and carved from per-mask slabs of up to TM_SLAB_TILES = 32 tiles
(one mem_alloc of 128 KB + 64; fewer for small masks), linked into a per-mask free list;
slabs are returned by `tm_clear` / `tm_free` only. Memory: 64 + 8·TW·TH bytes (8 MB for
65535²) plus 4 KB per non-uniform tile. Pixels of edge tiles outside w x h are "don't care":
every operation ignores them. Operations turn tiles that come out uniform (0 / 255 over the
in-range area) back into sentinels, so masks stay sparse.
```
tm_alloc(w, h) -> rax TMASK* | 0 (bad size / no memory)      tm_free(tm)   (tm may be 0)
tm_clear(tm, value) -> eax 0 | ERR_NOMEM      0 / 255 free every tile; other values materialise
tm_get(tm, x, y) -> eax                       0 outside
tm_span(tm, x, y, maxn) -> rax p, edx n       p = TM_EMPTY / TM_FULL (sentinel runs continue over
                                              neighbouring tiles of the same kind, up to maxn) or a
                                              pointer to n <= 64 bytes inside one tile; outside the
                                              mask: TM_EMPTY (x < 0: n = min(maxn, -x))
tm_row(tm, x, y, n, buf) -> rax               pointer to n bytes of (x..x+n-1, y): into the tile when
                                              one tile holds the run, else buf is filled (0 outside)
tm_row_w(tm, x, y) -> rax p, edx n            writable pointer (materialises the tile), n = bytes to
                                              the end of the tile row (<= w - x); 0 = outside / OOM
tm_tile_w(tm, tx, ty) -> rax tile             writable 64 x 64 tile with its content, 0 on failure
tm_tile_set(tm, tx, ty, kind)                 slot := TM_EMPTY (kind 0) / TM_FULL (else)
tm_fill_rect(tm, x0, y0, x1, y1, value) -> eax  x1, y1 exclusive, clipped; whole tiles -> sentinels
tm_combine(dst, src, op) -> eax               per pixel, same sizes (else ERR_FAIL), dst may be src
tm_combine_block(tm, tx, ty, src, stride, op) -> eax   64 rows x 64 bytes at src (or a sentinel)
tm_copy(dst, src) -> eax · tm_clone(src) -> rax · tm_invert(tm) · tm_optimize(tm)
tm_threshold(tm, level)                       v = v >= level ? 255 : 0, level clamped to 1..255
tm_bbox(tm, rect) -> eax 0 | 1                rect = i32 x0, y0, x1, y1 (exclusive) of v != 0 (0s if empty)
tm_count_tiles(tm) -> eax tiles in memory, edx TM_FULL slots
tm_load_a8(tm, src, stride) -> eax · tm_store_a8(tm, dst, stride)     dense w x h A8 images
tm_raster_path(tm, rule, op) -> eax · tm_raster_polygon(tm, pts_f32, n, rule, op) -> eax
tm_trace(tm, cb, user) -> eax 0 | ERR_NOMEM | first nonzero cb result
```
Combine ops (`TM_*`): REPLACE 0 = src, ADD 1 = max(d, s), SUB 2 = round(d·(255 − s)/255),
INTERSECT 3 = round(d·s/255), XOR 4 = |d − s|; sentinel shortcuts per tile (e.g. ADD with a full
src tile just sets TM_FULL, INTERSECT with an empty src frees the tile).

Rasterization: `tm_raster_path` takes the current path of path.s (path_begin / path_move[_f] /
path_line[_f] / curves; every subpath closed implicitly, RULE_NONZERO / RULE_EVENODD), builds
the line list once (path_lines_begin / path_emit_line), then renders chunks of 64 rows x up to
4096 px through the existing rasterizer: a private FMT_A8 target whose SF_PX is offset so that
the chunk's pixels land in a small buffer (gfx_begin + gfx_clip_push + colour 0xFFFFFFFF, opacity
255 + ras_fill: k_a8_over on a zeroed buffer stores exactly the coverage), and merges the chunk
tile by tile with `tm_combine_block`. REPLACE clears the mask first, INTERSECT clears the tiles
outside the path's bounding box; the draw context (target, clip, origin, opacity, colour) is
restored. Because it uses g_dc / g_lines this function is main-thread only; everything else in
tmask.s touches only the masks passed in. Coverage = the rasterizer's exact area coverage
(verified: max |Δ| <= 2 against an exact analytic area reference for simple polygons, holes,
partly outside polygons, chunk boundaries).

Outlines: `tm_trace` walks pixel edges of the binary image v >= 128 (TM_TRACE_LEVEL) and reports
each closed contour as `cb(user, pts, n) -> eax` (0 = continue) with n i32 (x, y) corner points in
pixel-edge coordinates (0..w, 0..h), implicitly closed, no collinear points. The inside is on the
right of the walking direction (screen coordinates, y down): outer contours are clockwise, holes
counter-clockwise, so a non-zero fill of the contours reproduces the mask. Pixels touching only
at a corner are separate (4-connectivity, matching the flood fill): at a saddle the walk always
turns right. Every boundary edge is reported exactly once (visited bits for horizontal edges,
allocated lazily per tile from 64 KB blocks). Only rows of non-uniform tiles, tile borders and
the mask bottom are scanned, so huge sparse masks are cheap (65535² with 3 small regions: 4–6 ms).

### 2.8 Flood fill — gfx/flood.s
```
flood_fill_mask(src, x, y, tolerance_pct, flags, out) -> eax 0 | ERR_NOMEM | ERR_FAIL
```
src = 32-bit surface (FMT_ARGB; the test uses the raw B, G, R, A bytes; any stride),
out = TMASK at least as large as src. out is cleared, then every pixel that matches the seed
colour and (without `FF_GLOBAL` = 1) is 4-connected to the seed through matching pixels becomes
255 (hard edges; antialiasing is up to the caller). ERR_FAIL: A8 / empty surface, mask too small,
seed outside src (the mask is cleared when it is usable). ERR_NOMEM leaves an empty mask.
Colour test = paint.net's CheckColor (a = seed, b = candidate):
```
sum = Σ_{c ∈ R,G,B} floor((1 + (a.c − b.c)²) · a.A / 256) + (a.A − b.A)²
tol = round(pct · 255 / 100) (pct clamped 0..100);   match ⇔ sum <= tol · tol · 4
```
(the pct → tol scaling is to be calibrated against real paint.net; 0 % matches only colours whose
sum is 0, 100 % matches everything). Implementation: a match bitmap (1 bit per pixel, rows of
ceil(w/64) u64) computed per row on demand with SSE2 (4 pixels per step: |a − b| with psubusb,
d² with pmullw, (1 + d²)·a.A >> 8 from pmullw/pmulhuw halves, sums via a biased pmaddwd), a
scanline fill on the bitmaps with an explicit growable stack of run starts (no recursion, no
dropped seeds), then 64 x 64 bit blocks become TM_EMPTY / TM_FULL slots or tiles (8-byte table
expansion). Memory 2 bits per pixel + h bytes + stack. Thread safe (state in the frame).
Measured on 4000 x 3000: contiguous full fill 18–21 ms, global 16–17 ms, noisy regions 10–30 ms.

---------------------------------------------------------------------------------------------------
## DESIGN.md §7 — "As built" bullets

Graphics (G1)
* New include `gfx2.inc` (BM_*, VC_*, TM_*, FF_GLOBAL; guarded); new modules straight.s,
  blendmode.s, tmask.s, flood.s (see §2.5–2.8).
* Blend modes divide with divps (exact floor for numerators <= 65025, divisors 1..255) instead of
  paint.net's multiply-add-shift table; results are bit-identical to UserBlendOps.
* `tm_raster_path` reuses ras_fill through a private A8 target with an offset SF_PX (no change in
  raster.s); its speed is bounded by the scalar `k_a8_over` of raster.s (~60 % of the time of a
  full-canvas selection: 4000 x 3000 lasso ≈ 20 ms). An SSE2 k_a8_over (or a "store coverage"
  mode for A8 targets) in raster.s would make masks ~2x faster.
* `tm_trace` separates diagonal neighbours (4-connectivity) and reports clockwise outer contours.
* apps/fxtest: batch harness (`--batch IN --result OUT`, guarded buffers with 0xCD pads, timed
  repetitions) and `--selftest` (known answers; run by build.py test via tests/selftest.uis).

---------------------------------------------------------------------------------------------------
## Tests (tools/*.py, both targets, bit-exact unless noted)

* `python tools/blend_check.py --target both` — straight kernels exhaustive over (c, a) incl. the
  png.s reference and round-trip rules, lerp over all (a, b, m), erase over all (alpha, cov) for 10
  strengths, view rows for 6 cell sizes / negative offsets; every blend mode on the full 256 x 256
  channel grid for 20 alpha pairs, 10^6 random pixels x 7 opacities (k_bm_row in place,
  k_bm_cov_row, k_bm_solid_row with and without coverage row); n = 0..67 at every alignment
  0..15 with guard bytes (≈26 000 records); benchmark (Normal >= 150, others >= 40 Mpx/s).
* `python tools/tmask_check.py --target both` — load/store/count sparsity, get/span/row/row_w,
  fill_rect, tile ops, invert, threshold, bbox, copy/clone, all combine ops (also dst == src,
  combine_block with strides / sentinels), rasterization vs exact area (max |Δ| <= 2) and every op
  bit-exact on its own coverage, draw context preserved, trace (closed, rectilinear, corners only,
  each boundary edge once with inside on the right, saddles separated, early stop), 65535² masks.
* `python tools/flood_check.py --target both` — BFS reference on regions / noise / every 5 %
  tolerance step / transparent seeds / checkerboards / snakes / combs / binary noise / padded
  strides / larger masks / bad seeds, output sparsity; benchmark (full fill < 150 ms).

## Open questions (calibrate against real paint.net)
* Layer opacity: rhsA = INT_SCALE(rhs.A, opacity) before the blend op (assumed).
* Flood tolerance scaling tol = round(pct · 255 / 100) with `sum <= tol² · 4` (assumed; paint.net
  may square the percentage differently), and whether the fill compares against the seed colour
  only (as implemented) or a running colour.
* BM_OVERWRITE is a straight (non-premultiplied) lerp including alpha, like ColorBgra.Lerp.
* Selection combine modes in paint.net are geometric (polygon clipping); TM_* are the per-pixel
  equivalents used for the rendered mask.
