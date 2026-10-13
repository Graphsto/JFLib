# CW — WebP decoding (VP8 lossy, VP8L lossless, RIFF container)

Doc fragment for `ASMLIB/docs/DESIGN.md` §4 / §7 and `ASMLIB/docs/modules/CODECS.md` (to be
merged by the integrator). Everything below is implemented, assembles on both targets, is
bit-exact with libwebp 1.3 (`WebPDecodeRGBA`) on every tested file and is covered by
`tools/webp_check.py`. Encoders are the next package (`webp_encode` / `vp8l_encode` /
`vp8_encode` still return ERR_UNSUPPORTED).

## 1. Files

| File | Content |
|---|---|
| `ASMLIB/src/img/webp.s` | container: `webp_probe`, `webp_decode`, `webp_encode` (stub), `webp_parse`, `webp_alpha`, `webp_codec` (IC_CAPS = IFC_DECODE, IC_IOM 0) |
| `ASMLIB/src/img/webp_vp8l.s` | lossless: `vp8l_decode`, `vp8l_decode_alpha`, `vp8l_build_table`, `vp8l_encode` (stub) |
| `ASMLIB/src/img/webp_vp8.s` | lossy: `vp8_decode`, `vp8_itx`, `vp8_itx_dc`, `vp8_wht`, `vp8_yuv_rgb`, `vp8_encode` (stub) |
| `apps/codectest/src/webp_ops.s` | ops 120..125 (below) |
| `tools/webp_check.py` | checker: libwebp through ctypes in WSL as encoder (full `WebPConfig`) and reference decoder, ffmpeg encodes, Python container writers, models, fuzzing, perf |

No shared file was changed (no img.inc constants were needed; `webp_codec` was already
registered in imgreg.s).

## 2. API

```
webp_probe(data, len, meta) -> eax 1 / 0          container + bitstream headers -> IMGMETA scalars
webp_decode(data, len, sf, dec) -> eax 0 / ERR_*  IMGCODEC decode (still image / first frame)
webp_parse(data, len, info) -> eax 0 / ERR_*      RIFF walk into a WPINFO record (webp.s WI_*)
webp_alpha(alph, len, plane, w, h, dec) -> eax    ALPH chunk payload -> u8 plane[w * h]
vp8l_decode(data, len, sf, dec) -> eax            "VP8L" payload -> new FMT_ARGB surface
vp8l_decode_alpha(data, len, plane, w, h, dec)    header-less VP8L stream (compressed ALPH) -> green
vp8l_build_table(lengths, n, root, table, work)   libwebp BuildHuffmanTable (see §7) -> entries / 0
vp8_decode(data, len, sf, dec) -> eax             "VP8 " payload (len = bytes readable) -> FMT_ARGB, alpha 255
vp8_itx(coeffs, dst) / vp8_itx_dc(coeffs, dst)    inverse DCT (libwebp Transform_SSE2 arithmetic) / DC only
vp8_wht(in, out)                                  inverse WHT into the 16 luma DCs (out[16 * n])
vp8_yuv_rgb(y, u, v) -> eax 0xFFRRGGBB            libwebp VP8YuvToRgb (14-bit multipliers, 6 fraction bits)
```
All Win64, thread-safe (every state on the heap / stack, nothing global written).
`dec` may be 0 for the bitstream functions. `vp8_itx*` work on the BPS = 32 layout.

Decoded pixels are always straight `FMT_ARGB` (the dispatcher premultiplies for PARGB
callers). Lossy images without ALPH get `SFF_OPAQUE`.

## 3. Container (webp.s)

- RIFF size 12 .. 2^32 − 10 and ≤ len − 8 (else ERR_HDR / ERR_TRUNC); bytes after the RIFF are
  ignored. First chunk: `VP8 ` / `VP8L` (simple file; size ≤ RIFF − 12) or `VP8X` (size
  exactly 10, canvas 24-bit + 1, w·h < 2^32); anything else ERR_HDR.
- Chunks are walked with 64-bit padded sizes that must stay inside the RIFF (ERR_HDR).
  Still VP8X images: the image chunk must match the canvas (ERR_HDR); the **last** ALPH
  before the image applies to a VP8 image whatever the VP8X alpha flag says and is ignored
  for VP8L (libwebp ParseOptionalChunks); ICCP / EXIF / XMP are taken only when their VP8X
  flag is set and only the first chunk of each kind (libwebp demux / `WebPDemuxGetChunk`);
  a leading `Exif\0\0` is dropped from EXIF; a broken chunk after the image only ends the
  metadata walk; no image chunk → ERR_TRUNC.
- Bitstream headers are checked like `VP8GetInfo` / `VP8LGetInfo` (key frame, profile ≤ 3,
  shown, first partition < chunk size, start code, 14-bit sizes ≠ 0; VP8L signature 0x2F,
  version 0). Like libwebp, VP8's last token partition and VP8L's end-of-stream limit run to
  the end of the input (still images) / the padded chunk end (animation frames).
- Animation (VP8X animation flag): ANIM (≥ 6 bytes) must precede the ANMF chunks (each ≥ 16
  bytes); bare VP8 / VP8L / ALPH chunks are errors (demux rules). The first frame (optional
  first ALPH + VP8, or VP8L without ALPH; unknown sub-chunks end the frame; padded sub-chunks
  inside the ANMF) must fit the canvas at (2·x, 2·y) with the **bitstream** size. It is
  decoded exactly like `WebPAnimDecoder` does for frame 1: onto a transparent-black canvas
  without blending; the ANIM background colour is only a hint and — as in libwebp — not
  used. Chunk errors after the first frame end the frame count. `ID_FRAME` > 0:
  ERR_UNSUPPORTED (ERR_LIMIT when ≥ frames; stills: ERR_LIMIT).
- IMGMETA: IM_W/H = canvas, IM_BITS 8, IM_MODEL IMM_YCC (VP8) / IMM_RGB (VP8L), IM_CHANNELS
  3 / 4, IMF_ALPHA (VP8X alpha flag, VP8L alpha hint, ALPH present), IMF_LOSSLESS,
  IMF_ANIMATED (> 1 frame), IM_FRAMES, IM_LOOPS (0 = forever), IM_DELAY (first frame ms),
  IM_ORIENT from the EXIF chunk (applied by the dispatcher unless IMGD_NO_ORIENT); blobs with
  IMGD_ICC / EXIF / XMP (also with IMGD_PROBE). `webp_probe` allocates nothing.
- ALPH: header byte method 0 (raw, ≥ w·h bytes else ERR_TRUNC) / 1 (VP8L), filter none /
  horizontal / vertical / gradient (libwebp `filters.c` unfilter: row 0 from the left,
  column 0 of later rows from above), pre-processing 0 / 1 (level reduction: nothing to do,
  libwebp without alpha dithering), reserved bits 0; payload ≤ 1 byte or bad fields → ERR_HDR
  (libwebp fails the whole decode too).

## 4. Lossless (webp_vp8l.s)

- Complete RFC 9649 decoder mirroring `vp8l_dec.c`: transforms (each type once, applied in
  reverse order; predictor with the 14 modes — 14 / 15 act as mode 0 —, row 0 black then L,
  column 0 T, TR of the last column = first pixel of the row; cross colour; subtract green;
  colour indexing with 1 / 2 / 4 / 8-bit bundling, palette delta-coded and padded to 256
  transparent-black entries), colour cache 1..11 bits (hash 0x1E35A7BD), meta prefix codes
  (entropy image; group indices remapped to the used ones — unused groups are still read
  and validated but not stored, like libwebp's mapping), simple / normal codes, code-length
  code with repeat codes and max_symbol, LZ77 with the 120-entry distance map.
- Prefix codes: libwebp's `BuildHuffmanTable` rules exactly (8-bit root + second level, u32
  entries `symbol << 16 | length`; incomplete or over-subscribed codes are errors, one used
  symbol = 0-bit code); two passes (size, then build), table buffer grown on demand.
- End of stream: libwebp's rule — error once more bits were consumed than the data holds
  (streams < 8 bytes count as 8 zero-padded bytes); bits past the end read as zero. For ALPH
  streams that take libwebp's 8-bit path (only colour indexing, no cache, 0-bit red / blue /
  alpha codes: `DecodeAlphaData`) running out of data during the last symbol is accepted; the
  last ≤ 128 bits of such streams go through `vl_a8_tail`, an exact model of libwebp's
  `VP8LBitReader` (its 64-bit window wraps `val_ >> (bit_pos & 63)` past the data and resets
  `bit_pos` at end of stream), so even these corrupt streams decode like libwebp.
- Errors: ERR_HDR (signature, version, repeated transform, cache bits), ERR_CODE (invalid
  code / cache symbol without cache), ERR_DIST (copy before the start / past the end),
  ERR_TRUNC (data exhausted), ERR_NOMEM / ERR_LIMIT (ID_MAXMEM before every big allocation),
  ERR_CANCEL (polled per row).
- Speed: the level-0 image is decoded straight into the surface buffer (contiguous rows,
  then spread to the 64-byte stride in place), SSE2 transforms (predictor modes that only
  use the row above 4 pixels at a time, cross colour with libwebp's pmulhw formulation,
  subtract green).

## 5. Lossy (webp_vp8.s)

Key frames of every profile (inter frames: ERR_UNSUPPORTED). Mirrors libwebp's
`vp8_dec.c` / `tree_dec.c` / `quant_dec.c` / `frame_dec.c` / `dsp/dec*.c` / `io_dec.c` /
`upsampling*.c` / `yuv*.h`:
- Boolean decoder with libwebp's 56-bit refill and eof rule (eof in the first partition after
  a macroblock row or in a token partition after a macroblock → ERR_TRUNC), `VP8GetSigned`
  bit trick; segment header (map, absolute / delta quantiser and filter levels, reset values
  like `ResetSegmentHeader`), filter header (simple / normal, level, sharpness, ref / mode
  deltas), 1..8 token partitions (sizes clamped to what is left, the last one must hold a
  byte), dequantisation (y2 dc · 2, y2 ac · 101581 >> 16 ≥ 8, uv dc index ≤ 117),
  probability updates, skip probability, intra modes (kBModesProba contexts in libwebp's mode
  order), residual tokens (bands, contexts, cat 3..6), int16 dequantised coefficients.
- Transforms as libwebp on x86-64 dispatches them: full blocks through the SSE2 Transform
  arithmetic (16-bit wrap), blocks with ≤ 3 coefficients through TransformAC3_C (int), DC
  only through TransformDC_C, chroma planes with any AC through the full transform for all 4
  blocks, WHT = TransformWHT_C with int16 truncation. (For extreme coefficients the SSE2 and
  C versions of libwebp differ; the x86-64 build is the reference.)
- Reconstruction with libwebp's edge rules (left 129, top 127 incl. top-left in row 0,
  top-left 129 in column 0, top-right replication for 4x4 blocks / rightmost macroblock), DC
  variants at the picture edges (CheckMode), unfiltered top samples for prediction.
- Loop filters (SSE2, 16 lanes: a luma edge or the U and V pixels of a chroma edge), simple and
  normal, per segment / mode strength (PrecomputeFilterStrengths), sharpness, hev thresholds,
  inner edges when 4x4 or coefficients; libwebp's order per macroblock. The saturating byte
  arithmetic reproduces the C clamps exactly (NeedsFilter as 2|p0−q0| + |p1−q1|/2 ≤ t).
- Output: libwebp's fancy upsampling (EmitFancyRGB row pairing: row 0 and, for even heights,
  the last row use one chroma row) and the MultHi YUV → RGB with VP8Clip8, SSE2 (identical to
  libwebp's SSE2 and C versions).

## 6. codectest ops (webp_ops.s)

| Op | Function | Data → result |
|---|---|---|
| 120 | time `img_decode_ex` | file, arg = reps \| IMGD_* << 16 → {µs per call, w, h} |
| 121 | `vp8l_build_table` | {n, root, u8 lengths[n]} → entries, u32 table |
| 122 | `vp8l_decode` | VP8L payload → surface record |
| 123 | `webp_alpha` | {w, h} + ALPH payload → plane |
| 124 | `vp8_decode` | VP8 payload → surface record |
| 125 | `webp_parse` | file → WPINFO fields |

## 7. Internals the encoder agent can reuse

- **Canonical prefix codes**: `vp8l_build_table` / `vl_build` define the code exactly as the
  decoder reads it (codes assigned in (length, symbol) order, bits read LSB first, the first
  bit read is the MSB of the code: the standard deflate convention). A VP8L encoder writes
  `bitrev(code, len)` into the LSB-first stream. The code-length code order is
  `vl_cl_order`, repeat codes 16 / 17 / 18 = `vl_rep_bits` {2, 3, 7} + `vl_rep_offs` {3, 3, 11};
  the decoder accepts only complete codes (or one symbol), so the encoder's length-limited
  Huffman (15 bits, 7 for the code-length code) must be complete; one symbol → simple code.
- **Transforms**: `vl_inv_pred` modes (`.ip_m0..13`, AVG2 = floor average, Select via psadbw,
  ClampedAddSubtract Full / Half with truncating /2) — the forward predictor subtracts the
  same prediction from the *original* neighbours; `CROSS` macro (libwebp's pmulhw form; the
  forward transform subtracts the deltas: r -= g2r·g, b -= g2b·g + r2b·r with the original
  r); subtract green; colour-indexing bundling layout (first pixel in the low bits of green,
  8 >> bits bits per index, palette delta-coded against the previous entry). Distance map:
  `vl_code_to_plane` (0xYX, dx = 8 − X, dy = Y; the encoder needs the inverse map for
  (dx, dy) with dy ≤ 7, |dx| ≤ 8, falling back to dist + 120). Hash of the colour cache:
  `(argb · 0x1E35A7BD) >> (32 − bits)`.
- **VP8 tables** (webp_vp8.s .rodata): `v8_dctab` / `v8_actab` (kDcTable / kAcTable),
  `v8_proba0` (CoeffsProba0), `v8_update` (CoeffsUpdateProba), `v8_bmodes` (kBModesProba in
  libwebp's mode order DC TM VE HE RD VR LD VL HD HU), `v8_bandoff` (kBands · 33),
  `v8_zigzag`, `v8_cat` (cat 3..6 extra-bit probabilities), `v8_scan`, `v8_p4tab` (4x4
  predictor recipes). They were extracted from the installed libwebp binary and verified.
- **VP8 predictors / reconstruction**: `v8_p16_*`, `v8_p8_*`, `v8_p4_*` (rcx = destination
  in the BPS = 32 work buffer with the 129 / 127 edge conventions of `v8_recon`) are exactly
  what an encoder needs for mode decisions and its own reconstruction; `vp8_itx*` /
  `vp8_wht` give the decoder-side reconstruction (the encoder's forward DCT / WHT must be
  libwebp's `FTransform` / `FTransformWHT`, not the inverse); `v8_lfk` + `v8_lfv` / `v8_lfh`
  is the loop filter (an encoder filtering its reconstruction for filter-strength search can
  call them on its planes).
- **Bool decoder model** (`GBCORE`, `v8_load`): the encoder's bool encoder must produce
  streams this model reads; `GETSIGNED` is the prob-128 sign bit.

## 8. Tests (`python tools/webp_check.py --target both --out build/cw`)

Suites and record counts per target (all pass on Windows and Linux):

| Suite | Records | Content |
|---|---|---|
| build | 600 | `vp8l_build_table` vs a Python model of BuildHuffmanTable (random complete / broken / empty codes, 19 .. 2328 symbols) |
| vp8l | 462 | libwebp lossless: methods 0..6, qualities 0..100, exact, near-lossless 20 / 60, image hints; noise / gradients / 2..256-colour palettes (bundling 1 / 2 / 4 / 8 bit) / blocks / ffmpeg test sources, sizes 1x1, 1xN, Nx1, odd, up to 720x480, all alpha kinds |
| ffll | 259 | ffmpeg `-lossless 1 -compression_level 0..6 -quality 0/50/100 -pix_fmt bgra` |
| vp8 | 1093 | libwebp lossy: quality 0..100, methods 0..6, segments 1..4, sns 0..100, filter strength 0..100 (none / simple / normal), sharpness 0..7, partitions 0..3, autofilter, sharp YUV, preprocessing, passes, presets 1..5 |
| fflossy | 222 | ffmpeg `-quality 5..100 -preset picture/photo/drawing/icon/text/default -compression_level`, yuv420p and yuva420p |
| alpha | 338 | libwebp alpha_compression 0/1 × alpha_filtering 0/1/2 × alpha_quality 100/60/0, hand-made ALPH (raw and VP8L, all 4 filters, level-reduction bit), op 123 vs a model, 8 error cases |
| container | 43 | simple / VP8X layouts, ALPH rules, unknown chunks, metadata blobs + flags, Exif prefix, first ICCP, broken trailing chunk, trailing bytes, orientation applied, probe, IMGD_PROBE, frame / maxpx / premultiplied, webp_parse, 19 error files |
| anim | 23 | ffmpeg libwebp_anim (lossy / lossless / alpha, loops), Python layouts (offsets, ALPH + VP8, VP8L, several frames, unknown sub-chunk), frame 1 / frames → UNSUPPORTED / LIMIT, 7 error layouts |
| fuzz | 2000 (default) | truncation, bit flips, byte runs, insertions, structural mutants (RIFF / chunk sizes, VP8L header bits, VP8 frame header); decoded mutants must equal libwebp's output |
| perf | 2 | 12 MP (4000x3000) lossy q80 and lossless m4, best of 3 |

Fuzzing runs done: 3 000 (seed 1234), 20 000 (seed 777) and 30 000 (seed 4242) cases on
both targets — no crash, no hang; 1 087 / 7 275 / 11 143 mutants were decoded by both
decoders and all equal libwebp; mutants only we decode are animations (libwebp's still API
rejects them); none that only libwebp decodes.

Performance (single thread, best of 3; libwebp 1.3 SIMD for comparison):

| File | Windows | Linux (WSL) | libwebp (WSL) |
|---|---|---|---|
| 12 MP lossy q80, 893 KB (perf suite) | 108–130 ms | 145–165 ms | 128 ms |
| 12 MP lossless, 18 MB (perf suite) | 185–257 ms | 238–254 ms | 294 ms |
| 12 MP lossy q90 + heavy noise, 6.3 MB (4.2 bit/pixel) | 471 ms | 519 ms | 511 ms |

Timings vary ±25 % under the load of the parallel agents. Other checks run green on this
build: `imgmeta_check.py --only detect,probe,fmtinfo,decex` (the stub expectations skip
formats that now report IFC_DECODE).

## 9. Static analysis and memory-safety review

`asmcheck` on `webp*.s` + `webp_ops.s`: 0 errors, 0 warnings. Annotations (24 assume, 2 ok)
document table indices bounded by invariants the interval analysis cannot see (CLIPQ cmov
clamps of quantiser indices, coefficient positions < 16, `.rc_check` results 0..6, recipe
edge indices, code-length code positions, distance codes 1..120, 0-terminated category
rows).

Review (x86-memory-safety-review checklist) of all input paths: every RIFF / chunk / sub-
chunk length is 64-bit and checked against the RIFF end before the header or payload is
read; bitstream headers only after their byte counts are checked; bit readers never load
past the data (8-byte loads only with ≥ 8 bytes left, then byte-wise, zero fill); all
output sizes come from the (checked) dimensions, never from declared lengths; w·h and
stride·h in 64 bits; sub-image / entropy-image / palette indices bounded by construction
(group index < 65536 → map sized max + 1, palette 256 entries for any 8-bit index, entropy
image looked up only while row < height — the copy path returns to the end check first);
LZ77 copies checked against decoded / remaining pixels, 16-byte copies stay inside a 64-byte
slack; Huffman tables sized by a validating first pass (pass 1 and 2 compute the same size
by construction); recursion depth ≤ 2 (sub-images have no transforms / meta codes); every
loop bounded by pixel counts, alphabet sizes or the data (reads past the end are detected);
VP8 partition sizes clamped, coefficient writes through the 16-entry zigzag into a fixed
24-block buffer, work-buffer predictors / transforms at fixed offsets (+64 slack for 16-byte
row loads), planes and row buffers with slack for the SIMD reads, SIMD output tails through
the home area; `vl_a8_tail` loads its window only inside [start, start + len). Real bugs
found during development and fixed (all caught by the tests): a copy ending exactly at the image end decoded one more symbol (read past the
image, now back to the end check), an `xchg` that truncated the context pointer, wrong
frame offsets in the scalar 6-tap filter (removed with the SIMD rewrite). Crash-free on
53 000 mutants with the data ending at a page end (codectest guard pages).

## 10. Notes / open issues

- `ID_FRAME` > 0 of animations is not composited yet (ERR_UNSUPPORTED). Adding it needs
  libwebp's `BlendPixelNonPremult` (its approximate integer blend), dispose-to-background and
  the key-frame rules of `anim_decode.c`; the container walk already counts the frames.
- The encoder package owns `webp_encode` / `vp8l_encode` / `vp8_encode` and the IC_IOM of
  `webp_codec` (today 0; paint.net shows quality + lossless for WebP).
- Integrator: regenerate API.md (`python tools/gendoc.py`) for the new public functions;
  merge §2–§5 into DESIGN.md §4 / CODECS.md.
