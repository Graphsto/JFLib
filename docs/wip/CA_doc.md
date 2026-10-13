# CA — core codecs PNG / JPEG / BMP / GIF (+ quantizer): API, contracts, options, tests

Doc fragment for `ASMLIB/docs/DESIGN.md` §4 / §7, `GUIDE.md` (codec options) and `API.md`
(`python tools/gendoc.py`). Files owned by CA: `ASMLIB/src/img/png.s`, `jpeg_dec.s`,
`jpeg_enc.s`, `jpeg_dsp.s`, `bmp.s`, `gif.s`, `quantize.s`, `imgcore.s`,
`apps/codectest/src/{png,jpeg,bmp,gif}_ops.s`, `tools/{codec,jpeg,gif,png,bmp}_check.py`.
Public constants: `ASMLIB/inc/img.inc`, block "CA: core codecs" at the end.

## 1. What changed (summary)

* **Native straight alpha.** Every core decoder has a `*_fl(data, len, sf, flags)` entry; with
  `JDF_STRAIGHT` it allocates an `FMT_ARGB` surface holding the file's straight values
  (colour kept at alpha 0, PNG colour-key pixels keep their colour with alpha 0). Without the
  flag the output is `FMT_PARGB` and — for PARGB callers — bit-identical to before, except PNG
  16-bit samples (see below). `img_decode_ex` (imgcore.s) uses the native paths, so
  `IMGD_STRAIGHT` no longer unpremultiplies (no precision loss at low alpha). Premultiplied
  output is always exactly `premultiply(straight output)`.
* **Straight encoder input.** Every core encoder accepts `FMT_XRGB / PARGB / ARGB / A8`
  directly (rows through `img_get_row`); the premultiplied temporary copy in the glue is gone.
  ARGB at 32 bit (PNG / BMP) is lossless, including the colour of alpha-0 pixels.
* **PNG 16-bit samples** are rounded: `round(v / 257)` (was: high byte). `codec_check.py`'s
  expectation was updated.
* **Classic API unchanged**: `png_decode / png_encode[_ex] / bmp_decode / bmp_encode /
  jpeg_decode / jpeg_encode / gif_decode / gif_encode / img_quantize` keep their signatures and
  output (checked byte for byte against the C0 reference binary, see §6). `bmp_decode` now also
  decodes RLE4 / RLE8 / BI_JPEG / BI_PNG (were ERR_UNSUPPORTED).

## 2. New / changed functions

```
png_decode_fl(data, len, sf, flags) -> eax 0 / ERR_*          png.s   flags: JDF_STRAIGHT
png_encode_cfg(sf, cfg, &buf, &len) -> eax 0 / ERR_*          png.s   PNGCFG (img.inc PC_*)
png_put_chunk(dst, type, data, len) -> rax end                png.s   chunk writer (data 0 = in place)
bmp_decode_fl(data, len, sf, flags) -> eax 0 / ERR_*          bmp.s   JDF_STRAIGHT (+ JDF_NO_ORIENT for BI_JPEG)
bmp_encode_cfg(sf, cfg, &buf, &len) -> eax 0 / ERR_*          bmp.s   BMPCFG (BC_*)
gif_decode_fl(data, len, sf, flags) -> eax 0 / ERR_*          gif.s   JDF_STRAIGHT (same bytes)
gif_encode_q(sf, &len, qcfg) -> rax buffer / 0                gif.s   quantizer cfg (dither level, threshold)
jpeg_decode_fl(data, len, sf, flags) -> eax 0 / ERR_*         jpeg_dec.s  JDF_*
jpeg_decode_abbrev(tables, tlen, data, len, sf, flags)        jpeg_dec.s  JPEG-in-TIFF (§3)
jpeg_encode_ex(sf, quality, &len, cfg) -> rax buffer / 0      jpeg_enc.s  JENCCFG (JEC_*): chroma, density
jpeg_down_h2v1(dst, src, n)                                   jpeg_dsp.s  4:2:2 downsampling (libjpeg bias 0,1)
img_quantize_ex(sf, max, cfg, pal, idx) -> eax n, edx trans   quantize.s
img_quantize_alloc(sf, max, cfg, pal, &idx) -> eax n, edx trans   quantize.s  (index image mem_alloc'd)
```
imgcore.s: `png/jpeg/bmp/gif_decode_ex` call the `_fl` decoders with `JDF_STRAIGHT` (from
IMGD_STRAIGHT) and `JDF_NO_ORIENT` (from IMGD_NO_ORIENT); `*_encode_opt` translate IMGOPT (§4).
`jpg_orient_compat` is still used, but only to report IM_ORIENT exactly as jpeg_dec reads it;
the old "undo the orientation" workaround is gone (jpeg_dec does not rotate with JDF_NO_ORIENT).

### Flags and records (img.inc, CA block)
| Name | Value | Meaning |
|---|---|---|
| `JDF_STRAIGHT` | 1 | FMT_ARGB output (JPEG / GIF: same bytes, only SF_FMT differs) |
| `JDF_NO_ORIENT` | 2 | JPEG: keep the stored orientation |
| `JDF_RGB` / `JDF_YCC` | 4 / 8 | JPEG: 3 components are RGB / YCbCr whatever the markers say |
| `JDF_CMYK` | 16 | JPEG: 4 components are plain CMYK (not Adobe-inverted) |
| `IQ_THRESH_SHIFT` | 8 | quantizer cfg = level (0..8) \| threshold (0..256) << 8 \| IQF_* |
| `IQF_KEEPALPHA` | 1<<20 | exact palette with per-entry alpha only (eax = 0 when > max colours) |
| `IQF_WHITE` | 1<<21 | composite over white first (no transparency) |
| `PNG_IOM` / `JPEG_IOM` / `BMP_IOM` / `GIF_IOM` | | IC_IOM of the core codecs (also used in imgio.s's static table) |
| `PC_*` (PNGCFG, 40 B) | | PC_LEVEL, PC_DEPTH (0 / 8 / 24 / 32 / PNGD_CLASSIC), PC_QCFG, PC_CHUNKLEN, PC_CHUNKS, PC_CANCEL |
| `BC_*` (BMPCFG, 24 B) | | BC_DEPTH (0 / 8 / 24 / 32 / BMPD_CLASSIC), BC_QCFG, BC_PPMX, BC_PPMY, BC_CANCEL |
| `JEC_*` (JENCCFG, 16 B) | | JEC_CHROMA (0 / 444 / 422 / 420 / 400), JEC_UNIT (0 = 96 dpi, 1 dpi, 2 dpcm), JEC_XD, JEC_YD |
| `IM_CHRM` | 104 | IMGMETA u16[8]: PNG cHRM (white x, y, red x, y, green x, y, blue x, y) in 1/50000 (0 = none) |

**Integrator:** `IM_CHRM` uses bytes 104..119 of IMGMETA's free area — please move it into the
IMGMETA section of img.inc when merging (and consider it for other formats' chromaticities).

## 3. jpeg_decode_abbrev (for the TIFF agent, JPEG compression 7)

```
jpeg_decode_abbrev(tables, tlen, data, len, sf, flags) -> eax 0 / ERR_*
    rcx tables, rdx tlen   JPEGTables contents (tables-only stream) or 0 / 0
    r8 data, r9 len        one strip / tile (abbreviated image stream)
    [rsp+32] sf            SURFACE* (overwritten; surf_free the result)
    [rsp+40] flags         JDF_*
```
* The tables stream is parsed first: SOI, then any DQT / DHT / DRI / APPn / COM segments, then
  EOI or the end of the data. SOF or SOS in it → ERR_HDR; no SOI → ERR_HDR; tlen 1 → ERR_TRUNC.
  Its tables stay loaded; tables in the image stream replace them per slot (libjpeg). Huffman
  tables never defined fall back to Annex K; a quantisation table never defined → ERR_HDR.
  At the image's SOI the restart interval and the JFIF / Adobe state are reset (libjpeg).
* The image stream decodes exactly like `jpeg_decode` (baseline / extended / progressive, 8-bit,
  1 / 3 / 4 components, sampling 1..4, restart intervals, truncated data decodes partially). The
  surface has the SOF size (full strip / tile — crop it yourself).
* Orientation is never applied. `JDF_YCC` for Photometric 6, `JDF_RGB` for Photometric 2,
  neither = libjpeg's default rule; `JDF_CMYK` for Photometric 5 (plain CMYK); 1 component =
  grey (invert yourself for MinIsWhite). Output FMT_PARGB, or FMT_ARGB with `JDF_STRAIGHT`;
  alpha 255, SFF_OPAQUE.
* Errors like jpeg_decode (TRUNC, HDR, UNSUPPORTED: arithmetic / lossless / hierarchical /
  12-bit / 2 components / DNL, DIMS: w or h > 32768 or w·h > 2^28, CODE, NOMEM). On error sf is
  empty and nothing is allocated. Thread safe (heap context).
* Tested by `tools/jpeg_check.py --only abbrev` (every layout / restart / MJPEG-style missing
  DHT / overriding tables / error cases) and `--only jmfuzz` (mutated table + image pairs).

## 4. Save options per format (IC_IOM, IMGOPT)

| Format | IOM | Honoured IMGOPT fields |
|---|---|---|
| PNG | BITDEPTH, DITHER, ATHRESH, EFFORT, ICC, EXIF, XMP | `IO_BITDEPTH` 0 auto / 32 / 24 / 8; `IO_DITHER` (biased, 0 = level 7) and `IO_ATHRESH` (biased, 0 = 128) for 8 bit; `IO_EFFORT` = deflate level (0 = 6); `IOF_EMBED_ICC` → iCCP ("ICC Profile", zlib 9), `IOF_EMBED_EXIF` → eXIf (orientation reset to 1), `IOF_EMBED_XMP` → iTXt "XML:com.adobe.xmp"; pHYs whenever IO_META has a dpi |
| JPEG | QUALITY, CHROMA, ICC, EXIF, XMP | `IO_QUALITY` (0 = 95); `IO_CHROMA` 444 / 422 / 420 / 400 (4:0:0 = luminance only), else 4:4:4 from quality 90 and 4:2:0 below; after APP0: APP1 EXIF (orientation 1; skipped if > 65527 bytes), APP2 ICC (`icc_jpeg_segments`, up to 255 segments), APP1 XMP (skipped if > 65504 bytes); JFIF density from IO_META (IMR_CM → dots per cm, else dpi; none → 96 dpi) |
| BMP | BITDEPTH, DITHER | 0 auto / 32 / 24 / 8; `IO_DITHER` for 8 bit; pixels per metre from IO_META's dpi (else 2835 = 72 dpi) |
| GIF | DITHER, ATHRESH | dither level, transparency threshold |

Bit-depth rules:
* **PNG auto:** A8 → 8-bit grey; ≤ 256 distinct straight colours (all alpha-0 pixels count as
  one, stored as 0x00000000) → 8-bit palette with per-entry tRNS (lossless); else RGB8 when
  opaque, RGBA8 otherwise. **32:** RGBA8 straight. **24:** RGB8, premultiplied colour over white.
  **8:** `img_quantize_ex` (dither level, alpha < threshold → the transparent entry in tRNS);
  palette rows use filter 0 (libpng's rule), others the smallest sum of |signed bytes|.
  The classic `png_encode` rule (XRGB → RGB, PARGB / ARGB opaque → RGB else RGBA, A8 → grey)
  is `PNGD_CLASSIC`.
* **BMP auto:** not opaque → 32 (BITMAPV4HEADER, BI_BITFIELDS, alpha mask, sRGB); opaque with
  ≤ 256 colours → exact 8-bit palette; else 24. **24 / 8** composite over white (BMP palettes
  have no alpha); 8 uses the dither level. Classic `bmp_encode` = `BMPD_CLASSIC` (24 when
  opaque, else 32).
* **GIF:** always the quantizer (exact ≤ 256 colours incl. the transparent one, else Wu + dither);
  `gif_encode` = level 8, threshold 128.
* **Quantizer** (`img_quantize_ex`): input PARGB (unpremultiplied), ARGB, XRGB, A8. Dither
  level L scales the Floyd–Steinberg error: the accumulated error e (16ths) is applied as
  (e·L + 64) >> 7 (= the old (e + 8) >> 4 at L = 8, so `img_quantize` output is unchanged).
  Threshold t: alpha < t → transparent (t = 0: never, 256: always). paint.net's defaults
  (level 7, threshold 128) are what `IO_DITHER = 0` / `IO_ATHRESH = 0` give.

## 5. Decoder details

* **PNG:** all colour types / depths; straight output as above; 16-bit round(v/257) in both
  outputs; APNG → the default image (acTL / fcTL / fdAT are ancillary and skipped; ID_FRAME > 0
  → ERR_UNSUPPORTED). Metadata scanner (imgcore.s png_meta): IHDR, tRNS, pHYs (unit metre →
  dpi, IMR_CM), gAMA, sRGB, **cHRM (IM_CHRM, new)**, cICP, iCCP (IMGD_ICC; inflate capped at
  **16 MB**, bigger / broken → no profile), eXIf (IM_ORIENT; blob with IMGD_EXIF), iTXt XMP
  (plain or zlib), acTL / first fcTL (frames, loops, delay).
* **BMP:** RLE8 / RLE4: encoded runs (RLE4 colour pairs), absolute runs (padded to 16 bits),
  end of line, end of bitmap, delta; pixels the stream skips stay transparent (0x00000000);
  runs past the right edge are clipped; everything is checked against the data end (a stream
  that ends early gives a partial image, result 0). BI_JPEG / BI_PNG: the file at bfOffBits
  (biSizeImage bytes, 0 or too large = up to the end) through jpeg_decode_fl / png_decode_fl
  (flags passed on; the embedded file's size wins). OS/2 2.x (64-byte header) Huffman 1D
  (comp 3) and RLE24 (comp 4), CMYK and unknown compressions → ERR_UNSUPPORTED. V5 embedded
  ICC (PROFILE_EMBEDDED), LCS_sRGB / Windows colour space → IMF_SRGB, pels per metre → dpi (the
  scanner already had these; verified). For BI_JPEG / BI_PNG the scanner reads the embedded
  file's header (`bmp_embed_meta`), so IM_W / IM_H — and with them img_decode_ex's
  ID_MAXPX / ERR_DIMS check — are the embedded image's real size, not the BMP header's.
* **GIF:** transparent index → 0 in both formats. Scanner: frames (all blocks), first delay,
  NETSCAPE loops, **ICCRGBG1 "012" application extension → IM_ICC (IMGD_ICC, new; ≤ 16 MB)**.
* **JPEG:** as before; `JDF_NO_ORIENT` natively; scanner: SOF, JFIF / EXIF density, EXIF blob +
  orientation + ColorSpace, XMP, Adobe APP14 (colour model), ICC via icc_jpeg_collect.

## 6. Tests

All tools take `--target win|linux|both --out build/ca --no-build --only ... --fuzz N`.
* `tools/png_check.py` (new): straight (965 records: every type / depth / interlace / tRNS, every
  16-bit value, every RGBA8 colour × alpha pair), meta (all chunks, zlib XMP, iCCP bomb, 4 MB
  iCCP, eXIf orientations 1..8 applied / kept), apng, enc (705: depths × dither × threshold ×
  input formats, img_encode path; Python reader checks every CRC, PLTE / tRNS rules), encmeta +
  metart (iCCP / pHYs / eXIf / iTXt written and read back), classic (png_encode byte-identical to
  the C0 binary), wic (our 8/24/32/auto files decoded by WIC), fuzz, shots
  (`build/ca/win/pngcheck/pal8_*.png`, viewed: dither 0 / 4 / 8, thresholds 128 / 1).
* `tools/bmp_check.py` (new): straight, rle (378: Python writer + decoder model, hand-made
  escapes, cuts; full images also against ffmpeg), embed, unsup, meta (V5 ICC, sRGB, dpi), enc
  (512 + WIC), classic (bmp_encode / bmp_decode byte-identical to C0), fuzz.
* `tools/jpeg_check.py` new suites: flags, abbrev, encex (chroma × quality × density × ARGB /
  PARGB: identical files, identical on both targets, our decoder == libjpeg-turbo for every
  file, PSNR), jmeta (EXIF / ICC 1 + 3 segments / XMP / density written and read back),
  classic (jpeg_encode / jpeg_decode byte-identical to C0), jmfuzz.
* `tools/gif_check.py` new suites: quantex (thresholds, IQF_WHITE, IQF_KEEPALPHA, levels),
  qcompat (level 0 / 8 == img_quantize, level 4 differs), camisc (gif_decode_fl, gif_encode_q,
  img_encode options, ICCRGBG1, frames / loops).
* `tools/codec_check.py`: 16-bit PNG expectation = round(v/257); RLE8 case updated.
* codectest ops: 25..27 (jpeg_ops.s), 45..47 (gif_ops.s), 260..263 (png_ops.s: 260 = img_encode
  with IO_META for any format, 261 = img_decode_ex returning the whole IMGMETA incl. IM_CHRM),
  280 / 281 (bmp_ops.s).
* Reference binaries for the byte-identity suites: `build/ca/ref/codectest_c0.exe` (copy of
  C0's `build/c0/win/codectest.exe`, the pre-CA state); the suites are skipped when it is absent.

## 7. Notes for the integrator

* **imgio.s** (minimal edit): the static g_img_fmts entries of PNG / JPEG / BMP / GIF use
  `PNG_IOM / JPEG_IOM / BMP_IOM / GIF_IOM`; GIF got `IFC_ICC` (also in img_fmt_props). The file
  header comment "a straight PNG / BMP / GIF result is unpremultiplied from them" is outdated
  (they are native now) — please update when you touch imgio.s.
* **tools/imgmeta_check.py** (minimal edits): IOM expectation of the core formats; "bmp 24"
  probe asks for `bitdepth=24` (auto now picks an exact 8-bit palette for ≤ 256 colours);
  "straight 0/255 exact" accepts the file's own colour at alpha 0 (native straight decode).
* DESIGN.md §4 lines to update: PNG decode "16-bit samples keep their high byte" → rounded;
  BMP "RLE and embedded JPEG/PNG give ERR_UNSUPPORTED" → decoded; encoder descriptions (§4 above).

## 8. Test status (CA, build/ca, both targets)

* `codec_check.py`: all suites pass (win + linux).
* `jpeg_check.py`: all suites pass, old and new (model, pydec, pywriter, ffdec, gdidec, errors,
  damaged, enc, flags, abbrev, encex, jmeta, classic, jmfuzz, fuzz).
* `gif_check.py`: all suites pass (model, ext-py, dec, fuzz 8319, quant, quantex, qcompat, camisc,
  classic, enc, ext, ext-enc).
* `png_check.py`, `bmp_check.py`: all suites pass (fuzz 2500 / 2667 records per target with
  guard pages; WIC and ffmpeg cross-checks).
* `imgmeta_check.py`: every core-format suite passes; the remaining failures are the "stub"
  expectations of TIFF / TGA / DDS / PDN, which other agents have implemented meanwhile
  (fmtinfo, decex "stub ...", encode "stub encoder ...") — for the integrator to update.
* asmcheck (`build.py check`): 0 errors in CA files (the 19 errors of the full run are in
  plat/linux/wl_*.s, another agent's work in progress). BOUNDS warnings in CA files are the
  proven-in-context table / palette walks (quantizer axis tables, adam7, jd_bias, palette
  loops bounded by counts <= 256).
* Memory-safety review (x86-memory-safety-review checklist) of all new / changed CA code:
  RLE decoder (x <= W and y < H invariants, every read against the data end, truncated
  absolute runs clipped to the bytes present), embedded BMP payloads (bfOffBits / biSizeImage
  clamped to the data), GIF ICC sub-block gathering (two passes, chain validated before
  copying, 16 MB cap), PNG cHRM / chunk writers (sizes computed from the same conditions that
  write), JPEG APPn assembly (segment lengths bounded, oversize EXIF / XMP skipped),
  abbreviated JPEG parsing (all through jd_run's bounds), encoder buffers (64-bit size math).
  Two real bugs found by the tests during development and fixed: the quantizer's work buffer
  size was computed from a clobbered register (heap overflow for images >= ~1000 pixels), and
  je_rows clobbered rcx (the source row pointer) before jpeg_rgb_ycc (wrong colours, no memory
  error). Both are covered by regression tests (png_check enc sizes 64x33, jpeg_check classic
  byte identity).

