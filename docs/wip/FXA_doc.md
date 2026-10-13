# FXA: effect runner (efftest), fx_check harness, adjustments, Quantize, Stylize, fx helpers

Documentation fragment for `ASMLIB/docs/DESIGN.md` §7 / `API.md` / `GUIDE.md` (integrator).

| File | Content |
|---|---|
| `ASMLIB/src/fx/fx_adj.s` | `fxtab_adj`: the 13 adjustments (Korrekturen) |
| `ASMLIB/src/fx/fx_color.s` | `fxtab_color`: Quantize |
| `ASMLIB/src/fx/fx_stylize.s` | `fxtab_stylize`: Edge Detect, Emboss, Outline, Relief |
| `ASMLIB/src/fx/fx_lut.s` | shared helpers: row iterator, LUT rows, intensity, x87 math, sRGB, Levels LUT, spline |
| `ASMLIB/src/fx/fx_hist.s` | histogram helpers, paint.net local (circular, sliding) histogram engine |
| `ASMLIB/inc/fx.inc` | additive: `FP_BLOB = 9` (see §2) |
| `apps/efftest/src/main.s`, `selftest.s`, `inc/efftest.inc`, `tests/selftest.uis` | generic runner |
| `tools/fx_check.py` | numpy-referenced test suites + importable harness |

## 1. efftest -- generic effect runner (stable CLI)

Build: `python build.py app efftest --out build/<you>` -> `build/<you>/win/efftest.exe`,
`build/<you>/linux/efftest` (Linux: `wsl.exe -d Ubuntu-24.04 --cd <repo> -e ./build/<you>/linux/efftest ...`).
Always pass `--shot -` (never written; keeps the runtime headless, no window).

```
efftest --shot - --list                  every registered effect + properties
efftest --shot - --props NAME            one effect
efftest --shot - --fx NAME [OPTIONS]     run one effect
efftest --shot - --ops FILE              one run per line of FILE (each line = OPTIONS incl. --fx;
                                         blank lines and lines starting with '#' are skipped;
                                         tokens separated by blanks, no quoting)
efftest --shot - --selftest [all]        built-in checks (FXA effects; "all": every registered one)
OPTIONS
  --in FILE           source: ".raw"/".argb" = RSR1 {u32 "RSR1", u32 w, u32 h, u32 fmt, h rows of
                      w*4 bytes B,G,R,A}; any other extension -> img_load_ex(IMGD_STRAIGHT) (FMT_ARGB)
  --out FILE          optional result: ".raw"/".argb" RSR1 (fmt 3) or img_save_opt by extension (.png)
  --p key=value       property (repeatable). int/seed: decimal; dec/angle: f64; bool: 0/1/true/false/
                      yes/no/on/off; choice: key or index; pan: "x,y"; color: hex AARRGGBB (<= 6
                      digits -> alpha FF; '#' or '0x' prefix allowed); blob: hex bytes ("0300ff...")
                      Values are passed unclamped (kernels clamp).
  --threads N         1 = all tiles serially on the main thread, in order (FXA_JOB = 0);
                      otherwise the worker pool (thr_parallel_for; pool size = the process' --threads)
  --tile N            tile edge, default 256.  FXF_BANDS: full-width bands of N rows;
                      FXF_WHOLE: one render call over the whole image
  --roi x0,y0,x1,y1   render only this rectangle (exclusive x1/y1, clamped); outside = source
                      (or --fill).  prepare gets the ROI in FXA_X0..Y1
  --sel FILE          selection mask (same size; red channel = coverage) -> TMASK in FXA_SEL (for
                      statistics); afterwards out = k_lerp_mask_row(src, result, mask)
  --fill AARRGGBB     initial destination instead of a copy of the source (two runs with different
                      fills must agree wherever render had to write)
  --bench N           N x (prepare + render); prints the fastest prepare and the fastest render
  --cancel            prepare + one render over the ROI with a JOB whose JOB_CANCEL = 1; prints
                      "err NAME code=C cancel prepare_us=P render_us=R" (expected C = -14)
  --show-params       print "params" + the 32 slots in hex before running
```
Output, one line per run on stdout:
```
ok NAME w=W h=H roi=X0,Y0,X1,Y1 tiles=T threads=N prepare_us=P render_us=R mpxs=M
err NAME code=C MESSAGE
```
(mpxs = ROI pixels / render microseconds.)  Checks after every run: guard bytes around every row of
source and destination (stride = w*4 + 16; the last row ends at a page end so reads past the
image fault) -> `code=-200`; source modified -> `-201`; bad arguments -202; unknown effect -203;
bad property -204.  Exit code 0 when every run printed `ok`, else 1 (2 = usage).

`--list` format (the harness parses it):
```
effect NAME id=ID cat=FXC flags=FXF nprops=N
  prop KEY type=int|dec|bool|choice|angle|pan|color|seed|blob slot=S dec=D flags=F min=.. max=.. def=..[,y] [choices=a|b|c]
```
(color: `def=AARRGGBB` hex; pan: `def=x,y`; blob: `max` = slot count.)

`--selftest` (also `apps/efftest/tests/selftest.uis`, run by `build.py test`): 97 x 61 hash image
with transparent pixels; known answers of invert_colors / black_white; for every effect with its
default properties: serial 256-tiles == pool 23-tiles (destination pre-filled with different
colours, so every pixel must be written), JOB_CANCEL -> ERR_CANCEL from prepare or render,
guard bytes.  `--selftest all` currently passes for all 54 registered effects (FXB/FXC included).

## 2. Contract additions (fx.inc, additive)
* `FP_BLOB = 9`: slots FXP_SLOT .. FXP_SLOT + n - 1 hold raw bytes, n = FXP_MAX (slot count, f64);
  default all zero (fx_defaults already does that); edited by a custom control (Curves).
* FP_COLOR defaults: FXPROP's `def` is a `.double`; for colours write the record by hand so the
  low 32 bits of FXP_DEF are the ARGB value (fx_adj.s has a local macro
  `FXA_PROPC slot, name, argb`; the integrator may promote it to fx.inc as `FXPROPC`).

## 3. tools/fx_check.py -- harness (use it for FXB / FXC)

```
python tools/fx_check.py [--target win|linux|both] [--out build/fxa] [--suite adjust,stylize,quantize]
                         [--only name,..] [--no-build] [--quick] [--bench] [--view DIR] [--keep]
```
`--bench`: 4000 x 3000, best of 3, single thread and pool.  `--view DIR`: contact sheets of every
effect (PNG, over a checkerboard) to inspect with an image viewer.

Importable:
```python
import sys; sys.path.insert(0, "tools"); import fx_check as fc
h = fc.Harness("win", fc.ROOT / "build/fxb")          # "linux" runs through WSL
img = fc.img_testsrc(320, 240)                        # uint8 (H, W, 4), bytes B, G, R, A
r = h.run1("gaussian_blur", img, {"radius": 5})       # Result: ok, code, out (array), mpxs, line
rs = h.runs([fc.Spec(fx, img, params, tile=64, threads=1, roi=..., sel=mask_img, fill=0x12345678,
                     bench=3, cancel=True, keep_out=False), ...])   # one efftest process
fails = fc.generic_checks(h, "gaussian_blur", img, {"radius": 5})   # [] = passed
rep = fc.Report("win"); rep.compare("name", r, expected, tol=1, frac=None)
```
`generic_checks(h, fx, img, params, rep=None, cancel=True, sel=True, tol_sel=0, slow=False,
roi_independent=None)`: serial/256 is the reference; pool/64, pool/whole, serial/37 and two
`--fill` runs must be bit-identical; an ROI run with `--fill` must match inside the ROI (skipped by
default for FXF_PREPARE effects whose statistics depend on the ROI; pass `roi_independent=True`
otherwise) and leave everything outside untouched (FXF_BANDS: rows, FXF_WHOLE: skipped); a
selection run must equal `lerp_mask(src, ref, m)` (FXF_PREPARE effects: only unselected pixels
checked); `--cancel` must give code -14 within 50 ms (200 ms with `slow=True`).
Images: `img_gradient`, `img_noise` (random alpha incl. 0), `img_shapes` (transparent background
with random colours), `img_allvalues` (every byte value per channel), `img_ffmpeg(src, w, h)`
(lavfi: testsrc2, mandelbrot, cellauto, smptebars, ...; alpha forced 255), `img_testsrc`,
`img_photo` (smooth mandelbrot), `with_alpha_hole(img)`.  Helpers: `write_raw/read_raw` (RSR1),
`write_png`, `contact_sheet`, `lerp_mask` (exact k_lerp_mask_row), `luma` (paint.net intensity),
`level_table`, `spline_table`, `apply_lut`.

## 4. Helper APIs (thread safe, no global state)

fx_lut.s
```
fx_rows(args, fn, ctx) -> eax 0 | ERR_CANCEL     for every ROI row (polls JOB_CANCEL):
                                                 fn(dst, src, n, ctx, x0, y) (Win64; dst/src = row
                                                 start at x0; x0/y are stack args 5/6)
fx_lut_row(dst, src, n, lut)                     lut = 4 x 256 bytes (tables for B, G, R, A)
fx_lut_ident(lut)                                identity tables (1024 bytes)
fx_luma(argb) -> eax                             (7471 B + 38470 G + 19595 R) >> 16
fx_luma_row(dst8, src, n)
fx_pow(x xmm0, y xmm1) -> xmm0                   x87, 64-bit precision; y == 1 -> x exactly, y == 0
                                                 -> 1, x <= 0 -> 0
fx_exp2 / fx_log2 / fx_cos / fx_sin (xmm0) -> xmm0
fx_srgb_dec(c) / fx_srgb_enc(l) -> xmm0          IEC 61966-2-1 transfer (enc clamps to 0..1)
fx_lin_table(f32 out[256])                       sRGB byte -> linear
fx_srgb_table(u8 out[n], n)                      linear i/(n-1) -> round(255 enc)
fx_levels_lut(lut1024, lvl)                      paint.net Level (see §5 Levels); LVL record:
                                                 u8 in_lo[4], in_hi[4], out_lo[4], out_hi[4] (B,G,R,-),
                                                 f32 gamma[3] (B, G, R)
fx_spline_lut(lut256, pts, n)                    paint.net SplineInterpolator (natural cubic spline,
                                                 NR 3.3, ClampToByte = truncation); pts = n x {u8 x,
                                                 u8 y}, any order, repeated x: last wins, n <= 64;
                                                 n = 0 identity, n = 1 constant
```
fx_hist.s
```
fx_hist_rgba(args, hist u32[4][256]) -> rax count | ERR_CANCEL   B,G,R,A histograms of the ROI
                                                 (alpha > 0 and, with FXA_SEL, coverage > 0)
fx_hist_percentile(h, total, fraction xmm2) -> eax    smallest j with cum > total * fraction
fx_hist_mean(h) -> xmm0                          f32(sum j h) / f32(sum h)
fx_lochist(args, rad, fn, ctx) -> eax 0 | ERR_CANCEL | ERR_NOMEM
     paint.net LocalHistogramEffect.RenderRect: per ROI pixel the histograms of the pixels in the
     circle u^2 + v^2 <= ((2 rad + 1)^2 + 2) / 4 clipped to the image; fn(ctx, hist, area, px)
     -> eax output pixel; hist = u32[4][256] (B, G, R, A) followed by u32[4][16] block sums
     (bins 16k..16k+15) at hist + 4096.  Starts fresh at each ROI row's left edge and slides, so
     results do not depend on the ROI split.  Useful for Median / Reduce Noise / Oil Painting.
```

## 5. Effects -- exact semantics
Pixels straight ARGB.  I = paint.net intensity (7471 B + 38470 G + 19595 R) >> 16 (weights 0.114,
0.587, 0.299 in 16.16).  Adjustments keep alpha (except Invert Alpha) and transform the colour of
transparent pixels like every other pixel (as paint.net).  Parameters are clamped by the kernels.
All are tile / thread independent (checked bit-exact).  "exact" = bit-identical to the numpy
reference in fx_check.py; "±1" = float-LUT effects (x87 pow vs Python pow) allowed to differ by 1
on <= 0.1 % of values (in practice identical).

Adjustments (FXC_ADJUST, all FXF_ADJ):
| effect | props (slot: key type range default) | formula |
|---|---|---|
| auto_level (PREPARE, NO_DIALOG) | – | prepare: fx_hist_rgba over ROI (alpha > 0, sel > 0); per channel lo = percentile(0.005f), hi = percentile(0.995f), md = (byte)(f32 mean + 0.5f); gamma = f32(clamp(ln 0.5 / ln(f32(md-lo)/f32(hi-lo)), 0.1, 10)) if lo < md < hi else 1; Level(lo, hi, gamma, 0, 255) LUT (paint.net HistogramRgb.MakeLevelsAuto / AutoFromLoMdHi). ±1 |
| black_white (NO_DIALOG) | – | B = G = R = I. exact |
| brightness_contrast | 0 brightness int -100..100 0; 1 contrast int -100..100 0 | paint.net 3.x: contrast < 0: mul = c+100, div = 100; > 0: mul = 100, div = 100-c; 0: 1/1. div == 0: c = (I + b < 128) ? 0 : 255 for B, G, R; div == 100: shift = (I-127)*mul/div + 127 - I + b; else shift = (I-127+b)*mul/div + 127 - I (C# truncating division); c = clamp(c + shift[I]). exact |
| curves (PREPARE) | 0 mode choice luminosity/rgb 0; 1 luminosity blob(5); 6 red blob(5); 11 green blob(5); 16 blue blob(5) | blob = u8 n (<= 19; 0 = identity), n x {u8 x, u8 y}; tables = fx_spline_lut. Luminosity: c = clamp(c + curve[I] - I) (paint.net LuminosityCurve); RGB: per channel table. exact |
| exposure (PREPARE) | 0 exposure dec -5..5 0 (2 dec) | LUT: round(255 enc(dec(j/255) * 2^ev)), same for B, G, R. ±1 |
| highlights_shadows (PREPARE) | 0 shadows int -100..100 0; 1 highlights int -100..100 0; 2 radius int 1..200 30 | own design (paint.net 5.1's algorithm is not public): grid cell s = max(1, radius div 4); cell = sum(a I) / (255 sum a) (0.5 if empty); separable Gaussian sigma = radius/(2s) cells, kernel radius min(ceil(3 sigma), 40), clamp-to-edge, f32 in tap order; per pixel m = bilinear grid value (vertical first) at ((x+0.5)/s-0.5, (y+0.5)/s-0.5); gain = 2^(2 S/100 (1-m)^2 - 2 H/100 m^2) quantised to 1025 steps; c = ENC[round(min(lin(c) gain, 1) 4096)] via a (gain, byte) table. Positive shadows brighten dark areas, positive highlights darken bright areas (Photoshop sense). exact vs the f32 mirror |
| hue_saturation | 0 hue int -180..180 0; 1 saturation int 0..200 100; 2 lightness int -100..100 0 | paint.net HueSaturationLightness: sf = sat*1024/100; c = clamp((I*1024 + (c-I)*sf) >> 10); HSV hue rotation with exact integer arithmetic (see below); lightness: blend with white (a = l*255/100) or black (a = -l*255/100): c = (c (255-a) + K a) / 256 (paint.net BlendConstant, integer). Identity params = copy. exact |
| invert_alpha (NO_DIALOG) | – | A = 255 - A. exact |
| invert_colors (NO_DIALOG) | – | B, G, R = 255 - c. exact |
| levels (PREPARE) | 0 input_black color FF000000; 1 input_white FFFFFFFF; 2 output_black FF000000; 3 output_white FFFFFFFF; 4-6 gamma_red/green/blue dec 0.1..10 1; 7-9 red/green/blue bool 1 | per channel (B, G, R bytes of the colours): v = f32(j) - in_lo; v < 0 -> out_lo; j >= in_hi -> out_hi; else trunc(clamp(out_lo + (out_hi - out_lo) pow(f64(f32(v / f32(in_hi - in_lo))), f64(f32 gamma)), 0, 255)) (paint.net UnaryPixelOps.Level). in_hi <= in_lo: identity; unchecked channels: identity. exact |
| posterize | 0 red int 2..64 16; 1 green; 2 blue; 3 linked bool 1 | paint.net PosterizePixel.CalcLevels: t1[i] = 255 i/(L-1); walk j with k += L, k > 255 -> k -= 255, j++. linked: green/blue use red's value. exact |
| sepia (NO_DIALOG) | – | Desaturate, then Level(gamma B 1.2f, G 1.0, R 0.8f): {sepB[I], I, sepR[I]} (tables generated with paint.net's f32/f64 steps). exact |
| temperature_tint (PREPARE) | 0 temperature int -100..100 0; 1 tint int -100..100 0 | own design: t, m = value/100; log2 gains R = 0.5t + 0.15m, G = -0.3m, B = -0.5t + 0.15m, divided by their luminance 0.2126 R + 0.7152 G + 0.0722 B (grey keeps its luminance); per channel LUT round(255 enc(dec(j/255) g_c)). ±1 |

Hue rotation (exact, integer values carried in f32 lanes): mx/mn/d = max/min/max-min of the
(saturated) B, G, R; R max: base 0, num = G - B; else G max: base 2, num = B - R; else base 4,
num = R - G; D = 60 (d base + num) + hue d; for R, G, B with n = 5, 3, 1:
K = (D + 60 n d) mod 360 d, X = clamp(min(K, 240 d - K), 0, 60 d), c = mx - floor((X + 29) / 60)
(= round-half-up of the continuous HSV result mx - d t).

Quantize (FXC_COLOR, FXF_PREPARE | FXF_SLOW): 0 colors int 2..256 16; 1 algorithm choice
octree/median_cut 0; 2 dithering int 0..8 7; 3 alpha choice keep/binary 0.  Everything happens in
prepare over the ROI (the result image is the aux; render copies rows), because Floyd-Steinberg is
sequential -- tiles/threads cannot change the result.  Considered pixels: in ROI, selection > 0,
alpha > 0 (keep) / >= 128 (binary).  Palette: <= K distinct colours -> exactly those; else
octree = Gervautz-Purgathofer (8 levels, child bit order R, G, B; after each pixel, while leaves > K
the most recently created internal node of the deepest level with internal nodes absorbs its leaf
children; palette = leaves depth-first, colour = round(sum/count); memory bounded by ~9K nodes);
median_cut = Heckbert on a 32^3 histogram: split the box with most pixels (first on ties) spanning
> 1 cell along its longest cell extent (ties R, G, B) after the first slice with 2 cum >= count,
shrink boxes to occupied cells, colour = rounded mean.  Mapping: exact nearest palette colour
(squared RGB distance, lowest index on ties; lazily built candidate lists per 16^3 cell); dithering
L > 0: Floyd-Steinberg left to right, 7/3/5/1 sixteenths, v = clamp(c + ((E L + 64) >> 7)); only
"opaque" pixels take/pass error.  Alpha: keep = unchanged; binary = 255 if >= 128 else 0 (colour
still mapped).  exact vs the Python reference.

Stylize (FXC_STYLIZE, no prepare; paint.net 3.x ColorDifferenceEffect weights, r = angle 2 pi / 360,
dr = pi/4: row 0 cos(r+dr) cos(r+2dr) cos(r+3dr); row 1 cos(r) CENTER cos(r+4dr); row 2 cos(r-dr)
cos(r-2dr) cos(r-3dr); quantised wq = floor(w 16384 + 0.5); taps outside the image skipped; v =
trunc(sum wq c / 16384) toward zero + OFFSET, clamped; alpha = centre alpha):
| effect | props | kernel |
|---|---|---|
| edge_detect | 0 angle angle -180..180 45 | CENTER 0, OFFSET 128 on B, G, R (grey relief with coloured edges, paint.net 4+ look) |
| emboss | 0 angle angle -180..180 0 | CENTER 0, OFFSET 128 on I, output grey (paint.net EmbossEffect) |
| relief | 0 angle angle -180..180 45 | CENTER 1, OFFSET 0 on B, G, R |
| outline (FXF_SLOW) | 0 thickness int 1..200 3; 1 intensity int 0..100 50 | paint.net OutlineEffect on fx_lochist(radius = thickness): mc1 = area (100-i)/200, mc2 = area (100+i)/200; per channel skip empty bins, count to mc1 -> c1, on to mc2 -> c2 (bins < 255), c = 255 - (c2 - c1); alpha = a2 whose skip loop uses the BLUE histogram (paint.net bug kept) |
All stylize results exact vs the numpy references.

## 6. Deviations from paint.net (documented choices)
* Hue/Saturation: no int-percent HSV quantisation (paint.net 3.x HsvColor had int S/V -> banding);
  integer-exact rotation with round-half-up instead of truncation.
* Stylize: weights in 1/16384 fixed point (paint.net: double; differs only when a sum is within
  0.03 of an integer); alpha = centre alpha (3.x wrote 255); Edge Detect adds 128 (paint.net 4+ look;
  3.36/Pinta had no offset).
* Auto-Level histogram skips fully transparent pixels and uses the selection mask (> 0) instead of
  the selection's bounding box; percentiles compare in f64.
* Levels: out_hi < out_lo allowed (inverts); invalid in range -> identity for that channel;
  the R/G/B checkboxes are a kernel channel mask.
* Posterize: `linked` makes green/blue follow red in the kernel.
* Exposure, Highlights/Shadows, Temperature/Tint: own designs (paint.net 5.1 sources unknown).
* Quantize: nearest-colour mapping instead of octree descent; octree reduces on the fly
  (Gervautz-Purgathofer) instead of after all pixels; median cut on 5-bit cells.

## 7. Tests and performance
`python tools/fx_check.py --target both` (efftest built by the script; `--no-build` to skip):
1586 checks per target, all passing on Windows and Linux (adjust 967, stylize 305, quantize 314 incl.
generic checks).  `efftest --selftest` via `build.py test`.  asmcheck: 0 errors in fx_adj, fx_color,
fx_stylize, fx_lut, fx_hist, efftest (2 annotated false positives; 54 unprovable BOUNDS warnings =
loop indices bounded by counts the analysis cannot see).

4000 x 3000, render Mpx/s single thread (Windows / Linux) -- pool 12 threads (Ryzen 5 3600):
invert_colors 1063/1221, invert_alpha 1371/756, black_white 517/430, sepia 407/405,
brightness_contrast 207/199, posterize 482/412, hue_saturation 121/117, levels 481/519,
auto_level 471/548 (+28 ms prepare), curves luminosity 209/204, rgb 611/420, exposure 628/549,
temperature_tint 619/467, highlights_shadows 195/133 (+30-42 ms prepare) -- all >= 100 Mpx/s;
pool 550-2000 Mpx/s.  Effects: edge_detect 122/93, emboss 79/69, relief 121/78, outline 4/5
(pool 35/27; paint.net's algorithm, block-sum accelerated), quantize render 400+ (copy) with
prepare 290-320 ms (no dither) / 570-600 ms (dither 8) for 12 Mpx.

## 8. Open issues / notes for the integrator
* `build.py app efftest` fails while another agent's library file is mid-edit; FXA
  used a private wrapper that substitutes the last good object (no foreign sources touched).
* Default values / ranges for Exposure, Highlights/Shadows, Temperature/Tint, Quantize are my
  choices; calibrate against real paint.net 5.1 if available.
* Quantize prepare is single-threaded (sequential Floyd-Steinberg); for live preview of huge
  images the app may want a smaller preview ROI.  Octree mapping cache could make it ~2x faster.
* Outline with thickness 200 is slow (O(r) histogram updates per pixel + O(r^2) per row start),
  as in paint.net.
