# CT — TIFF codec (decoder + encoder)

Doc fragment for `ASMLIB/docs/DESIGN.md` §4 / §7 and `GUIDE.md` (codec options). Everything below
is implemented and tested on both targets (`tools/tiff_check.py`, see §6).

## 1. Files

| File | Content |
|---|---|
| `ASMLIB/src/img/tiff.s` | `tiff_probe`, `tiff_decode`, `tiff_encode`, `tiff_codec` (IMGCODEC); IFDs through exif.s `tifr_*` |
| `ASMLIB/src/img/tiff_ccitt.s` | `ccitt_decode` — CCITT Modified Huffman / T.4 (1D, 2D) / T.6 |
| `apps/codectest/src/tiff_ops.s` | codectest ops 60..62 (timed decode, encode with IO_META, timed encode) |
| `tools/tiff_check.py` | the checker (writer, numpy decoder model, WIC / ffmpeg / libtiff references, fuzz, perf) |

No shared file was changed (no public constants needed: TIFC_* already existed). `IC_CAPS` =
`IFC_DECODE | IFC_ENCODE`, `IC_IOM` = `IOM_BITDEPTH | IOM_DITHER | IOM_TIFFCOMP | IOM_ICC | IOM_XMP`.

## 2. Decoder

`tiff_decode(data, len, sf, dec)` (IMGCODEC decode; the dispatcher applies IM_ORIENT),
`tiff_probe(data, len, meta)` (header scan; the only allocation is the context, freed again).

* **Structure**: II / MM, classic and BigTIFF (exif.s reader: spans checked, cycles and > 1024
  IFDs rejected). `IM_FRAMES` = IFDs of the main chain (it ends at a zero / out-of-range pointer,
  a cycle or 1024 IFDs), `ID_FRAME` selects the page (past the last: ERR_LIMIT). Strips and tiles
  (≤ 65536 × 65536), PlanarConfiguration 1 and 2, RowsPerStrip missing / 0 / > height = whole
  image, missing StripByteCounts (uncompressed: the strip size, else up to the end of the file),
  byte count 0 = sparse strip (zeros), last strip holding full RowsPerStrip rows, SHORT / LONG /
  LONG8 offsets.
* **Photometric**: 0 WhiteIsZero, 1 BlackIsZero, 2 RGB, 3 palette (1..16 bit; ColorMap with
  fewer entries: the rest black), 5 CMYK (InkSet 1, ≥ 4 samples), 6 YCbCr (8 bit; subsampling
  1 / 2 / 4 × 1 / 2 / 4 incl. partial blocks, default 2 × 2, YCbCrCoefficients and
  ReferenceBlackWhite with libtiff's `TIFFYCbCrToRGBInit` tables — bit-exact —, chroma replicated
  over the block like `TIFFRGBAImage`; YCbCrPositioning therefore has no effect), 8 CIELab
  (8 / 16 bit, D50 → Bradford → linear sRGB → sRGB curve). Photometric missing: RGB for ≥ 3
  samples, WhiteIsZero for CCITT, else BlackIsZero (libtiff's guesses).
* **Samples**: 1..32-bit unsigned (SampleFormat 1 / 4) and signed (2: mapped to offset binary)
  integers, all bit-packed depths MSB first; floats 16 / 24 (Photoshop float24) / 32 bit.
* **Compression**: 1 none, 5 LZW (and libtiff's old bit-reversed LSB-first "compat" LZW, detected
  by `00` + odd second byte), 8 / 32946 Deflate (zlib header checked, Adler not; output past
  the strip is ignored), 32773 PackBits (runs may cross rows), 7 JPEG (`jpeg_decode_abbrev` with
  JPEGTables; JDF_YCC for Photometric 6, JDF_RGB for 2, JDF_CMYK for 5, grey inverted for 0;
  JPEG strips / tiles larger than needed are cropped), 2 / 3 / 4 CCITT (tiff_ccitt.s, §4).
  FillOrder 2 reverses the input bits first (every compression but JPEG).
  ERR_UNSUPPORTED: 6 (old JPEG), 34712 (JPEG 2000), 34925 (LZMA), 50000 (ZSTD), 50001 (WebP),
  34887 (LERC), 34676 / 34677 (LogLuv), 32771 and anything else; JPEG with alpha / planar data /
  palette / Lab; 64-bit samples; > 16 samples per pixel.
* **Predictor** (only with LZW / Deflate, like libtiff; ignored otherwise): 2 = horizontal
  differencing for 8 / 16 / 32-bit samples (file byte order) and every bit-packed depth (24 bit
  unsupported, like libtiff); 3 = floating point predictor (16 / 24 / 32-bit floats).
* **Alpha**: ExtraSamples 2 (unassociated) → FMT_ARGB; 1 (associated): 8 bit → FMT_PARGB with the
  colour clamped to alpha (the dispatcher unpremultiplies with the library formula), deeper /
  float → unpremultiplied at full precision (`round(v · max / a)`, floats `v / a`) → FMT_ARGB;
  0 or no ExtraSamples: alpha only for 4-sample RGB and 2-sample grey (the task's rule). Opaque
  images come out in the requested format (no conversion pass).
* **Sample depth → 8 bit** (chosen to match WIC = paint.net's TIFF decoder where WIC is exact):

  | Source | Rule | vs WIC |
  |---|---|---|
  | RGB / CMYK / alpha 16 bit | `v >> 8` | exact |
  | grey 16 bit | `round(v / 257)` | ±1 (WIC's grey path is non-linear) |
  | palette ColorMap | `trunc(v / 257)` | exact (libtiff: `v >> 8`) |
  | other integer depths (1..15, 17..32) | `round(v · 255 / (2^n − 1))` | 2 / 4 / 6 bit exact, 12 bit ±1 |
  | float colour | linear → sRGB (library table, `img_row_f32_to_srgb8`) | ±1 |
  | float alpha | `clamp(v, 0, 1) · 255` rounded, NaN → 0 | exact (32 bit) |
  | CMYK | libtiff: `(255 − k)(255 − c) / 255` (truncated) | WIC converts colour-managed (differs) |
  | CIELab | D50 → sRGB (Bradford) | in gamut ±1; out of gamut WIC maps differently |

* **Metadata**: IM_W/H, IM_BITS (bits per sample; IMF_FLOAT for floats), IM_MODEL (GRAY / RGB /
  PAL / CMYK / YCC / LAB), IM_CHANNELS (samples), IM_FLAGS (ALPHA, SRC_PREMUL, FLOAT, HDR for
  > 8 bit, LOSSLESS unless JPEG), IM_ORIENT (tag 274, 1..8, else 1), IM_DPIX/Y + IM_RESUNIT
  (ResolutionUnit 2 inch (default) / 3 cm → dpi; 1 → 0), IM_FRAMES. Blobs on request: IMGD_ICC
  (tag 34675), IMGD_XMP (700), IMGD_EXIF: the page's Exif (34665) and GPS (34853) IFDs are rebuilt
  as a standalone classic TIFF structure in the file's byte order (IFD0 = ImageDescription, Make,
  Model, Orientation, X/YResolution, ResolutionUnit, Software, DateTime, Artist, Copyright +
  pointers; Exif / GPS entries copied with relocated values; Interop / SubIFD pointers and
  BigTIFF-only types dropped; > 16 MB not built) — `exif_read` reads it.
* **Limits / safety**: BitsPerSample 0 → ERR_HDR, > 32 → ERR_UNSUPPORTED; sizes from the
  dimensions only (64-bit math); offset / byte-count arrays must cover every chunk (ERR_HDR);
  offsets past the end → ERR_TRUNC, byte counts clamped to the file; decompressed data shorter
  than the rows needed → ERR_TRUNC; chunk buffers ≤ 4 GB and ID_MAXMEM (ERR_LIMIT); frames < 4 KB
  (all state in heap contexts); ID_CANCEL polled per strip / tile group.
* **Parallel decoding**: images of ≥ 1 MP with ≥ 2 strip / tile groups are decoded with
  `thr_parallel_for` (each worker takes a clone of the context with its own buffers; groups write
  disjoint surface areas). The error of the lowest failing group is returned, so results and
  error codes never depend on the number of threads (`--threads 1` gives identical output).
  When the clones do not fit into ID_MAXMEM / memory, decoding is sequential.

## 3. Encoder

`tiff_encode(sf, opt, &buf, &len)`: little-endian classic TIFF, one page, strips of ~64 KB
(RowsPerStrip = 65536 / row bytes, ≥ 1), PlanarConfiguration 1, IFD after the strips.

| IMGOPT | Effect |
|---|---|
| `IO_BITDEPTH` 0 auto | not opaque → 32; opaque with ≤ 256 colours → exact 8-bit palette; else 24 (the BMP encoder's rule) |
| 32 | RGBA 8 bit, ExtraSamples = 2 (unassociated), straight values (alpha-0 colour kept) |
| 24 | RGB, colour composited over white (`(c·a + 255·(255 − a) + 127) / 255`) |
| 8 | palette: `img_quantize_alloc` with the IO_DITHER level over white (IQF_WHITE); ColorMap = v · 257 |
| `IO_DITHER` | level + 1 (0 = default 7), used for 8 bit |
| `IO_TIFFCOMP` | 0 / TIFC_LZW = LZW (default), TIFC_DEFLATE = Deflate (tag 8, zlib level 6), TIFC_PACKBITS (each row packed separately — TIFF 6.0; ffmpeg decodes row by row), TIFC_NONE |
| Predictor | 2 (horizontal differencing) for LZW and Deflate at 24 / 32 bit |
| `IO_META` | X/YResolution from IM_DPIX / IM_DPIY (16.16; whole numbers as n/1, else /65536; IMR_CM → dots per cm ·10000 / 10000; default 96 dpi), ResolutionUnit 2 / 3; `IOF_EMBED_ICC` → tag 34675, `IOF_EMBED_XMP` → tag 700 |

Files that would exceed classic TIFF's 32-bit offsets give ERR_LIMIT; IO_CANCEL is polled per strip.
LZW output is libtiff-compatible (MSB first, Clear first, early change, Clear at code 4094, EOI;
hash with generation-stamped slots). Output verified with our decoder (exact), WIC, ffmpeg and
libtiff (exact).

## 4. CCITT (tiff_ccitt.s)

`ccitt_decode(src, len, dst, prm)` with a parameter block (width, rows, row bytes, compression,
T4/T6 options, a work buffer owned by the caller). 13-bit run-length lookup tables (white /
black, built once per decode from the T.4 code lists) and a 7-bit mode table; changing-element
lists per row (≤ 2·width + 16 entries, bounded). Behaviour follows libtiff's tif_fax3.c: output
bit 1 = black run (so BlackIsZero images come out inverted, like libtiff and WIC); Modified
Huffman rows byte aligned; T.4 rows start with an EOL (searched when none was seen, fill bits
skipped), T4Options bit 0 = 2D with the tag bit; T.6 all 2D; premature EOL → rest of the row
white; width overruns clipped; uncompressed mode / invalid codes → ERR_CODE; data ending early →
the row is completed white, remaining rows white (ERR_TRUNC when not even one row was complete).
All variants (RLE, T.4 1D / 2D / 2D + fill bits, T.6, FillOrder 1 / 2, both photometrics, widths
1..1728) are bit-identical to libtiff.

## 5. Deviations (documented, tested with tolerances)

* WIC reads 32-bit **integer** grey as float and 16-bit **float** grey as integers; half-float
  alpha is sRGB-encoded and half +inf becomes 0 in WIC — we decode the samples as declared.
* WIC ignores ExtraSamples = 0 on 2-sample grey (we treat it as alpha, as specified).
* Associated 8-bit alpha: WIC unpremultiplies with `((255 << 16) / a · c) >> 16` (white at
  c = a gives 254); we use the library's exact formula (±1).
* CMYK and out-of-gamut CIELab: WIC is colour-managed; we follow libtiff / the plain formula.
* Uncompressed / LZW YCbCr: WIC cannot read it; libtiff mis-decodes partial 4 × 4 blocks
  (`putcontig8bitYCbCr44tile` skew) — we follow the TIFF block layout (checked against a model).
* libtiff rounds 16-bit samples and shifts ColorMap entries (`>> 8`); we follow WIC.

## 6. Tests (`python tools/tiff_check.py --target both --out build/ct`)

| Suite | win | linux | What |
|---|---|---|---|
| gen | 472 | 438 | Python-written files (every photometric / depth / sample format / alpha kind × strips / tiles / planar / II-MM / BigTIFF / all compressions / predictors / FillOrder 2 / special layouts) vs a numpy model of the rules; the model cross-checked with WIC on 34 files |
| wic | 42 | – | WPF TiffBitmapEncoder: None / Ccitt3 / Ccitt4 / Lzw / Rle / Zip × BlackWhite / Gray8 / Gray16 / Bgra32 / Pbgra32 / Rgba64 / Prgba64 / Cmyk32 / Bgr24 / Rgb48 vs WIC decode (CMYK vs libtiff) |
| ffmpeg | 84 | – | 17 pix_fmts × raw / packbits / lzw / deflate, -dpi 300 (IM_DPIX checked) vs WIC; YUV vs a block model |
| libtiff | 208 | – | libtiff-written CCITT (all variants), JPEG (YCbCr 2×2 / 2×1 / 1×1, odd size, RGB, grey, quality 30, tiles), 16-bit predictor, CMYK, planar, 4-bit palette vs TIFFReadRGBAImage |
| encode | 713 | 323 | all bit depths × compressions × 8 test images (incl. PARGB / XRGB input, 1×1, multi-strip): file structure, exact round trip, WIC / ffmpeg / libtiff (win), dither levels, ICC / XMP / dpi through IO_META |
| meta | 57 | 57 | IMGMETA fields, orientations 1..8 (applied / IMGD_NO_ORIENT), blobs, EXIF block (II / MM / BigTIFF → exif_read), pages, IFD loops, probe, img_format_info |
| errors | 68 | 68 | unsupported variants, malformed / truncated files, huge counts, ID_MAXPX, cancel |
| par | 26 | 20 | ≥ 1 MP files through the parallel decoder (strips, tiles, planar, float, YCbCr, LZW, CCITT, JPEG tiles), identical with --threads 1, error of the lowest failing strip, cancel |
| fuzz | 2288 | 2288 | mutated files (all compressions incl. CCITT / JPEG / EXIF / pages / BigTIFF) + structural IFD mutants; a 20 000-case run (20 288 records) passed on both targets too |
| perf | 5 | 5 | 12 MP RGB decode (win / linux): LZW + predictor 19 / 20 ms, Deflate 27 / 50 ms, Deflate + predictor 24 / 63 ms, uncompressed 15 / 13 ms, single-strip Deflate 153 / 198 ms, single-strip LZW without predictor (ffmpeg file, 36 MB of codes) 119 / 184 ms — all < 250 ms; encode 12 MP LZW + predictor ≈ 260–290 ms |

`python build.py check --out build/ct`: 0 findings in the TIFF files (4 sentinel-terminated table
walks annotated). `python build.py test`: all scripts pass except `PaintCraft: skeleton.uis` and an
asmcheck COPYLOOP error in `apps/efftest/src/main.s` (other agents' work in progress).
Memory-safety review (x86-memory-safety-review) of all CT files: two findings fixed during the
review — the LZW encoder's output reserve was below the worst case with Clear codes for rows over
~170 KB (now 1.5n + n/1024 + 64), and outputs over 4 GB would have wrapped the 32-bit strip /
IFD offsets (now ERR_LIMIT); one development bug found by the tests (a 64-bit `imul` reading a
u32 field) fixed before release.

## 7. Notes for the integrator

* `tools/imgmeta_check.py` (C0) still expects TIFF (and TGA / DDS / PDN) to be stubs: suites
  fmtinfo ("stub claims decode/encode", filter lists), decex ("stub tiff") and encode ("stub
  encoder 5", "convert-ex -> .tif should fail") must be updated.
* `inflate_raw` allocates and frees its ~10 KB state per call (one mmap / VirtualAlloc per
  strip); an additive `inflate_raw_st(..., state)` would help many-strip files, especially under
  parallel decoding on Linux (munmap TLB shootdowns).
* Not implemented: writing EXIF (IOF_EMBED_EXIF is not in IC_IOM), BigTIFF / multi-page writing,
  old-style JPEG, LogLuv / LERC / LZMA / ZSTD / WebP-in-TIFF.
