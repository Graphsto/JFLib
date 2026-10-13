# CB — BC1..BC7 encoders and the DDS writer (phase P2)

Doc fragment for `ASMLIB/docs/DESIGN.md` §4 / §7 and `API.md` (to be merged by the integrator).
Everything below is implemented, assembled on both targets and tested
(`tools/bc_check.py`, `tools/dds_write_check.py`; CD's `tools/dds_check.py` still passes).

## 1. Files

| File | Content |
|---|---|
| `ASMLIB/src/img/bc_enc.s` | public API (`bc_encode_block`, `bc_encode_to`, `bc_encode`, `bc_encoded_size`), surface driver (block rows on the thread pool), BC1 / BC2 / BC3 / BC4 / BC5 (+ signed) encoders, float "pixel set" helpers `bce_*` shared with BC6H / BC7 |
| `ASMLIB/src/img/bc7_enc.s` (new) | BC7 encoder (`bc7_enc`), partition estimator / bit writer also used by BC6H (`b7_moments`, `b7_est`, `b7_top`, `b7_submask`, `b7_put`) |
| `ASMLIB/src/img/bc6h_enc.s` (new) | BC6H UF16 / SF16 encoder (`bc6h_enc`, `bc6hs_enc`), all 14 modes |
| `ASMLIB/src/img/dds.s` | CD's reader unchanged; the writer section (`dds_encode`, header / level / row / resize helpers, format tables); `dds_codec` now IFC_DECODE \| IFC_ENCODE with IOM_DDSFMT \| MIPMAPS \| MIPFILTER \| EFFORT \| SRGB \| CUBE |
| `ASMLIB/src/img/bc_dec.s` (CD) | additive only: `.globl bc6_modes, bc6_layout` (BC6H header layout for the encoder) |
| `ASMLIB/inc/img.inc` | CB block at the end: `BCQ_*`, `BCE_SCRATCH`, `DDSF_BC4S` / `BC5S` / `BC6HS` / `R8` / `BGR8` / `DDSF_COUNT`, `MIPF_FANT` / `MIPF_LANCZOS`, `IOF_DDS_UNIFORM` |
| `apps/codectest/src/dds_ops.s` | CD's ops 100..106 unchanged, new ops 107..112 |
| `tools/bc_check.py`, `tools/dds_write_check.py` | checkers (below); `bc_check.py --tables` prints the generated tables |

## 2. API (bc_enc.s)

```
bc_encode_block(bcfmt, px, dst, cfg) -> eax 0 / ERR_UNSUPPORTED / ERR_NOMEM
      one 4x4 block: px = 16 straight BGRA pixels (64 bytes, row-major) -> 8 / 16 bytes
bc_encode_to(sf, bcfmt, cfg, dst, cancel) -> eax 0 / ERR_DIMS / ERR_UNSUPPORTED / ERR_NOMEM / ERR_CANCEL
      whole surface into dst (bc_encoded_size bytes, blocks in raster order); cancel = u32* (arg 5, 0 = none)
bc_encode(sf, bcfmt, cfg, &buf, &len) -> eax 0 / ERR_*      the same into a new mem_alloc'd buffer (&len = arg 5)
bc_encoded_size(w, h, bcfmt) -> rax                         ceil(w/4) * ceil(h/4) * 8 / 16 (0 = unknown format)
```
* `bcfmt` = `BCF_*` (CD): BC1..BC7 = 1..7 (BC4 / BC5 unsigned, BC6H UF16), BC4S 8, BC5S 9, BC6HS 10.
* `cfg` = `BCQ_*`: level `BCQ_FAST` 1 / `BCQ_NORMAL` 2 / `BCQ_SLOW` 3 (0 = normal); `BCQ_UNIFORM` (BC1..BC3
  colour: uniform RGB error; default = DirectXTex's perceptual scaling R 0.2125/0.7154, G 1, B 0.0721/0.7154);
  bits 8..16 = BC1 alpha threshold + 1 (0 = 128; 1 = threshold 0 = no transparency).
* Surfaces: any format `img_get_row` reads (XRGB / PARGB / ARGB / A8) → straight BGRA rows; partial edge
  blocks repeat the last column / row (tested: identical to encoding the explicitly padded image).
* Threads: `thr_parallel_for` over block rows (one heap scratch of 16 w + `BCE_SCRATCH` bytes per row);
  every block depends only on its pixels → output identical for any thread count and on both OSes
  (SSE2 single precision in a fixed order, no rcpps / rsqrtps; verified by hashing every stream).
  Cancel flag polled once per block row (`thr_job_cancel` → ERR_CANCEL); a failed row allocation → ERR_NOMEM.
* No global state; frames < 4 KB (the largest block context, BC6H, is 11 KB on the heap).

## 3. Encoders

Common machinery (`bce_*`): a "pixel set" (≤ 16 pixels × 4 float channels, SoA + AoS, lane mask, block
positions); principal axis = covariance + 8 power iterations (start: the column with the largest variance);
exact decoder palettes; nearest entry per pixel (squared error, ties → lowest index) with SSE2 over 4
lanes; least-squares endpoints for the chosen weights. Unused channels are zero in pixels and palettes.
Errors are sums of squared integer differences (exact in single precision).

* **BC1 / BC2 / BC3 colour.** Alpha < threshold → transparent (BC1 only, 3-colour mode, index 3; the
  fit ignores those pixels). One colour → exact tables (`bc1_s4_*` 4-colour 2/3-mix, `bc1_s3_*` 3-colour
  1/2-mix; max error 1 / 2 per channel). Otherwise principal axis → 565 → palette (bc_dec.s' exact
  rounding = WIC) → least squares (fast 1, normal 2, slow 4 passes) → for BC1 also the 3-colour mode
  (normal / slow; used when better) → greedy ±1 search on the six endpoint fields (normal 1 round, slow
  8). 4-colour blocks are written with c0 > c1 (c0 == c1: all indices 0), 3-colour with c0 ≤ c1 — also
  for BC2 / BC3, so decoders that honour the BC1 ordering rule in BC2 / BC3 agree.
* **BC2 alpha** round(a / 17). **BC3 alpha, BC4, BC5** (`bc4_chan`): candidates (max, min) in the 8-value
  mode and (min', max') without 0 / 255 in the 6-value mode; normal: least squares + greedy ±1 rounds;
  slow: additionally ±40 grids (step 8) around both candidates with ±4 windows around each grid's best,
  window + least squares until no gain, 16 greedy rounds (optimal = exhaustive 65536-pair search on every
  test block). Palettes exactly bc_dec.s' formulas; SSE2 byte search. BC4 stores paint.net's intensity
  (7471 B + 38470 G + 19595 R) >> 16, BC5 red / green. Signed: codes −127..127, byte x ↔ code
  round(254 x / 255) − 127, so the decoder's round(255 (v + 1) / 2) returns the byte.
* **BC7** (DirectXTex's speeds): fast = mode 6 only (BC7_QUICK); normal = modes 1, 3, 6 for opaque blocks,
  4 / 5 (rotation 0, both index selections), 6, 7 for blocks with alpha (DirectXTex default set; rotations
  of 4 / 5 gained < 0.01 dB on photos); slow = all 8 modes, all rotations / index selections, every p-bit
  combination. Partitioned modes: every partition estimated from per-pixel moment sums —
  n (trace − λ + λ / (levels − 1)²) per subset, λ = largest covariance eigenvalue (power iteration +
  Rayleigh quotient) — and the best K (normal 4, slow 16) fitted exactly. Endpoints: nearest codes from
  tables `b7_qt` (per effective bit depth, with / without p-bit), unique p-bits per endpoint / shared per
  subset by endpoint error (slow: all combinations evaluated), fast 1 / normal 2 / slow 3 least-squares
  passes. Anchor rule by swapping a subset's endpoints and mirroring its indices. One-colour blocks: mode 5
  with table endpoints (index 1, exact for every byte) and exact alpha. Prototype (Python) used to choose
  K: K = 1 / 4 / 8 → 43.42 / 43.73 / 43.76 dB on a hard crop.
* **BC6H** (UF16 / SF16 from 8-bit input, v / 255 as IEEE half): fitted in the decoder's interpolation
  domain X = h · 64 / 31 (UF16) or h · 32 / 31 (SF16). All 14 modes writable through bc_dec.s' layout
  tables: 1-region modes 11..14 always, 2-region modes 1..10 from normal on for the best partition
  candidates (normal 2, slow 8, bc7_enc.s' estimator); per mode: nearest of 3 codes by unquantized value,
  exact palette → the byte bc_dec.s outputs, nearest bytes, least squares; transformed modes only when all
  deltas fit (with the decoder's wrap-around), so mode 11 is always available. Baseline quality (8-bit
  content in an HDR format).

## 4. DDS writer (dds.s)

`dds_encode(sf, opt, &buf, &len)` writes what DirectXTex (paint.net's DDSFileTypePlus) writes:
* `IO_DDSFMT` 1..21 (0 = BC1): BC1..BC7, BGRA8, BGRX8, RGBA8, BGR5A1, BGRA4, B5G6R5, L8, A8, R8G8,
  BC4S, BC5S, BC6HS, R8, BGR8. `IOF_SRGB` selects the sRGB twin of BC1 / BC2 / BC3 / BC7 / BGRA8 / BGRX8 /
  RGBA8 (DXGI 72 / 75 / 78 / 99 / 91 / 93 / 29) and filters mips in linear light.
* Header: legacy FourCC / masks when DirectXTex has one (DXT1 / DXT3 / DXT5, BC4U / BC4S / BC5U / BC5S,
  A8R8G8B8, X8R8G8B8, A8B8G8R8, A1R5G5B5, A4R4G4B4, R5G6B5, L8, A8, R8G8B8), else DX10 (BC6H UF16 95 / SF16
  96, BC7 98, R8G8 49, R8 61, every sRGB twin). DirectXTex writes BC4S / BC5S with legacy FourCCs too — kept;
  R8G8 is written as DX10 49 instead of DirectXTex's A8L8 masks (our reader, like D3D9, reads A8L8 as
  grey + alpha, which would not round-trip). Flags CAPS | HEIGHT | WIDTH | PIXELFORMAT | MIPMAPCOUNT |
  LINEARSIZE (block formats: top-level bytes) / PITCH (row bytes); depth 1; caps TEXTURE (+ COMPLEX |
  MIPMAP with mips, + COMPLEX for cubes); caps2 0xFE00 for cubes; DX10: dimension 2D, misc 4 for cubes,
  array size 1, misc2 0.
* Pixels: BC formats through `bc_encode_to` (IO_EFFORT 1..3 fast, 0 / 4..6 normal, 7..9 slow;
  `IOF_DDS_UNIFORM`; BC1 threshold `IO_ATHRESH`, same bias as the BCQ field). Raw formats: intensity for
  L8 / R8, round(c (2ⁿ − 1) / 255) for 4 / 5 / 6-bit channels, 1-bit alpha a ≥ 128, X = 255, packed rows.
* `IOF_MIPMAPS`: levels down to 1 × 1 (floor(log2(max)) + 1), each from the previous level with rs
  (`IO_MIPFILTER` 0 / FANT / BOX → Fant area average, LINEAR bilinear, CUBIC bicubic, NEAREST, LANCZOS
  Lanczos-3; premultiplied filtering; `RS_GAMMA` with IOF_SRGB) — rows in bands of 32 on the pool
  (rs_prepare / rs_resize_rows / rs_free). XRGB / A8 sources are converted to ARGB once for the mips.
* `IOF_CUBE_CROSS`: w = 4 s, h = 3 s (else ERR_DIMS); faces from the horizontal cross cells that
  `dds_decode` produces (+X (2,1), −X (0,1), +Y (1,0), −Y (1,2), +Z (1,1), −Z (3,1)), each face with its
  whole mip chain (DDS order).
* Errors: ERR_UNSUPPORTED (IO_DDSFMT unknown), ERR_DIMS, ERR_NOMEM, ERR_CANCEL (IO_CANCEL polled per face /
  level, per block row, every 64 raw rows); everything allocated is freed on every path.

## 5. codectest ops (dds_ops.s, CB)

| op | function | arg / data → result |
|---|---|---|
| 107 | bc_encode_block | bcfmt \| cfg << 8, n × 64 bytes → n blocks (guard-page output buffer) |
| 108 | bc_encode | bcfmt \| cfg << 8, surface record → blocks |
| 109 | time bc_encode | as 108, data = u32 reps + surface → u32 µs per call, u32 bytes |
| 110 | bc_encode_to, cancel flag set | as 108 → result code |
| 111 | synthetic img_encode | IMGOPT + {w, h, pattern (\| 256: IO_CANCEL set), reps} → {u64 len, u32 µs, u32 FNV-64 low} |
| 112 | time img_encode | reps, IMGOPT + surface → {u32 µs, u32 len} |

## 6. Tests (all pass, Windows and Linux)

`python tools/bc_check.py --target both --out build/cb` (+ `--quick`, `--only`):
- **block** 5634 checks per target: 97 synthetic blocks (noise, one / two colours, ramps, extremes, alpha
  patterns) × every format × level × metric × BC1 threshold (0 / 1 / 201): bc_dec.s == Python reference,
  no reserved BC6H / BC7 modes, BC1 transparency exactly at the threshold, one-colour blocks exact for BC7 /
  BC4 / BC5 (≤ 1 signed, ≤ 2 BC1..BC3 colour), two-value channels exact (BC3 alpha / BC4 / BC5), BC2 alpha
  = round(a / 17); slow BC4 / BC4S equal to an exhaustive search over all 65536 endpoint pairs on every
  test block (ratio 1.000).
- **image** 451: 5 photos / renders (Windows wallpapers, 512² crops) + 7 synthetic images (ffmpeg testsrc2 /
  smptehdbars / mandelbrot, gradient, noise, alpha ramp, text-like edges) × 10 formats × 3 levels; decoded
  by bc_dec.s, a sample of every stream through the Python reference (identical); minimum PSNR on the
  photos (BC1 ≥ 33, BC7 normal ≥ 42, …).
- **mesa** 14400: sampled blocks of every stream decoded by Mesa llvmpipe: BC6H / BC6HS / BC7 identical to
  the reference (1440 / 1440 each); BC1..BC5 within 2 (Mesa bit-replicates / truncates).
- **ref**: Mesa's own encoders and WIC's DDS encoder (DirectXTex BC code, writes BC3) on the photos (§7).
- **edges** 47: 1×1, 3×5, 5×3, 4097×1, 1×4097, 17×9 == the explicitly edge-padded image; PARGB source ==
  its unpremultiplied ARGB, XRGB == opaque ARGB; cancel → ERR_CANCEL; bad bcfmt → ERR_UNSUPPORTED.
- **determinism**: 90 streams (3 images × 10 formats × 3 levels) byte-identical for `--threads 1` / the pool
  on Windows and Linux.
- **perf**: §8.

`python tools/dds_write_check.py --target both --out build/cb` (+ `--full` for the 16384² case):
header 104 (every IO_DDSFMT × linear / sRGB × mips × cube: all fields + file length), raw 249 (stored
bytes == Python conversion model for ARGB / PARGB / XRGB / A8 sources, our reader's decode, IMGMETA,
IMF_SRGB / IM_DXGI of the sRGB twins), bc 170 (every level's blocks decode (Python) to what our reader
returns), mips 110 (sizes to 1 × 1; Fant levels vs a Python area average max |d| 1; other filters close;
black / white checker level 1 mean 128 in gamma space, 188 with IOF_SRGB), cube 39 (cross → cube → cross,
per face and level; wrong cross sizes → ERR_DIMS), wic 6 (DXT1 / DXT3 / DXT5 files decoded by WIC
identically to our reader; WIC decodes neither the DX10 sRGB twins nor uncompressed DDS), ffmpeg 15
(DXT1 / 3 / 5, BC4U / BC4S / BC5U / BC5S, A8R8G8B8, X8R8G8B8, A8B8G8R8, R5G6B5, A1R5G5B5, L8, A8, R8G8B8:
max |d| 0..1; ffmpeg has no A4R4G4B4), errors 6 (unknown IO_DDSFMT, IO_DDSFMT 0 = BC1, IO_CANCEL for BC1 /
BC7 / BGRA8), big 25 / 28 (4097 × 1, 1 × 4097, 3 × 4097 with 13 levels, 1 × 1 in all 21 formats; `--full`:
16384² BC1 + 15 levels (179 MB, 6.1 s incl. the 1 GB source), 8192² BGRA8 + mips (358 MB), 8192² BC7
(67 MB, 12.4 s)).

CD's `tools/dds_check.py` (bcblock, surface, formats, layout, errors) and C0's `tools/imgmeta_check.py`
(fmtinfo, encode — they follow the runtime caps) still pass.

`python tools/asmcheck/asmcheck.py` on bc_enc / bc7_enc / bc6h_enc / dds / dds_ops (both targets):
**0 errors, 0 warnings, no annotations** (table indices bounded by masks / explicit branches).

## 7. Quality (PSNR in dB, levels fast / normal / slow)

Photos: RGB PSNR (BC4 vs the intensity, BC5 R / G); default perceptual metric for BC1..BC3.

| image | BC1 | BC3 | BC4 | BC5 | BC6H | BC7 |
|---|---|---|---|---|---|---|
| photo-lake (trees, water) | 37.9 / 38.6 / 38.8 | 37.9 / 38.6 / 38.8 | 46.0 / 47.5 / 47.9 | 45.4 / 46.8 / 47.2 | 45.0 / 46.3 / 46.5 | 45.7 / 47.6 / 48.0 |
| photo-hills | 42.0 / 43.5 / 43.7 | 42.0 / 43.5 / 43.7 | 54.7 / 56.5 / 56.9 | 54.5 / 56.1 / 56.6 | 51.0 / 53.0 / 53.2 | 50.1 / 52.5 / 53.3 |
| render-stripes | 33.2 / 33.5 / 33.5 | 33.2 / 33.4 / 33.5 | 43.3 / 44.8 / 45.2 | 42.7 / 44.3 / 44.7 | 34.3 / 38.3 / 39.3 | 37.8 / 44.9 / 45.6 |
| render-ribbons | 36.1 / 36.5 / 36.5 | 36.1 / 36.4 / 36.5 | 46.6 / 48.9 / 49.2 | 45.1 / 47.4 / 47.9 | 38.1 / 42.3 / 43.0 | 40.6 / 46.5 / 47.1 |
| photo-collage | 37.7 / 38.6 / 38.7 | 37.7 / 38.3 / 38.4 | 46.4 / 47.8 / 48.4 | 46.5 / 48.0 / 48.4 | 43.2 / 46.1 / 46.5 | 45.4 / 48.2 / 48.6 |

Uniform metric (BCQ_UNIFORM / IOF_DDS_UNIFORM), BC1 normal: 39.2 / 44.7 / 34.9 / 38.0 / 39.2 (+0.4..1.2 dB RGB
PSNR; the perceptual default trades RGB PSNR for green / luminance fidelity, like DirectXTex).
Signed BC4S / BC5S are within 0.1..0.3 dB of the unsigned ones, BC6HS within 0.5 dB of BC6H.

Independent encoders on the same photos (our normal level in parentheses):
- WIC's DDS encoder (Windows' DirectXTex BC code, BC3): 38.14 (38.60), 42.25 (43.52), 33.24 (33.43),
  35.91 (36.45), 37.89 (38.28) → ours ≥ DirectXTex on all five.
- Mesa llvmpipe encoders: BC1 34.8 / 36.2 / 32.8 / 35.2 / 34.8, BC4 46.1 / 54.7 / 43.5 / 47.1 / 46.6, BC5 45.5 /
  54.4 / 43.0 / 45.9 / 46.6, BC7 33.4 / 37.6 / 29.9 / 32.3 / 33.3, BC6H 36.0 / 42.8 / 31.3 / 35.0 / 35.8 → ours
  better everywhere (checked: ours ≥ Mesa − 0.5).
- Targets: BC7 normal ≥ 42 dB on photos (44.9 .. 52.5), BC1 ≥ 33 dB (33.5 .. 43.5): met.

## 8. Performance

Ryzen 5 3600 (6 cores / 12 threads) while other agents kept the CPU 70..80 % busy; photo content tiled.

| | single thread (Win — Linux) | pool (Win — Linux) |
|---|---|---|
| BC1 4096² fast / normal / slow | 0.27 / 0.88 / 1.33 s — 0.35 / 0.85 / 1.39 s | 0.08 / 0.24 / 0.38 s — 0.15 / 0.32 / 0.50 s |
| BC7 2048² fast / normal / slow | 0.16 / 2.51 s — 0.18 / 2.49 s | 0.05 / 0.63 / 7.3 s — 0.09 / 0.74 / 8.0 s |
| BC3 / BC5 / BC4 2048² normal | 0.25 / 0.18 / 0.09 s | 0.07 / 0.05 / 0.03 s |
| BC6H 2048² normal | 7.1 s — 6.5 s | 1.6 s — 2.0 s |
| DDS 4096² + mips (pool, synthetic) | | BC1 0.40, BC3 0.43, BC7 fast 0.25, BC7 normal 3.7, BGRA8 0.07 s |

Targets BC1 4096² < 1.5 s and BC7 normal 2048² < 6 s single-threaded: met. Pool scaling (BC7 normal,
1024², same bytes for any thread count): 1 → 4 → 12 threads 1.45 → 0.81 → 0.43 s (Windows), 1.36 → 0.52 →
0.33 s (Linux) with the CPU otherwise ~80 % busy; noise-like synthetic content costs ~21 µs / block for BC7
normal (no early exits) against ~10 µs on photos, which explains the 6.6 s once measured for a synthetic
4096² + mips file (no scaling defect).

## 9. Memory-safety review (x86-memory-safety-review checklist)

Inputs are pixels only (no parsing); the risks are sizes, fixed buffers and indices:
- sizes 64-bit throughout (dst offsets (by · bw + bx) · bs, level / chain / file sizes, row buffers 16 w);
  w, h ≤ 65535 checked (ERR_DIMS) before any size math in dds_encode; allocation failures → ERR_NOMEM with
  everything freed (per-row scratch, level surfaces, output buffer) on every error / cancel path.
- fixed buffers: pixel sets ≤ 16 lanes (16-bit masks, explicit lane bound), palettes ≤ 16 entries (1 KB
  broadcast), candidate list ≤ 16 (K clamped), bit-writer position masked, BC6H header fields 16 entries
  (field index & 15), layout offset ≤ 1015; all table indices masked / branch-bounded (asmcheck proves them).
- frames < 4 KB (largest: dds_encode, 272 bytes of locals); block contexts on the heap (≤ 11 KB of the
  16 KB BCE_SCRATCH, checked with `.if` at assembly time).
- **Finding (fixed):** `dds_whdr` cleared 148 header bytes also for a 128-byte legacy header, i.e. up to 12
  bytes past the logical output buffer for tiny files (1 × 1 BC1 / BC4: 136 bytes); hidden by mem_alloc's
  page rounding. Now exactly the header size.
- Found by the test suites during development (all fixed): BC2 alpha rounding ((a + 8) · 3855 >> 16 was off at
  multiples of 17 → 3856), a register clobbered by an asmcheck-driven change in the BC7 partition estimator
  (quality regression caught by the PSNR thresholds), the legacy pixel-format table index mask.

## 10. Needed changes in shared files (integrator)

- `img.inc`: my CB block (constants only). `DDSF_*` 17..21, `MIPF_FANT` / `MIPF_LANCZOS`, `IOF_DDS_UNIFORM`
  (bit 16 of IO_FLAGS, DDS only) and `BCQ_*` should be mentioned in DESIGN.md §4; `img_opt_init(IMGF_DDS)`
  keeps DDSF_BC1 (paint.net's default) — an all-zero IMGOPT gives the same.
- `bc_dec.s` (CD's): additive `.globl bc6_modes, bc6_layout`.
- The Save-Configuration UI can read everything from `IFI_IOM` (DDSFMT, MIPMAPS, MIPFILTER, EFFORT = BC7 speed
  fast / normal / slow, SRGB, CUBE); the "error metric" option maps to IOF_DDS_UNIFORM (no IOM bit exists).
- API.md: regenerate with `tools/gendoc.py` (new public functions in bc_enc.s).

## 11. Open issues

- BC6H is a baseline for 8-bit content (all modes valid and Mesa-identical, but slower than BC7: ~27 µs /
  block normal, scalar palette decode per mode); float sources would need a half-float surface path.
- BC7 slow is ~12 × normal (DirectXTex's USE_3SUBSETS is similarly slow); K / pass counts are the knobs.
- DDS writer: no DX9-legacy option (DirectXTex's FORCE_DX9_LEGACY: ATI1 / ATI2), no forced DX10 headers, no
  premultiplied-alpha (DXT2 / DXT4) output, no texture arrays / volumes; vertical (3 × 4) crosses are rejected
  (ERR_DIMS) — the reader produces horizontal crosses, which round-trip.
- paint.net shows "BC4 (grayscale)": we store the intensity; a raw red-channel BC4 would need another flag.
