# CE — WebP encoding (VP8L lossless, VP8 lossy + ALPH, RIFF container)

Doc fragment for `ASMLIB/docs/DESIGN.md` §4 / `CODECS.md` (integrator merges it next to
CODECS_WEBP.md, whose "encoders are the next package" lines are now obsolete). Everything
below is implemented, assembles on both targets, `asmcheck` 0 errors, and is covered by
`tools/webp_enc_check.py` (all suites pass on Windows and Linux, byte-identical output).

## 1. Files

| File | Content |
|---|---|
| `ASMLIB/src/img/webp.s` (encoder part) | `webp_encode` (container, metadata, dispatch), `webp_enc_alpha` (ALPH payload), `wa_filter` / `wa_estimate`; `webp_codec` now `IFC_DECODE \| IFC_ENCODE` + IC_IOM |
| `ASMLIB/src/img/webp_vp8l_enc.s` | lossless: `vp8l_encode`, `vp8l_enc_argb`, shared `wpe_load_argb` / `wpe_cfg_from_opt` |
| `ASMLIB/src/img/webp_vp8_enc.s` | lossy: `vp8_encode`, `vp8_enc_argb` |
| `ASMLIB/inc/img.inc` (CE block at the end) | `IO_WEBPPRESET`, `IOM_PRESET`, `WPP_*`, `IOF_WEBP_*`, WEBPENC record `WE_*`, `WEF_*`, test bits `WEF_T_*` |
| `apps/codectest/src/webp_ops.s` (CE section at the end) | ops 126..129 (`wce_op`) |
| `tools/webp_enc_check.py` | checker (libwebp 1.3 in WSL decodes / encodes for comparison) |

Decoder files: the old `vp8l_encode` / `vp8_encode` stubs were removed from `webp_vp8l.s` /
`webp_vp8.s` (the encoder owns these names) and `.globl` was added to `vl_code_to_plane`,
`v8_bmodes`, `v8_dctab`, `v8_actab`, `v8_proba0`, `v8_update`, `v8_zigzag`, `v8_p4tab`
(nothing else changed; `webp_check.py` still passes).

## 2. API

```
webp_encode(sf, opt, &buf, &len) -> eax 0 / ERR_*      IMGCODEC encode (img_encode / img_save_opt)
vp8l_encode(sf, opt, &buf, &len) -> eax                "VP8L" chunk payload (5-byte header included)
vp8_encode(sf, opt, &buf, &len) -> eax                 "VP8 " chunk payload (alpha not coded)
vp8l_enc_argb(argb, w, h, cfg, &buf, &len) -> eax      u32 BGRA pixels -> VP8L (WEF_NOHDR: ALPH stream)
vp8_enc_argb(argb, w, h, cfg, &buf, &len) -> eax       u32 BGRA pixels -> VP8 key frame
webp_enc_alpha(px, w, h, cfg, &buf, &len) -> eax       alpha of px -> ALPH chunk payload
wpe_load_argb(sf, &wh) -> rax pixels / 0 (edx error)   img_get_row(FMT_ARGB) of any surface, 1..16383
wpe_cfg_from_opt(cfg, opt)                             WEBPENC from an IMGOPT
```
WEBPENC (`WE_*`, 24 bytes): effort (0 = 5), quality (0 = 95), flags (`WEF_NOEXACT`,
`WEF_NOHDR`, `WEF_1THREAD`, `WEF_SHARPYUV`), preset, cancel pointer. Sizes 1..16383 per
side (WebP limit; 16384+ → ERR_DIMS). All buffers mem_alloc'd, nothing global, thread-safe.
Errors: ERR_DIMS, ERR_NOMEM, ERR_CANCEL (polled per tile / 64 K positions / MB row),
ERR_LIMIT (file > 4 GB, VP8 partition 0 ≥ 512 KB after the retries).

## 3. IMGOPT / save options

| Field | WebP meaning |
|---|---|
| `IOF_LOSSLESS` | VP8L instead of VP8 |
| `IO_QUALITY` 1..100 (0 = 95) | lossy quality (libwebp curve); ignored for lossless |
| `IO_EFFORT` 1..9 (0 = 5) | lossless search effort (5 ≈ `cwebp -z 6`); lossy method 0,1,2,3,4,5,5,6,6 |
| `IO_WEBPPRESET` (offset 36, the IO_DDSFMT slot) | `WPP_DEFAULT` / `PICTURE` / `PHOTO` / `DRAWING` / `ICON` / `TEXT` = libwebp presets (sns, filter strength, sharpness, segments) |
| `IOF_EMBED_ICC / EXIF / XMP` + `IO_META` | ICCP / EXIF (orientation reset to 1, no "Exif\0\0") / XMP chunks in VP8X |
| `IOF_WEBP_NOEXACT` (0x20000) | RGB under alpha 0 may change: lossless sets it to 0, lossy flattens / smooths transparent 8x8 blocks (libwebp `exact = 0`; much smaller files with big transparent areas). Default: exact |
| `IOF_WEBP_SHARPYUV` (0x40000) | accepted, not implemented (standard conversion) |
| `IOF_WEBP_1THREAD` (0x80000) | accepted; the encoder is single-threaded anyway |
| `IO_CANCEL` | polled |

`webp_codec` IC_IOM = `IOM_QUALITY | IOM_LOSSLESS | IOM_EFFORT | IOM_ICC | IOM_EXIF | IOM_XMP |
IOM_PRESET` (new bit `IOM_PRESET = 1 << 19`).

**paint.net 5.1 WebP save dialog** (German resources `WebPFileType.*`): "Qualität" (slider) →
`IO_QUALITY`; "Verlustfrei" (checkbox) → `IOF_LOSSLESS`; "Preset" with "Standard / Bild /
Foto / Zeichnen / Icon / Text" → `IO_WEBPPRESET` 0..5 (same order as libwebp's WebPPreset);
"Weitere Informationen" is a forum link only. paint.net shows no effort / metadata / exact
controls for WebP: use `IO_EFFORT` 0, metadata flags as for the other formats.

## 4. Container (webp.s)

Simple `RIFF/WEBP/VP8L` or `VP8 ` file unless metadata is embedded or a lossy image has alpha;
then `VP8X` (flags ICC 0x20, alpha 0x10, EXIF 0x08, XMP 0x04; canvas − 1 in 24 bits) followed by
ICCP, ALPH, VP8 / VP8L, EXIF, XMP (spec order). Chunks padded to even sizes, RIFF size = file −
8. Lossless with alpha in a VP8X file sets the alpha flag too.

ALPH (lossy + alpha < 255 somewhere): header byte `method 1 | filter << 2` (no
pre-processing), then a header-less VP8L stream of the filtered alpha plane (green channel,
`vp8l_enc_argb` with WEF_NOHDR). Filter (none / horizontal / vertical / gradient with libwebp's
row 0 / column 0 rules): the one with the smallest residual entropy estimate; effort ≥ 8
encodes all four and keeps the smallest.

## 5. Lossless (webp_vp8l_enc.s)

Modelled on libwebp 1.3 `vp8l_enc.c` / `backward_references*.c` / `histogram_enc.c` /
`predictor_enc.c`:

- **Analysis**: alpha use, palette (≤ 256 colours, sorted, libwebp PaletteSortMinimizeDeltas),
  libwebp `AnalyzeEntropy` (direct / spatial / subtract green / spatial+SG / palette, transform
  overheads), small palettes (≤ 16) always palette; red/blue-always-zero skips cross colour.
  Effort e → libwebp method `min(6, e − 1)` and quality 0/20/30/50/75/80/90/95/100 (hash chain
  iterations `8 + q²/128`, window by quality, colour cache up to 10 bits from q > 25), histogram
  / transform tile bits as `GetHistoBits` / `GetTransformBits`. Effort 7–8 also encodes the
  runner-up configuration, 9 every applicable one; the smallest stream wins.
- **Transforms**: subtract green; predictor (all 14 modes per tile, libwebp
  `PredictionCostSpatialHistogram` with accumulated histograms and the ±15 neighbour bias);
  cross colour (libwebp's green-to-red / green-red-to-blue searches, `CombinedShannonEntropy`
  + `PredictionCostSpatial` + the −3 bonuses, accumulated histograms skipping repeats); colour
  indexing with 1/2/4/8-bit bundling, delta-coded palette sub-image. `WEF_NOEXACT`: transparent
  pixels become 0.
- **Entropy-coded image** (main and every sub-image): libwebp `VP8LHashChainFill` (2-pixel
  hash, runs hashed with their remaining length, from the end with left extension, above /
  previous pixel first, `length_max` 256); greedy LZ77 with lookahead and RLE parses, each
  with its own colour cache size, the cheaper estimate (exact Huffman bits + extra bits) wins;
  an LZ77 winner is refined by a shortest-path parse with the cache-aware cost model
  (`TraceBackwards` equivalent: every length up to 48 + the full length, lengths ≥ 3), kept
  when cheaper (effort ≥ 4; a second round from 7). Colour cache bits: all sizes 0..10
  simulated on the parse, exact Huffman cost.
- **Meta prefix codes** (effort ≥ 4, main image): tile histograms, libwebp entropy bins (64)
  with the combine-cost factor, greedy pair merging while it saves bits (≤ 64 clusters), then
  remapping of every tile to its cheapest group (effort ≥ 5), empty groups dropped.
- **Huffman**: libwebp `GenerateOptimalTree` (count_min doubling, 15 / 7 bits), canonical
  codes, code-length RLE (16/17/18) and trimming exactly like `StoreFullHuffmanCode`, simple
  codes for ≤ 2 symbols < 256, 0-bit codes for single symbols.

## 6. Lossy (webp_vp8_enc.s)

libwebp method 4 (`RD_OPT_BASIC`, token loop, one pass):
RGB → YUV 4:2:0 with libwebp's rounding and gamma-compressed (0.8) chroma averaging, alpha
weighted for partly transparent 2x2 blocks; planes padded to macroblocks; `WEF_NOEXACT` +
alpha: `WebPCleanupTransparentArea`. Analysis per macroblock (DCT histogram "alpha" of the best
I16 and chroma predictions, source context), k-means (6 rounds) into the preset's segments,
SetSegmentAlphas; `VP8SetSegmentParams` (QualityToCompression, SNS modulation, dq_uv),
filter strengths (`SetupFilterStrength`; `kLevelsFromDelta` approximated as "smallest level
with 2·level + interior ≥ 3·delta", exact for sharpness 0), SimplifySegments, segment map
probabilities. Matrices: kBiasMatrices, zero thresholds, kFreqSharpening, lambdas.
Mode decision per MB: PickBestIntra16 (4 modes, SSE + tlambda·TDisto, flatness rule),
PickBestIntra4 (10 modes per block, flatness penalty, early exit, I4 header-bit budget),
PickBestUV (UV DC error diffusion for q ≤ 98); residual costs from level-cost tables rebuilt
from the token statistics every max(MBs / 8, 96) macroblocks. Reconstruction through the
decoder's `vp8_itx` / `vp8_wht` (bit-exact with what decoders compute). Final probabilities
(`FinalizeTokenProbas`), StoreMaxDelta / AdjustFilterStrength, one token partition, no skip
flag; a pass is repeated with half the I4 header budget while partition 0 would exceed
512 KB. Bool encoder = libwebp `VP8BitWriter`.

## 7. Results (`python tools/webp_enc_check.py --target both --out build/ce`)

All suites pass on both targets; Windows and Linux files are byte-identical.

| Suite | Records | Content |
|---|---|---|
| lossless | 472 | 236 encodes: noise / gradients / palettes 1..257 colours / ffmpeg sources / photos, all alpha kinds, 1x1 … 16383x1 / 1x16383, efforts 1..9, NOEXACT; libwebp and our decoder exact |
| lossy | 171 | qualities 1..100, efforts 1..9, presets 0..5, alpha (ALPH exact), 16383x1; our decoder == libwebp on every file |
| alpha | 60 | ALPH payloads (op 128) decoded by `webp_alpha` (op 123) |
| container | 194 | chunk order / flags / canvas / padding / RIFF size, ICC / EXIF (orientation 1) / XMP read back, caps / IOM |
| errors | 6 | 16384 → ERR_DIMS, cancel → ERR_CANCEL (ops 126..128) |
| size | 19 | see below |
| det | 13 | repeated runs and 1THREAD identical, Windows == Linux |
| perf | 4 | 12 MP timings |

**Lossless vs libwebp `-z 6`** (method 4, quality 75, exact; 512² photo crops, 320x240 ffmpeg
sources, palette images): total size **1.003×**, geometric mean **1.000×** (photos
1.001–1.005, testsrc2 1.022, smptehdbars 1.033, mandelbrot 1.013, palettes 0.978–1.004,
rgbtestsrc 0.944). 1920x1200 photo 1.011×, 4000x3000 (upscaled) photo 1.038×.

**Lossy vs libwebp** (same quality, method 4): photos within −0.16 dB PSNR and ≤ 1.02× size
(total over all images 0.994× size); synthetic outliers: smptehdbars q90 1.053× size, rgbtestsrc
q75 1.094× (files < 1.4 KB), random black/white `cellauto` up to −1 dB at equal quality (the
analysis alphas, hence segment quantisers, differ slightly from libwebp).

**Speed** (12 MP photo, single thread; libwebp 1.3 in WSL for comparison):

| | Windows | Linux (WSL) | libwebp |
|---|---|---|---|
| lossless effort 5 (≈ -z 6) | 9.2–11.1 s, 8.60 MB | 10.2 s | 12.6 s, 8.28 MB |
| lossless effort 1 | 2.2–2.8 s, 9.32 MB | 2.7 s | 3.2 s (-z 1), 8.64 MB |
| lossy q90 effort 5 | 2.6–3.0 s, 808 KB | 3.2 s | 1.4 s, 838 KB |
| lossy q90 effort 1 | 1.2–1.9 s | 1.4 s | – |

## 8. codectest ops (webp_ops.s, CE section)

| Op | Function | Data → result |
|---|---|---|
| 126 | `vp8l_enc_argb` | {w, h, effort, flags} + u32 pixels → stream (flags bit 31: cancel set) |
| 127 | `vp8_enc_argb` | {w, h, effort, flags, quality, preset} + pixels → stream |
| 128 | `webp_enc_alpha` | {w, h, effort, flags} + pixels → ALPH payload |
| 129 | time `img_encode` | IMGOPT + surface, arg = repetitions → {µs per call, size} |

Test-only WE_FLAGS bits (`WEF_T_*`): forced configuration / predictor mode / cache bits, no
cross colour / cache / meta / shortest path / greedy merging / RLE, keep the shortest path
always, report (cache bits / groups / cache estimates), dump the match array.

## 9. Memory safety

asmcheck on webp*.s + webp_ops.s: 0 errors (45 BOUNDS warnings = not provable indices into
tables / per-block buffers; annotations document the invariants). Review points: all pixel
buffers carry ≥ 64 bytes of slack for the 16-byte SIMD loads; match lengths are capped to the
remaining pixels (`n − 1 − pos`), distances to the window and to `pos`; the hash chains only
contain earlier positions; bit writers check capacity before every store (VP8L: growth before
the 4-byte flush, final 8-byte store checked; VP8: room for the pending 0xff run + 1); level
store grows before each macroblock (worst case 25 × 33 bytes); Huffman tables sized for the
largest alphabet (green 1304); per-tile / per-group arrays from the tile counts (≤ 2600 tiles,
≤ 64 groups). Bugs found by the tests and fixed: a stack slot overlap in the match search
(found far fewer matches), the palette lookup's "last colour" sentinel colliding with
0xFFFFFFFF, a clobbered cfg pointer in `webp_enc_alpha`, a missing term of
CombinedShannonEntropy.

## 10. Open points

- Threads are not used (the analysis and RGB→YUV could run on `thr_parallel_for`; the output
  would stay identical); lossy encoding is ~2× slower than libwebp's SIMD code.
- Sharp YUV (`IOF_WEBP_SHARPYUV`), trellis quantisation (methods 5/6), near-lossless,
  alpha level reduction (alpha_quality < 100), autofilter, multiple token partitions and
  libwebp's palette+predictor / box-LZ77 variants are not implemented.
- Animation frames are not written (stills only).
- Integrator: merge §2–§6 into DESIGN.md / CODECS.md, drop the "encoders later" lines of
  CODECS_WEBP.md, regenerate API.md (`python tools/gendoc.py`).
