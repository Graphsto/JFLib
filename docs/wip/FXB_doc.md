# FXB: blur / artistic / noise / photo / object effects — documentation fragment

For `ASMLIB/docs/DESIGN.md` §7 ("As built", fx) and `ASMLIB/docs/API.md`. Files (owner FXB):

| file | contents |
|---|---|
| `ASMLIB/src/fx/fx_blur.s` | `fxtab_blur`: bokeh_blur, fragment_blur, gaussian_blur, median, motion_blur, radial_blur, sketch_blur, square_blur, surface_blur, unfocus, zoom_blur |
| `ASMLIB/src/fx/fx_artistic.s` | `fxtab_artistic`: ink_sketch, oil_painting, pencil_sketch |
| `ASMLIB/src/fx/fx_noise.s` | `fxtab_noise`: add_noise, reduce_noise |
| `ASMLIB/src/fx/fx_photo.s` | `fxtab_photo`: glow, red_eye_removal, sharpen, soften_portrait, straighten, vignette |
| `ASMLIB/src/fx/fx_object.s` | `fxtab_object`: drop_shadow |
| `ASMLIB/src/fx/fx_conv.s` | shared helpers: maths, row loader, separable / disc convolution engines, paint.net 3 Brightness/Contrast, blur→B/C pipeline |
| `ASMLIB/src/fx/fx_lochist.s` | sliding local histograms (median / percentile / rank / oil buckets) |
| `ASMLIB/src/fx/fxb_int.inc` | records (KERN, SEP, DSC, LH, HIST, OIL), macros ROWADDR / CANCELQ |
| `tools/fx_check_b.py` | numpy references + checks (imports FXA's `tools/fx_check.py`) |

No change to `fx.inc`, `fx_common.s`, `fx_all.s`, `build.py` was needed (the table names were
already registered). Includes used: `gfx2.inc` (BM_*), `gfx3.inc` (RS_*, XF_*, XA_*), `thread.inc`.
Library functions used: `rs_to_work` / `rs_from_work` (G2, working rows), `rs_affine_prepare` /
`rs_affine_rows` / `xf_*` (G2, Straighten), `k_bm_row` (G1, blend modes), `fx_hash32`, `mem_*`.
All kernels: pure, thread safe (state in the frame / one `mem_alloc` block per call, frames < 4 KB),
cancel polled once per output row, straight ARGB in and out, no FXF_BANDS / FXF_WHOLE / prepare.

---------------------------------------------------------------------------------------------------
## Conventions

* **W16 working rows** (as G2's resampler): 4 × i16 per pixel (B, G, R, A), premultiplied,
  full = 16320 (255·64); in: `rs_to_work` (P_a = a<<6, P_c = ((c·a+2)·16448)>>16; RS_GAMMA: linear
  light), out: `rs_from_work` (colour ratio kept, so transparent neighbours never bleed colour).
  Every averaging effect works on W16, i.e. premultiplied.
* **Exactness / tile independence**: all neighbourhood sums are integers (running box sums, prefix
  sums, FIR dot products, histogram counts); float appears only per pixel in a fixed operation
  order (IEEE, SSE2, no FMA). The result of a pixel therefore never depends on the ROI split, the
  thread count or the OS (verified: tiles 4/37/48/50/64/80/256/whole, 1 thread vs pool, Windows vs
  Linux — identical).
* **Edges**: separable / disc blurs extend the image by clamping; the local-histogram effects
  (Median, Surface, Sharpen, Reduce Noise, Oil) and the paint.net sampling effects (Fragment, Zoom,
  Radial, Motion) skip pixels outside the image like paint.net; Drop Shadow treats the outside as
  transparent.
* **Transparent pixels**: their colour never influences a visible result (checked for every effect
  by inverting the colour of all alpha-0 pixels): averaging is premultiplied; the local histograms
  skip alpha-0 pixels in the colour channels (`LHF_ALPHA0`); the oil buckets treat them as
  transparent black; Ink Sketch's edge kernel runs on premultiplied colours.
* **Radius → sigma** (Gaussian and everything built on it): paint.net 3's "Gaussian" was a triangle
  kernel of radius r; we use a true Gaussian with the same variance, σ² = r(r+2)/6
  (`fxc_var12(r)` = 12σ² = 2r(r+2)). σ ≤ 6 (r ≤ 13): exact FIR, h = ceil(3σ),
  q_i = floor(exp(−6i²/var12) / Σ · 16384 + 0.5), centre absorbs the rounding; larger: cascade of
  3 box filters (Kovesi widths, integer formula in `fxc_kern_gauss`), each pass rounded through f32.

---------------------------------------------------------------------------------------------------
## Effects (keys, properties min..max = default, semantics)

Property keys are ASCII (apps map them to German labels). FP_PAN = 2 slots (x, y), -1..1, 0 = centre.

### Blurs
* **gaussian_blur** — radius int 0..200 = 2. Gaussian above (r = 0: copy).
* **square_blur** — radius int 0..200 = 2. Mean of the (2r+1)² square (one box per axis, running sums).
* **unfocus** — radius int 1..200 = 4. Mean over paint.net's disc u²+v² ≤ ((2r+1)²+2)/4
  (paint.net 3's Unfocus = alpha-weighted disc histogram mean). Prefix sums per row (exact i32).
* **bokeh_blur** — radius int 1..100 = 25, intensity int 0..100 = 50, granularity int 0..100 = 50.
  Own design: disc mean in linear light (RS_GAMMA) with highlight boost: lum = (2126R+7152G+722B)/(10000A)
  (f32), t = clamp((lum − th)/(1 − th)), th = 0.95 − 0.009·granularity, colour × (1 + 0.0064·i²·t²),
  alpha unboosted; sums f32; values above 0.8·A are compressed x' = 0.8 + 0.2(1 − e^(−5(x−0.8))).
  Small highlights become bright discs; granularity = how much of the tonal range counts as highlight.
* **median** — radius int 1..200 = 10, percentile int 0..100 = 50. paint.net LocalHistogramEffect
  disc; per channel the smallest v with count(≤v) ≥ max(n·pct/100, 1) (paint.net GetPercentile);
  colour n = non-transparent pixels.
* **motion_blur** — angle 25° (−180..180), distance int 1..200 = 10, centered bool = 1.
  paint.net 3 geometry: θ = angle+180°, N = (1+d)·3/2 samples on the segment (centred or not);
  our samples are bilinear at 1/16 px (P = rint(16p)), premultiplied, samples outside the image
  skipped, mean = rint(f32(S)/(256n)). Interior pixels use the collapsed tap list (same integer sums).
* **radial_blur** — angle 2° (0..360), offset pan, quality int 1..5 = 2. paint.net 3's fixed-point
  rotation (n = q²(30+q²) steps each way, small-angle rotation formula) in 64-bit instead of
  wrapping 32-bit arithmetic; samples round((x,y)+c), ColorBgra.Blend mean (alpha-weighted,
  truncating). Samples at column/row 0 are allowed (paint.net 3 skipped them).
* **zoom_blur** — amount int 0..100 = 10, offset pan. paint.net 3's 64-step fixed-point zoom
  (centre (w<<15) + trunc(w·ox·32768)), ColorBgra.Blend.
* **fragment_blur** — fragments int 2..50 = 4, distance int 0..100 = 8, rotation angle 0..360 = 0.
  paint.net 3: offsets round_away(d·(−sin t), d·(−cos t)), t = (rot−90)° + i·360°/n, mean of the
  samples inside the image (ColorBgra.Blend).
* **surface_blur** — radius int 1..100 = 6, threshold int 1..100 = 15. Histogram bilateral over the
  disc: W[d] = floor(256·exp(−d²/(2t²)) + 0.5), v' = round(Σh·W·i / Σh·W) per channel (own weights;
  transparent centre uses the colour median as v).
* **sketch_blur** — radius int 1..50 = 10, angle 45°, spread int 0..180 = 60, seed. Own design of
  paint.net 5's look: three layers of stroke cells (length 2R+1 along the angle, width 4 px, layer
  offsets from fx_hash32); each cell picks a direction angle + spread·(r/255 − ½) (r = fx_hash32 of
  the cell); the pixel averages 2R+1 bilinear samples along that direction in every layer.

### Artistic
* **ink_sketch** — ink_outline int 0..99 = 50, coloring int 0..100 = 50. Colour layer = Glow
  (radius 6, brightness = contrast = −(c−50)·2, as paint.net 3); ink: 5×5 kernel (centre 24, others
  −1) on premultiplied colours, e = clamp per channel, I = intensity byte, d = min(255, I·(k+5)>>6),
  colour × (255−d)/255 (own ink curve).
* **oil_painting** — brush_size int 1..8 = 3, coarseness int 3..255 = 50. paint.net 3: square window,
  pixels bucketed by FastScale(intensity, coarseness), fullest bucket (lowest on ties) wins; its
  mean colour alpha-weighted (= paint.net for opaque pixels), alpha = ΣA/n.
* **pencil_sketch** — pencil_tip_size int 1..20 = 2, color_range int −20..20 = 0. paint.net 3:
  ColorDodge(desaturate(src), desaturate(invert(B/C(blur(src, tip), range, −range)))).

### Noise
* **add_noise** — intensity int 0..100 = 64, color_saturation int 0..400 = 100, coverage int
  0..100 = 100, seed. paint.net 3's formula (dev = i²/4, sat = s·4096/100, luminance-saturation mix,
  (n·dev + 32768) >> 16) with per-pixel hashes: h_k = fx_hash32(x, y, seed, k), changed when
  h_0 mod 100 < coverage, noise = (sum of the 4 bytes of h_k − 510)·1774 >> 8 (Irwin–Hall, σ ≈ 1024
  like paint.net's normal table), k = 1 R, 2 G, 3 B. Alpha kept.
* **reduce_noise** — radius int 0..200 = 6, strength dec 0..1 = 0.4. Lerp towards the local median
  with paint.net's strength curve f = strength·(1 − 0.75·I). **Deviation**: paint.net 3 / Pinta lerp
  towards the rank-equalised value (count < v)·255/N, which adds speckle in flat areas (checked
  visually) — the median keeps the intended "noise reduced, edges kept" look. Alpha kept.

### Photo
* **glow** — radius int 1..20 = 6, brightness int −100..100 = 10, contrast int −100..100 = 10.
  paint.net 3: Screen(src, B/C(blur(src))).
* **red_eye_removal** — tolerance int 0..100 = 70, saturation int 0..100 = 90. paint.net 3's
  RedEyeRemove op on every ROI pixel (the app limits it to the selection): if R − max(G,B) >
  tolerance and HSV saturation·255 > 100: R = trunc(255·I·sat/100).
* **sharpen** — amount int 1..20 = 10. paint.net 3: disc radius = amount, per channel
  clamp(floor((3s − median)/2)) (= Lerp(src, median, −0.5)).
* **soften_portrait** — softness int 0..10 = 5, lighting int −20..20 = 0, warmth int 0..20 = 10.
  paint.net 3: G = B/C(blur(src, 3·softness), lighting, −lighting/2); L = desaturated src with
  R·(1+w/100), B·(1−w/100) (f32, truncated); Overlay(L, G), alpha = src.
* **straighten** — angle −45..45 = 0 (positive = clockwise on screen, like xf_rotate), zoom bool = 1.
  rs_affine bicubic about the image centre; zoom scales by s = cos|a| + max(w/h, h/w)·sin|a| so no
  empty corner remains (edges clamped); zoom 0 leaves transparent corners. Matches G2's reference.
* **vignette** — offset pan, radius dec 0.1..4 = 0.5, density dec 0..1 = 1, color = black.
  ρ = |p − c| / (2·radius·half-diagonal), v = density·colour.a/255·smoothstep((ρ−0.4)/0.7); blend
  towards the colour in linear light (premultiplied W16), alpha kept. (Own curve; paint.net 4.1+
  look: clear centre, dark corners at radius 0.5.)

### Object
* **drop_shadow** — color = black, angle 45° (0 = right, 90 = down), distance int 0..100 = 10,
  blur_radius int 0..100 = 10, opacity int 0..100 = 50, shadow_only bool = 0. Shadow alpha =
  Gaussian (radius semantics above) of the source alpha moved by rint(d·(cos, sin)), transparent
  outside the layer; shadow = colour with alpha round(S·opacity·colour.a / (16320·100·255));
  result = source over shadow (k_bm_row Normal) or the shadow alone. The shadow may extend
  anywhere inside the layer bounds.

---------------------------------------------------------------------------------------------------
## Shared helpers (usable by every fx file)

`.include "src/fx/fxb_int.inc"` after `jfgui.inc` / `fx.inc` for the records and macros.

### fx_conv.s
```
fxc_exp(x) / fxc_ln(x) / fxc_pow(x, y) -> xmm0      f64 maths (Cody–Waite + Taylor, atanh series)
fxc_roi(args, rect) -> eax                          ROI clamped to dst (1 = not empty)
fxc_pint(args, slot, lo, hi) -> eax / fxc_pdbl(args, slot) -> xmm0    parameters
fxc_copy_roi(args) -> eax                           dst ROI = src ROI
fxc_load_row(dst, src, y, x0, x1, mode)             W16 row, clamp (default) / FXL_ZERO, FXL_GAMMA
fxc_var12(r) -> eax;  fxc_kern_gauss(kern, var12);  fxc_kern_box(kern, h)
fxc_sep_init(sep, args, kern) -> eax;  fxc_sep_run(sep) -> eax 0 / ERR_NOMEM / ERR_CANCEL
fxc_emit_argb(sep, y, row)                          standard SP_EMIT (W16 -> straight ARGB)
fxc_disc_init(dsc, args, r) -> eax;  fxc_disc_run(dsc) -> eax;  fxc_disc_emit_argb(dsc, y, row)
fxc_disc_edges(tab, r) -> eax area                  paint.net disc half widths
fxc_bac_init(tab, brightness, contrast) -> eax mode;  fxc_bac_row(dst, src, n, tab, mode)
fxc_desat_row(dst, src, n)
fxc_blurbac(args, r, brightness, contrast, fn) -> eax    blur(r) -> B/C -> fn(args, y, x0, n, row)
row kernels: fxc_pairs, fxc_fir_h, fxc_box_h, fxc_fir_v, fxc_box_v, fxc_wvec
```
**SEP** (separable run, `SP_SIZE` 96): source, destination, optional loader
`SP_LOAD(sep, dst_w16, y, x0, x1)` (default = clamped `fxc_load_row` with `SP_MODE`), `SP_EMIT(sep,
y, w16row)` per output row, H and V kernels (KERN), ROI, job, two user fields. The ROI is processed
in strips of up to 256 rows: H pass for rows y0−HY .. y1+HY (clamped rows computed once), V pass over
a row-pointer table (FIR: pmaddwd pairs; boxes: running i32 row sums), then emit.
**DSC** (disc run): optional `DS_PREP(dsc, dst_i32x4, y, x0, x1)` (default W16 widened), per source
row prefix sums (i32, wrapping), per output row Σ over the disc rows of P[x+e+1] − P[x−e]
(i32, or f32 with `DSF_F32`), `DS_EMIT(dsc, y, row)` with `DS_AREA` set.

### fx_lochist.s
```
lh_init(lh, args, fn, r) -> eax      LH for an effect (disc, LHK_HIST, ROI of args)
lh_run(lh) -> eax 0 / ERR_NOMEM / ERR_CANCEL
lh_percentile(hist, ch, target) -> eax   smallest v with count(<= v) >= max(target, 1), ch 0 B 1 G 2 R 3 A;
                                         255 when the histogram holds fewer than target pixels
lh_rank(hist, ch, v) -> eax              pixels with value < v
lh_edges(tab, r, shape) -> eax area      half widths of the window rows
```
LH record (`LH_SIZE` 96, zero it or use lh_init): `LH_SRC`, `LH_DST`, `LH_FN` = fn(lh, hist, x, y) ->
eax straight ARGB written to dst(x, y), `LH_USER` / `LH_USER2` / `LH_USER3`, `LH_JOB`, `LH_R`,
`LH_SHAPE` (`LHS_DISC` paint.net disc / `LHS_SQUARE`), ROI `LH_X0..LH_Y1`, `LH_FLAGS`
(`LHF_ALPHA0`: alpha-0 pixels not counted in the colour histograms), `LH_KIND` (`LHK_HIST`;
`LHK_OIL` = intensity buckets, `LH_USER2` = levels, callback gets an OIL record).
HIST record (read only in the callback): `HI_FINE + c·1024` u32 counts[256] of channel c,
`HI_COARSE + c·64` u32 counts[16] (v >> 4), `HI_NALL` pixels (alpha histogram), `HI_NCOL` pixels in
the colour histograms, `HI_SRC` centre pixel. Pixels outside the image are not counted. Traversal:
the first column's window slides down one row per ROI row, is copied, then slides right (2(2r+1)
updates per pixel + the callback). For paint.net's Outline (FXA): paint.net loops
`while (b < 255 && count < minCount) count += h[b++]`, i.e. b = lh_percentile(h, c, minCount) + 1
for minCount ≥ 1 (clamped to 255), b = first non-empty value for minCount = 0.

---------------------------------------------------------------------------------------------------
## Tests — `python tools/fx_check_b.py [--target win|linux|both] [--out build/fxb] [--only a,b]`

* Per effect a numpy reference of the documented integer / float pipeline (W16 conversions,
  kernels, histograms, fx_hash32, xf_sincos via xform_check, blend modes via blend_check, affine
  via xform_check): **bit-exact for all 23 effects** (Bokeh allows ±1 for exp() ulp differences;
  measured 0). Parameter sweeps incl. radius 0 / min / max, radius larger than small images,
  1×1 / 2×3 / tiny images, all angles / offsets / seeds.
* Own generic checks: tiles 64 / 256 / 37 / whole × 1 thread / pool identical, ROI, cancel
  (ERR_CANCEL), colour of alpha-0 pixels must not change any visible pixel.
* FXA's `fc.generic_checks` (imported): serial/256 vs pool/64 / whole / 37, two fill colours (every
  ROI pixel written), nothing written outside the ROI, selection lerp, cancel latency.
* Extreme sweep: every property at min and max (and all-min / all-max) on 1×1, 2×3, 17×9 images,
  tile 4 serial == tile 64 pool; efftest's guard bytes / source-unchanged checks.
* `--shots DIR` writes PNGs of every effect on the generated photo-like test image
  (`natural()`: sky gradient, sun, textured hills, soft shapes, fine lines, highlights, red eyes,
  a translucent strip with garbage colours under alpha 0); all were inspected.
* Result: **399 checks, 0 failures on Windows and on Linux (WSL)**. asmcheck on the 7 files:
  0 errors, 0 warnings (21 `assume` annotations for loop indexes bounded by parameters).

---------------------------------------------------------------------------------------------------
## Performance (4000 × 3000 opaque photo-like image, efftest pool = 13 threads, tile 256, Ryzen
## 5 3600 shared with other agents' builds, so ±30 %)

| effect (params) | ms | | effect (params) | ms |
|---|---|---|---|---|
| gaussian r=2 (default) | 56 | | glow (default) | 67 |
| **gaussian r=50** (target < 300) | **88–130** | | soften_portrait | 90 |
| gaussian r=200 | 147–155 | | sharpen amount 10 | 484 |
| square r=50 | 50 | | red_eye_removal | 8 |
| unfocus r=4 / r=20 | 36 / 114 | | straighten 7° (bicubic) | 216 |
| **median r=10** (target < 1.5 s) | **430–577** | | vignette | 67 |
| **motion 50 px** (target < 1 s) | **138–183** | | **oil brush 5** (target < 2 s) | **378–431** |
| motion 10 px (default) | 42 | | oil brush 3 (default) | 220 |
| zoom (default) | 533 | | pencil_sketch | 75 |
| radial (default, q=2: 272 samples/px) | 2800–3200 | | ink_sketch | 214 |
| fragment | 46 | | add_noise | 49 |
| surface r=6 | 694 | | reduce_noise r=6 | 275 |
| bokeh r=25 | 241 | | drop_shadow | 98 |
| sketch r=10 | 934 | | | |

Tile size matters for large radii (halo overhead ∝ (T + 2·3σ)² / T²): gaussian r=50 is 390 ms with
64-px tiles and 500 ms as one whole-image call on one thread. Runners should use ≥ 256-px work
units for the blur family.

---------------------------------------------------------------------------------------------------
## Deviations from paint.net / open issues

* Gaussian uses σ² = r(r+2)/6 (variance of paint.net 3's triangle kernel), clamped edges (paint.net
  renormalised over the image). Everything built on it (Glow, Soften, Pencil, Ink, Drop Shadow)
  inherits this.
* Reduce Noise pulls towards the local median (see above); Median / Sharpen / Surface / Reduce
  Noise skip alpha-0 pixels in colour histograms; Oil colour means are alpha-weighted.
* Own designs (no reference available): Bokeh, Sketch Blur, Surface Blur weights, Ink Sketch ink
  curve, Vignette curve, Straighten zoom, Drop Shadow parameterisation. Property keys / defaults of
  Bokeh, Sketch Blur and Drop Shadow are best guesses of paint.net 5.1's dialogs.
* Radial Blur keeps paint.net 3's per-pixel rotation recurrence (latency bound, ~3 s at the
  default quality on 12 MP); a faster arc-sampling design would change results.
* Whole-image single calls with radius ≥ 100 recompute halo rows per 16..256-row strip (strip
  buffers are capped at 16 MB); a streaming V pass would remove that (not needed with tiles).
* Straighten sign: positive = clockwise on screen (the app may negate it to match its UI).
