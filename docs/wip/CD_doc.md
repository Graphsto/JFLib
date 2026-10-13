# CD — TGA, DDS and BC block decoding (phase P1)

Doc fragment for `ASMLIB/docs/DESIGN.md` §4 / §7 (to be merged by the integrator).
Everything below is implemented, assembled on both targets and tested
(`tools/tga_check.py`, `tools/dds_check.py`).

## 1. Files

| File | Content |
|---|---|
| `ASMLIB/src/img/tga.s` | `tga_probe` (C0's strict heuristic, unchanged), `tga_decode`, `tga_encode`, `tga_codec` (IFC_DECODE \| IFC_ENCODE, IOM_RLE \| IOM_BITDEPTH) |
| `ASMLIB/src/img/dds.s` | `dds_probe`, `dds_decode`, `dds_encode` (still ERR_UNSUPPORTED, P2), `dds_codec` (IFC_DECODE, IOM 0) |
| `ASMLIB/src/img/bc_dec.s` | BC1..BC7 block decoders, `bc_block_bytes`, `bc_decode_block`, `bc6h_decode_half`, `bc_decode_to`, `bc_decode`; exported tables |
| `ASMLIB/inc/img.inc` | CD block at the end: `BCF_*`, `IM_DXGI` / `IM_TEXFL` / `IM_TEXN`, `TXF_*` |
| `apps/codectest/src/dds_ops.s` | ops 100..106 (below) |
| `apps/codectest/src/tga_ops.s` | no TGA-specific ops (the checker uses ops 240 / 241 / 252) |
| `tools/tga_check.py`, `tools/dds_check.py` | checkers (Python references, ffmpeg / WIC / Mesa cross-checks, fuzzing, perf) |

`bc_enc.s` is untouched (stub for the BC encoder agent).

## 2. TGA

**Decoder** (`tga_decode`; detection without a `.tga` hint still relies on the strict `tga_probe`):
- Image types 1 / 2 / 3 and RLE 9 / 10 / 11; 0, 32, 33 → ERR_UNSUPPORTED; other types, colour
  map type > 1, a mapped image without map → ERR_HDR; interleaved (descriptor bits 6..7) →
  ERR_UNSUPPORTED; w or h 0 → ERR_DIMS; ID_FRAME ≠ 0 → ERR_UNSUPPORTED.
- Colour mapped with 8- **and 16-bit** indices, entries 15 / 16 / 24 / 32 bit starting at the
  first-entry index; indices outside the map → opaque black (like bmp.s). True colour 15 / 16 /
  24 / 32; grey 8 and 16 (grey + alpha byte). A colour map of a true-colour image and the ID
  field are skipped. All four origins; RLE packets may cross rows; packets running past the
  image are ignored.
- 5-bit channels expand by bit replication `v << 3 | v >> 2` (ffmpeg's rule, not bmp.s's
  rounding) — needed for bit-exactness with ffmpeg.
- Alpha: v2 footer (`TRUEVISION-XFILE.\0`) + extension area (offset ≥ 18, size field ≥ 495,
  inside the file before the footer) → attributes type 3 = straight alpha, 4 = premultiplied
  (result FMT_PARGB with colours clamped to alpha, IMF_SRC_PREMUL), anything else = opaque.
  Without an extension area the storage decides: 32-bit pixels / entries and 16-bit grey carry
  alpha; 16-bit pixels / entries only with descriptor alpha bits > 0; then an alpha channel that
  is 0 everywhere is treated as opaque (IMF_ALPHA cleared, IM_CHANNELS reduced).
- IMGMETA: IM_W/H, IM_BITS 5 (15/16-bit pixels or entries) else 8, IM_MODEL PAL / RGB / GRAY,
  IM_CHANNELS (1 or 3, +1 with alpha), IMF_LOSSLESS, IMF_ALPHA, IMF_SRC_PREMUL.
- Size checks before allocating: img_dims_check (ERR_DIMS / ERR_LIMIT), then the minimum data
  size (uncompressed w·h·bpp; RLE one packet header + pixel per 128 pixels) → ERR_TRUNC, so a
  tiny file claiming 16384² fails without allocating 1 GB. Cancel polled per row.

**Encoder** (`tga_encode`): IO_BITDEPTH 0 / 32 → 32-bit BGRA straight alpha (descriptor alpha
bits 8), 24 → 24-bit BGR with transparent pixels composited over white (premultiplied c +
255 − a, like the JPEG encoder), others ERR_UNSUPPORTED; IOF_RLE → type 10 (run packets for ≥ 2
equal pixels, raw packets otherwise, **never across rows** — TGA 2.0 recommendation); rows
bottom-up (origin lower left: the default every reader supports; paint.net writes it too);
a 495-byte extension area (attributes type 3 for 32 bit, 0 for 24 bit) and the v2 footer.
Width / height > 65535 → ERR_DIMS. Output bound: 18 + h·(w·bpp + w/128 + 2) + 521 (proven:
every run packet saves at least the header of one raw packet), checked defensively per packet
(ERR_OVERFLOW impossible in practice). `img_opt_init(IMGF_TGA)` gives paint.net's default 32 bit
+ RLE.

## 3. DDS

**Headers:** `DDS ` + DDS_HEADER (size must be 124, pixel format size 32 → else ERR_HDR),
optional DX10 header (FourCC `DX10`; dimension 2 = 1D (height must be 1), 3 = 2D (+ cube flag),
4 = 3D (array size 1); array size 0 → ERR_HDR). w, h 1..65535 (ERR_DIMS), depth ≤ 65535
(ERR_HDR); the mip count (0 = 1) is clamped to log2(max side) + 1.

**Pixel formats** (8-bit straight BGRA output):
- Legacy, DirectXTex order RGB → LUMINANCE → ALPHA → BUMPDUDV → FOURCC: any contiguous masks
  of 8 / 16 / 24 / 32-bit pixels (fast paths for A8R8G8B8, X8R8G8B8, A8B8G8R8, X8B8G8R8),
  luminance L8 / L16 / A8L8 / A4L4 → grey (+ alpha), A8 → (0, 0, 0, A), BUMPDUDV (V8U8,
  Q8W8V8U8, V16U16, …) as signed; FourCC DXT1..DXT5 (DXT2 / DXT4 premultiplied), ATI1 / BC4U /
  BC4S, ATI2 / BC5U / BC5S (ATI2 = BC5 unchanged like DirectXTex; ffmpeg swaps R/G), D3DFMT 36,
  110..116. Non-contiguous masks, 1-bit signed fields, YUV, RGBG, … → ERR_UNSUPPORTED.
- DX10: 58 DXGI formats (table `dds_fmts`): BC1..BC7 (UNORM / SRGB / TYPELESS, BC4/5 SNORM,
  BC6H UF16 / SF16), R8G8B8A8 / B8G8R8A8 / B8G8R8X8 (UNORM / SRGB / TYPELESS), R8G8B8A8_SNORM,
  R10G10B10A2 (UNORM / TYPELESS), R11G11B10_FLOAT, R9G9B9E5, R16G16B16A16 UNORM / TYPELESS /
  SNORM / FLOAT, R16G16 UNORM / SNORM / FLOAT, R16 UNORM / SNORM / FLOAT, R32G32B32A32 /
  R32G32B32 / R32G32 / R32 FLOAT, R8G8 UNORM / SNORM, R8 UNORM / SNORM, A8, B5G6R5, B5G5R5A1,
  B4G4R4A4, A4B4G4R4. Everything else (video / planar / palette / integer / depth) → ERR_UNSUPPORTED
  (dds_probe still reports size and layout).
- Conversions: UNORM n bit → round(255 v / (2ⁿ − 1)) through per-channel LUTs (> 16-bit legacy
  masks: 64-bit division); SNORM → round(255 (v' + M) / 2M), v' = max(v, −M) (DirectXTex's
  [−1, 1] → [0, 1] bias); half / float / R11G11B10 / RGB9E5 → round(clamp(v, 0, 1)·255) in single
  precision (img_row_half_to_u8 / img_row_f32_to_u8, NaN → 0, no tone mapping). One channel →
  grey, two → (R, G, 0, 255). sRGB formats keep their bytes and set IMF_SRGB. Premultiplied
  files (DXT2, DXT4, DX10 alpha mode 2) return FMT_PARGB (colours clamped to alpha; the
  dispatcher unpremultiplies for IMGD_STRAIGHT).
- Rows are packed (pitch = w·bpp, like DirectXTex); `dwPitchOrLinearSize` is not trusted (writers
  disagree; sizes always come from the dimensions). Only the data of the decoded mip / faces must
  be present; all spans are checked (64-bit) **before** the surface is allocated.

**Layout:** ID_FRAME = mip level (IM_FRAMES = mip count; ≥ count → ERR_LIMIT). Cube maps (legacy
with any subset of faces, DX10 TEXTURECUBE; cube arrays use the first cube) → **4 × 3 horizontal
cross** of the chosen mip (texassemble's h-cross; empty cells transparent; decided over "+X face
only" because paint.net shows cube maps as a cross and its writer takes a cross back):
```
      +Y
  -X  +Z  +X  -Z
      -Y
```
Texture arrays → element 0; volumes → slice 0 of the chosen mip.

**IMGMETA:** IM_W / IM_H = decoded size of mip 0 (the cross for cube maps), IM_BITS (8, 5/6/4
for small channels, 10/16 for wide, 16/32 + IMF_FLOAT for floats, BC6H 16 + FLOAT), IM_MODEL
GRAY for grey / A8 formats else RGB, IM_CHANNELS, IMF_ALPHA / SRGB / FLOAT / HDR / LOSSLESS
(uncompressed) / SRC_PREMUL, and in the free area (img.inc CD block): `IM_DXGI` (u16 @120, DXGI
format of the data; legacy FourCCs mapped, legacy masks only for the fast paths, else 0),
`IM_TEXFL` (u8 @122, TXF_CUBE / VOLUME / ARRAY / CROSS / PREMUL / LEGACY), `IM_TEXN` (u32 @124,
array size / cubes / volume depth). CA uses 104..119 (IM_CHRM); 120..127 are CD's.

## 4. bc_dec.s API (for the BC encoder agent)

```
bc_block_bytes(bcfmt) -> eax 8 / 16, 0 unknown                      leaf
bc_decode_block(bcfmt, src, dst, stride) -> eax 0 / ERR_UNSUPPORTED  one block -> 4 rows of
                                             4 straight BGRA u32 at dst, rows `stride` apart
bc6h_decode_half(src, sign, dst)            16 x (r, g, b, a=1.0) IEEE halves (128 bytes)
bc_decode_to(src, len, bcfmt, sf, cancel) -> eax 0 / ERR_TRUNC / ERR_CANCEL / ERR_UNSUPPORTED
                                             blocks in raster order into an existing 32-bit
                                             surface (its w x h, edge blocks clipped); cancel =
                                             u32* polled per block row (0 = none)
bc_decode(src, len, w, h, bcfmt, sf) -> eax  new FMT_ARGB surface (args 5, 6 on the stack)
```
`bcfmt` = `BCF_BC1..BCF_BC7` (1..7 = DDSF_BC1..BC7; BC4/BC5 unsigned, BC6H UF16), `BCF_BC4S` 8,
`BCF_BC5S` 9, `BCF_BC6HS` 10. Exported tables (reuse them, they are verified against Mesa):
`bc_part2` / `bc_part3` (u32 per partition, 2 bits per pixel), `bc_anc2`, `bc_anc3a`,
`bc_anc3b` (anchors), `bc_sub2` / `bc_sub3` (u8[16] per partition: subset·16), `bc_w2` /
`bc_w3` / `bc_w4` (weights).

Exactness: BC6H / BC7 integer-exact per the D3D11 spec (Mesa's llvmpipe agrees on every tested
block; BC6H halves identical except Mesa returns −0 where finish_unquantize gives +0 — same
bytes). BC1..BC5: the spec's float formulas evaluated exactly and rounded to nearest; the only
exact ties (BC1 three-colour 1/2 mix: 127.5, 42.5) round down → **identical to WIC** for
BC1/BC2/BC3 on every tested block; Mesa / ffmpeg bit-replicate and truncate (|Δ| ≤ 2 / ≤ 1).
BC4 → grey, BC5 → (R, G, 0, 255); BC4/5 signed: −128 → −127, mode from the raw bytes, values
[−1, 1] → round(255 (v + 1) / 2). Reserved BC7 modes → (0, 0, 0, 0), reserved BC6H modes → black.
BC6H → bytes: round(clamp(h, 0, 1)·255), ties up, negative → 0 (no tone mapping).

Implementation notes: BC7 has one fully unrolled decoder per mode (macros `B7ONE` /
`B7DUAL`): fields read at constant bit offsets (independent reads), endpoints assembled in
registers, palettes interpolated with SSE2 two entries per step, index bits normalised by
inserting a 0 above each anchor so pixel i's index sits at bit i·ib. BC6H is table driven
(`bc6_layout`: one byte field·16 + bit per header bit, `bc6_modes`, `bc6_modemap`).

## 5. codectest ops (dds_ops.s)

| op | function | data / arg → result |
|---|---|---|
| 100 | bc_decode_block | arg bcfmt, n blocks → n × 64 bytes BGRA |
| 101 | bc6h_decode_half | arg sign, n blocks → n × 128 bytes halves |
| 102 | bc_decode | arg bcfmt, {u32 w, h} + blocks → surface record |
| 103 | time bc_decode | arg bcfmt \| reps << 8 → u32 µs per call, w, h |
| 104 | time img_decode_ex | arg reps, file (IMGD_STRAIGHT) → u32 µs, w, h |
| 105 | bc_decode_to with the cancel flag set | → result |
| 106 | img_decode_ex → raw IMGMETA (128 bytes) | arg frame (bit 31: full decode, else IMGD_PROBE) |

## 6. Tests (all pass, Windows and Linux)

`python tools/dds_check.py --target both --out build/cd`:
- bcblock 12 records (~6000 blocks: BC7 every mode × partition × rotation × index selection +
  reserved; BC6H 14 modes × 32 partitions, signed / unsigned + reserved; BC1 both modes;
  BC2/3; BC4/5 signed / unsigned incl. −128 and both modes) exact vs the Python reference;
  bc6half exact halves; mesa: the reference vs Mesa llvmpipe (WSL, EGL surfaceless + ctypes) —
  BC7 and BC6H exact, BC1..5 |Δ| ≤ 2; surface 77 (odd sizes, truncation, cancel); formats 624
  (every legacy / FourCC / DX10 format, 4 sizes, straight and premultiplied output, IMGMETA);
  layout 249 (mip chains, clamped mip counts, missing last mip, DX10 / legacy cube maps incl.
  partial faces and cube arrays, arrays, 1D, volumes); errors 60; ffmpeg 16 (DXT1/3/5, ATI1/2,
  BC4U/BC5U, A8R8G8B8, X8R8G8B8, A8B8G8R8, R5G6B5, A1R5G5B5, L8, L16, A8L8, A8: max |Δ| 0..1);
  wic 12 (BC1..BC3 legacy + DX10: 100 % identical); fuzz 2200 (22 000 in a long run per target,
  no crash / hang); perf.
- `python tools/tga_check.py --target both --out build/cd`: decode 1046 (random + systematic
  types × depths × origins × RLE crossing / not crossing, colour maps incl. 16-bit indices,
  first index, out-of-map indices, alpha rules for every extension type, all-zero alpha, footer
  and extension variants, overruns, 255-byte ID) exact vs the Python reference + meta; ffmpeg
  112 (ffmpeg-written bgra / bgr24 / rgb555le / gray / pal8 × RLE 0/1 and Python-written files:
  identical to ffmpeg's decode); encode 842 (32 / 24 bit × RLE × ARGB / PARGB / XRGB / A8 sources:
  header, extension area, footer, no packet crosses a row, size bound, our decode and
  ffmpeg's decode (120/120) identical to the input); detect 120; errors 41; fuzz 2400 (24 000 in a
  long run).
- `python build.py check --out build/cd`: 0 errors and 0 warnings in tga.s / dds.s / bc_dec.s /
  dds_ops.s (3 `assume`s: TGA image class 1..3 (C0's and mine), BC6H layout offset ≤ 1015 —
  mode table invariant). The global run currently reports 2 errors, both in
  `apps/fonttest/src/main.s` (another agent's work in progress; earlier wl_window.s).
- `build.py test`: stops at those foreign asmcheck errors; `build.run_tests` on both targets
  (all .uis scripts of every app) passes.

**Performance** (4096 × 4096, min of 5, Ryzen 5 3600 under 70..100 % load from other agents):
BC7 bc_decode 44 ms (mode 6) … 92 ms (random modes), DDS BC7 img_decode_ex straight 53 ms (Windows)
/ 72 ms (Linux); BC1 33 / 56 ms, BC3 60 / 81 ms, BC5 59 / 86 ms, BC6H 448 / 460 ms. Target
< 150 ms met on both targets.

**Memory-safety review** (x86-memory-safety-review checklist): all header fields bounded before
use; every span 64-bit against the end (CHECK_SPAN); output sizes only from dimensions; pixel
loops bounded by the pixel count, reads by bpp / block size per step; LUT indices masked to the
LUT size (2^width), divisors non-zero by construction (width ≥ 1 unsigned, ≥ 2 signed); palette
indices < 2^depth entries; table indices masked (asmcheck-proven); fixed-size local buffers
(BC7 palette 64 entries, BC6H fields 16) proven by asmcheck; scratch (LUTs, row buffers,
palette) freed on every path; tiny files with huge dimensions fail before allocating; fuzzing
with inputs ending at a guard page (codectest) on both targets. Findings fixed during the
review: pointer truncation by a 32-bit `xchg` in an early dds_mask_ch draft; stale kind after a
fast-path fallback; allocation before the data-size check (now ordered).

## 7. Needed changes in shared files (integrator)

- `tools/imgmeta_check.py` (C0) still asserts stub behaviour for TGA and DDS: fmtinfo "stub
  claims decode/encode" (TGA caps 0x107, DDS 0x3c5), `img_filter_list` expectations (TGA now in
  the encode lists), decex "stub dds" (`DDS ` + 124 zero bytes now → ERR_HDR, header size 0).
  TIFF / PDN trigger the same checks.
- img.inc: my CD block (BCF_*, IM_DXGI / IM_TEXFL / IM_TEXN at IMGMETA 120..127, TXF_*); please
  mention the IMGMETA free-area split (CA 104..119, CD 120..127) in DESIGN.md §4.
- DESIGN.md §4 / §7 and GUIDE.md: TGA / DDS paragraphs from sections 2–4; API.md via gendoc.

## 8. Open issues / possible improvements

- DDS writer and BC encoders: phase P2 (`dds_encode`, `bc_enc.s` stubs; IC_IOM 0 until then).
- BC6H decode is table driven per header bit (~0.45 s for 4096²); a per-mode run table would be
  ~3× faster if HDR textures matter.
- ffmpeg's TGA decoder does not flip 15-bit right-to-left images (no `case 15` in its flip) —
  excluded from the ffmpeg comparison; ours follows the spec.
- DDS legacy files with DWORD-padded rows (old D3DX, DirectXTex's DDS_FLAGS_LEGACY_DWORD) are
  read packed like DirectXTex's default (visible only for 24-bit / odd widths).
- IM_DXGI is 0 for legacy mask formats without a DXGI twin (e.g. R8G8B8, X8B8G8R8, A8L8).
