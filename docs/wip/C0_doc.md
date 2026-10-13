# C0 — Codec infrastructure (phase P0): API reference and codec author guide

Doc fragment for `ASMLIB/docs/DESIGN.md` §4 / §7 and `GUIDE.md` (to be merged by the
integrator). Everything below is implemented, assembled on both targets and tested
(`tools/imgmeta_check.py`, see the end).

## 1. Files

| File | Owner after P0 | Content |
|---|---|---|
| `ASMLIB/inc/img.inc` | C0 / integrator | Public constants: IMGD_*, IMGDEC, IMGMETA, IMGOPT, IOF_*, IOM_*, IFI_* / IFC_* (format table), IMGCODEC, DDSF_*, MIPF_*, TIFC_*, PDNDOC / PDNLAYER, PDU_*, PDNB_*. Include guard (`.ifndef IMG_INC_INCLUDED`), constants only. |
| `ASMLIB/inc/img_int.inc` | C0 | Internal macros for codec authors (bit readers, byte order, CHECK_SPAN, clamps, CANCEL_POLL) + JSEG / TIFR / TENT / EXIFINFO records. Includes img.inc. |
| `src/img/imgio.s` | C0 | Classic API (unchanged) + dispatcher, detection, format table, registration, img_init. |
| `src/img/imgreg.s` | C0 | `img_use_all_codecs` (opt-in registration of every non-core codec). |
| `src/img/imgmeta.s` | C0 | Metadata records, limits, straight/premultiplied conversion (SSE2), orientation, sample-depth converters, sRGB tables, JPEG segment walker. |
| `src/img/imgcore.s` | **P1 core-format agent** | IMGCODEC glue for PNG / JPEG / BMP / GIF (descriptors, metadata scanners, `*_decode_ex`, `*_encode_opt`). |
| `src/img/exif.s` | C0 (shared with tiff.s) | TIFF IFD reader `tifr_*`, `exif_read` & helpers. |
| `src/img/icc.s` | C0 (colour management adds `icc_xform_*` later) | ICC header check, JPEG APP2 reassembly / splitting. |
| `src/img/gzip.s` | C0 | gzip members (RFC 1952). |
| `src/img/deflate.s`, `inflate.s` | codec | Additive only: `deflate_raw`, `deflate_raw_ex`, `deflate_init`, `inflate_raw_ex` (zlib paths unchanged). |
| stubs `tiff.s tga.s dds.s bc_dec.s bc_enc.s webp.s webp_vp8l.s webp_vp8.s avif.s isobmff.s yuv.s heic.s jxl.s jxr.s pdn.s nrbf.s` | later codec agents | Entry points returning ERR_UNSUPPORTED (probes 0); `tga_probe` (strict heuristic) and `pdn_free` are real. |
| `apps/codectest/src/meta_ops.s` | C0 | ops 240..259 + `ct_surface_in` / `ct_surface_out`. |
| `apps/codectest/src/{tiff,tga,dds,webp,avif,heic,jxl,jxr,pdn,png,bmp}_ops.s` | codec agents | stub `<x>_op` for their op range. |
| `tools/imgtest_common.py` | C0 (shared) | Test infrastructure for every `tools/<codec>_check.py`. |
| `tools/imgmeta_check.py` | C0 | Checks of this package. |

Needed change in a shared file (integrator): add `.include "img.inc"` at the end of
`jfgui.inc` (harmless thanks to the include guard; until then files include it explicitly).

## 2. Public API

All functions: Win64 ABI, return `eax` 0 or ERR_* (< 0) unless stated.

### 2.1 Decoding
```
img_decode_ex(data, len, sf, dec) -> eax     decode any registered format
img_load_ex(path, sf, dec) -> eax            file (ERR_IO) + img_decode_ex; ID_HINT 0 -> from the extension
img_probe(data, len, meta) -> eax IMGF / 0   detect + header scan (no pixels, no blobs)
img_detect(data, len, hint) -> eax IMGF / 0  magic only
img_meta_free(meta)                          free IM_ICC / IM_EXIF / IM_XMP (meta may be 0)
```
IMGDEC (`dec`, may be 0 = defaults): `ID_FLAGS` IMGD_*, `ID_FRAME` (page / frame / mip, 0 = first),
`ID_HINT` (IMGF from the extension), `ID_MAXPX` (u64, 0 = 2^28), `ID_META` (IMGMETA* or 0),
`ID_CANCEL` (u64 pointer to a volatile u32; ≠ 0 → ERR_CANCEL), `ID_MAXMEM` (u64 scratch budget, 0 = none).

Flags: `IMGD_STRAIGHT` (FMT_ARGB output, else FMT_PARGB), `IMGD_ICC` / `IMGD_EXIF` / `IMGD_XMP`
(copy blobs into IMGMETA), `IMGD_NO_ORIENT` (keep the stored orientation), `IMGD_PROBE` (header and
metadata only, no pixels), `IMGD_THUMB` (hint: a preview is acceptable).

Dispatcher behaviour: `sf` is cleared first; the IMGMETA (caller's or a local one) is cleared and gets
`IM_FMT`, `IM_ORIENT = 1`, `IM_FRAMES = 1` (the caller's record must not hold blobs); without a caller
IMGMETA the blob flags are dropped. After a successful decode: `IM_W/IM_H` default to the surface size,
the orientation is applied unless `IMGD_NO_ORIENT` or `IMF_ORIENT_APPLIED` (codec did it), and the
pixels are converted to the requested format. On any error the surface is empty and the blobs are freed.
Errors: ERR_UNSUPPORTED (unknown / unregistered / no decoder), ERR_TRUNC (< 2 bytes), ERR_LIMIT
(w·h > ID_MAXPX), ERR_DIMS (w or h 0 or > 65535), ERR_CANCEL, codec errors.

IMGMETA (`IM_SIZE` 128): `IM_FMT`, `IM_W`, `IM_H` (stored size, before orientation), `IM_BITS` (u8
bits per sample; 32 float; 16 + IMF_FLOAT = half), `IM_MODEL` (IMM_GRAY 1, RGB 2, PAL 3, CMYK 4, YCC 5,
LAB 6, YCCK 7), `IM_ORIENT` (u8 1..8), `IM_CHANNELS` (u8 incl. alpha), `IM_FLAGS` (IMF_ALPHA 1,
SRC_PREMUL 2, LOSSLESS 4, ANIMATED 8, HDR 16, ORIENT_APPLIED 32, FLOAT 64, INTERLACED 128, SRGB 256),
`IM_FRAMES`, `IM_DPIX/IM_DPIY` (16.16 dpi, 0 unknown), `IM_CICP` (prim | tf<<8 | mat<<16 | full<<24),
`IM_GAMMA` (gAMA·100000), `IM_ICC/ICCLEN`, `IM_EXIF/EXIFLEN` (TIFF structure without "Exif\0\0"),
`IM_XMP/XMPLEN`, `IM_DELAY` (ms), `IM_LOOPS`, `IM_QUALITY` (estimate, 0 unknown), `IM_RESUNIT`
(IMR_NONE / INCH / CM: unit of the stored resolution).

### 2.2 Encoding
```
img_encode(sf, opt, &buf, &len) -> eax       sf: FMT_XRGB / PARGB / ARGB / A8; *buf mem_alloc'd
img_save_opt(path, sf, opt) -> eax           img_encode + plat_file_write (atomic); IO_FMT 0 -> extension
img_opt_init(opt, fmt)                       IMGOPT with paint.net's defaults for fmt
```
IMGOPT (`IO_SIZE` 64; all-zero = defaults): `IO_FMT`, `IO_QUALITY` (1..100, 0 = default 95),
`IO_FLAGS` (IOF_LOSSLESS, RLE, MIPMAPS, SRGB, CUBE_CROSS, EMBED_ICC, EMBED_EXIF, EMBED_XMP, INTERLACE),
`IO_BITDEPTH` (0 auto / 32 / 24 / 8), `IO_SAMPLEBITS` (0 = 8 / 10 / 12), `IO_DITHER` (**level + 1**:
0 = default 7, 1 = off … 9 = 8), `IO_ATHRESH` (**threshold + 1**: 0 = default 128), `IO_CHROMA` (444 /
422 / 420 / 400 decimal, 0 default), `IO_EFFORT` (1..9, 0 default; PNG = deflate level), `IO_DDSFMT`
(DDSF_*), `IO_MIPFILTER` (MIPF_*), `IO_TIFFCOMP` (TIFC_*), `IO_META` (IMGMETA* to embed), `IO_CANCEL`.
The only paint.net default that is "on" for a flag is TGA RLE — use `img_opt_init` to get it.
Errors: ERR_UNSUPPORTED (format / encoder / surface format), ERR_DIMS (empty surface), ERR_CANCEL,
ERR_NOMEM, codec errors; ERR_IO from img_save_opt.

### 2.3 Format table and registration
```
img_format_info(fmt) -> rax IMGFMT* / 0      IFI_NAME, IFI_EXTS ("jpg;jpeg;jpe;jfif;exif=JPEG"),
                                             IFI_MIME, IFI_CAPS (IFC_*), IFI_IOM (IOM_*), IFI_CODEC
img_format_from_path(path) -> eax IMGF / 0   png; jpg jpeg jpe jfif exif; bmp dib rle; gif; tif tiff;
                                             tga; dds; webp; avif; heic heif; jxl; jxr wdp hdp wmp; pdn
img_filter_list(need, buf, cap) -> rax bytes ui_filedlg list of formats whose caps ⊇ need (0 if too small)
img_register(fmt, codec) -> eax              put an IMGCODEC into the table (0 removes)
img_use_all_codecs()                         (imgreg.s) register TIFF..PDN
img_init()                                   build lazily built tables; call before worker threads
```
`IFI_CAPS` = static format properties (IFC_ALPHA, LAYERS, ANIM, PAGES, MIPS, LOSSY, LOSSLESS, HDR, ICC,
EXIF, XMP, CORE) | the registered codec's `IC_CAPS` (IFC_DECODE / IFC_ENCODE when really implemented).
`IFI_IOM` tells the Save-Configuration dialog which IMGOPT fields to show (IOM_QUALITY, LOSSLESS, RLE,
MIPMAPS, BITDEPTH, SAMPLEBITS, DITHER, ATHRESH, CHROMA, EFFORT, DDSFMT, MIPFILTER, TIFFCOMP, SRGB, CUBE,
ICC, EXIF, XMP, INTERLACE). Core formats are registered statically (IFC_CORE); stubs have IC_CAPS 0.

Opt-in linking: imgio.s only references the core descriptors; the others are referenced from imgreg.s,
so `img_use_all_codecs` (or `img_register`) is what pulls a codec into an executable.

Detection order (img_detect): PNG signature; FF D8 FF; GIF87a/89a; "DDS "; RIFF….WEBP; II*\0, MM\0*,
II+\0, MM\0+; II BC (JPEG XR); `ftyp` at 4 → AVIF / HEIC by major brand, compatible brands, then the
primary item type (meta/pitm/iinf/infe v2/v3: av01 → AVIF, hvc1/hev1 → HEIC; a grid primary uses the
first coded item); FF 0A or the JXL container signature; "PDN3"; 1F 8B 08 (gzip → legacy .pdn); "BM"
with info header size 12/40/52/56/64/108/124 (or < 18 bytes: truncated BMP); TGA only when the hint is
TGA or the registered `tga_probe` accepts (strict header check, see tga.s).

### 2.4 Helpers (imgmeta.s)
```
img_meta_blob(meta, off, src, len) -> eax    copy into IM_ICC / IM_EXIF / IM_XMP (replaces)
img_dims_check(w, h, dec) -> eax             1..65535 (ERR_DIMS), w·h ≤ ID_MAXPX (ERR_LIMIT); leaf
img_mem_check(bytes, dec) -> eax             ≤ ID_MAXMEM (ERR_LIMIT); leaf
img_surf_alloc(sf, w, h, fmt, dec) -> eax    dims check + surf_alloc (ERR_NOMEM)
img_premultiply(sf) / img_unpremultiply(sf)  in place ARGB <-> PARGB
img_convert(sf, fmt) -> eax                  in place XRGB/PARGB/ARGB -> PARGB/ARGB
img_surf_convert(src, dst, fmt) -> eax       new PARGB/ARGB surface from any (incl. A8 -> grey)
img_get_row(sf, y, dst, fmt)                 row y of any surface as straight / premultiplied BGRA
img_orient(sf, orient) -> eax                EXIF 1..8 (5..8 reallocate; OPAC/BLEND/AUX kept)
img_u16_to_u8(v) -> eax                      round(v / 257) exact
img_row_u16_to_u8(dst, src, n, be)           SSE2
img_half_to_f32(h) -> eax                    exact incl. subnormals / inf / NaN
img_row_half_to_u8 / img_row_f32_to_u8       clamp 0..1, NaN -> 0, round
img_row_f32_to_srgb8(dst, src, n)            linear -> sRGB bytes (4096-entry table)
g_img_srgb_lin                               f32[256] sRGB byte -> linear
jpg_seg_next(data, len, js) -> eax marker    JPEG segment iterator (JSEG in img_int.inc)
```
Rounding: premultiply c' = round(c·a/255) (same as the PNG/GIF/BMP decoders); unpremultiply
min(255, (c'·ceil(255·2^24/a) + 2^23) >> 24) (same as png.s unpremul_row); premul(unpremul(P)) == P for
every valid P. The row kernels are private SSE2 copies; switch to G1's `k_premul_row` /
`k_unpremul_row` (gfx/straight.s) once they exist if they are bit-identical.

### 2.5 EXIF / TIFF reader (exif.s)
```
tifr_open(tr, data, len) -> eax              TIFR (TR_SIZE ~8 KB: heap!) on II/MM classic or BigTIFF
tifr_visit(tr, off) -> eax                   ERR_LIMIT on a revisit (cycle) or > 1024 IFDs
tifr_count(tr, off) -> rax n / ERR_TRUNC     entries clamped to what fits
tifr_ifd(tr, off) -> rax                     visit + count
tifr_entry(tr, off, i, te) -> eax            TENT {tag, type, count, voff, bytes}, span checked
tifr_find(tr, off, tag, te) -> eax 1 / 0
tifr_get(tr, te, i) -> rax                   integer value (signed types sign-extended)
tifr_rational(tr, te, i) -> rax num, rdx den
tifr_next(tr, off) -> rax                    next IFD offset (0 = none)
exif_read(blob, len, info) -> eax            EXIFINFO {orient, unit, dpix, dpiy, cspace, orioff}
exif_orientation(blob, len) -> eax 1..8
exif_set_orientation(blob, len, o) -> eax 1 patched / 0
exif_copy_orient1(blob, len, &out, &outlen) -> eax   copy with orientation 1 (for saving)
```
Callers keep sub-IFD nesting ≤ TIFR_MAXDEPTH (3). exif_read: IFD0 orientation / resolution
(unit default inch; cm × 2.54; unit 1 → 0), Exif IFD ColorSpace, Interop IFD visited, IFD chain
walked to its end / a cycle / the limit.

### 2.6 ICC (icc.s), gzip (gzip.s), raw deflate
```
icc_check(data, len) -> rax size / ERR_TRUNC / ERR_HDR / ERR_UNSUPPORTED   (v2..v5, 'acsp', tag table)
icc_jpeg_collect(data, len, &out, &outlen) -> eax 1 / 0 none / ERR_CHUNK / ERR_NOMEM
icc_jpeg_segments(icc, len, &out, &outlen) -> eax  complete APP2 segments, 65519 data bytes each, ≤ 255
gzip_decode(src, len, dst, dstcap, &consumed) -> rax bytes / ERR_*   one member; consumed incl. trailer
gzip_encode(src, len, level, &out, &outlen) -> eax                   XFL 2/4/0, OS 255, MTIME 0
gzip_isize(src, len) -> rax                                          last 4 bytes (single member)
deflate_raw(src, len, level, &outlen) -> rax buffer
deflate_raw_ex(src, len, level, &outlen, head, tail) -> rax          room for container headers
inflate_raw_ex(src, srclen, dst, dstcap, &consumed) -> rax bytes
deflate_init()                                                       (img_init calls it)
```
gzip errors: ERR_TRUNC, ERR_HDR (ID, CM, reserved flags), ERR_CHECKSUM (FHCRC, CRC32, ISIZE),
ERR_OVERFLOW, inflate errors. `gzip_decode` returns the byte count (like zlib_inflate), not just 0.

### 2.7 PDN document (declared, implemented by the PDN agent)
`pdn_decode(data, len, doc, dec)`, `pdn_encode(doc, opt, &buf, &len)`, `pdn_free(doc)` (implemented),
`pdn_decode_flat` / `pdn_encode_flat` (IMGCODEC entries for img_decode_ex / img_encode).
PDNDOC: PD_W, PD_H, PD_N, PD_UNIT (PDU_PIXEL 1 / INCH 2 / CM 3 = paint.net MeasurementUnit), PD_DPUX /
PD_DPUY (f64), PD_LAYERS (PDNLAYER[PD_N], bottom first), PD_NVC/LEN, PD_META (IMGMETA*), PD_XML/LEN.
PDNLAYER (64 B): PL_SF (FMT_ARGB surface), PL_NAME (UTF-8), PL_OPACITY, PL_VISIBLE, PL_BLEND (PDNB_*,
order to be confirmed from real 5.1 files), PL_ISBG, PL_META/LEN. Everything is owned by the document.

## 3. Codec author guide

**Your files.** One codec = one file group (e.g. `tiff.s` + `tiff_ccitt.s`), plus
`apps/codectest/src/<x>_ops.s` and `tools/<x>_check.py`. Start every source with
`.intel_syntax noprefix` / `.include "jfgui.inc"` / `.include "img_int.inc"`.

**Entry points and descriptor** (already stubbed; keep the names):
```
<x>_probe(data, len, meta) -> eax 1 / 0            fill IMGMETA scalars from the header; no allocation
<x>_decode(data, len, sf, dec) -> eax 0 / ERR_*
<x>_encode(sf, opt, &buf, &len) -> eax 0 / ERR_*
<x>_codec: .quad <x>_probe, <x>_decode, <x>_encode ; .long IC_CAPS, IC_IOM
```
Set `IC_CAPS` to `IFC_DECODE` / `IFC_ENCODE` when they work and `IC_IOM` to the IMGOPT fields you
honour — the UI reads them through img_format_info. Registration is already in imgreg.s.

**Decode contract.** `dec` is never 0 and normalised (ID_MAXPX set; ID_META → zeroed IMGMETA with
IM_FMT, IM_ORIENT 1, IM_FRAMES 1; blob flags only when the caller has an IMGMETA). Fill IM_W/IM_H and
whatever else you know. With `IMGD_PROBE` fill the meta and return 0 without touching sf. Check sizes
before allocating (`img_dims_check` / `img_surf_alloc`, `img_mem_check` for big scratch). Produce
FMT_ARGB or FMT_PARGB (prefer what IMGD_STRAIGHT asks for); report `IM_ORIENT` and leave it to the
dispatcher (or apply it yourself and set IMF_ORIENT_APPLIED). Store blobs with `img_meta_blob`. Poll
`CANCEL_POLL rDec, ID_CANCEL, tmp, .xx_cancel` in long loops; on any error free what you allocated
(the dispatcher frees sf and the blobs too). Unsupported frame index → ERR_UNSUPPORTED (frame past the
last one: ERR_LIMIT).

**Encode contract.** `opt` never 0 (copy owned by the dispatcher). Fetch rows with
`img_get_row(sf, y, rowbuf, FMT_ARGB)` (straight BGRA from any input format). Zero fields mean "format
default"; remember the biased IO_DITHER / IO_ATHRESH. Write metadata from `IO_META` when the
IOF_EMBED_* flag is set (EXIF: `exif_copy_orient1`, JPEG ICC: `icc_jpeg_segments`). Return the
mem_alloc'd file in *buf / *len.

**Memory safety.** Frames < 4 KB (PROLOG has no stack probes) — contexts on the heap. Every length
64-bit against the end (`CHECK_SPAN off, len, end, tmp, fail`); output sizes from the dimensions,
never from declared lengths; depth / count limits (ISOBMFF 16, NRBF 64 / 1 M objects, IFD chain 1024
via tifr_visit); entropy symbols used as table indices clamped or `# asmcheck: assume`d (the assume
applies *after* the annotated instruction); NaN → 0. asmcheck must report 0 errors.

**img_int.inc.**
- Bit readers with caller-chosen registers `buf, cnt, ptr, end, tmp` (64-bit, none of them rcx; REFILL
  clobbers rcx): MSB-first `BRM_INIT / REFILL / PEEK n / SKIP n / GET n / GETV nreg / OVERRUN`, LSB-first
  `BRL_*`. After REFILL ≥ 56 bits are available; past the end zeros are shifted in and `ptr` keeps
  counting, `BRx_OVERRUN cnt, ptr, end, tmp, fail` detects consumption beyond the data.
- `RD_BE16/24/32/64`, `RD_LE16/24/32/64 dst, base, off` (off without spaces, e.g. `rcx*2+4`),
  `WR_BE16 base, off, src16`, `WR_BE32 base, off, src32`.
- `CHECK_SPAN`, `CLAMP_U8 reg32, tmp32`, `CLAMP reg, lo, hi, tmp`, `CANCEL_POLL rec, off, tmp, fail`.
- The bit readers are verified by codectest op 254 kinds 7 / 8 (`mo_bits` in meta_ops.s is a usage
  example: constant-width GET / PEEK / SKIP, variable GETV, OVERRUN) against a Python model for
  random streams of 0..300 bytes.

**Error codes.** 0 OK, −1 TRUNC, −2 HDR, −3 BLOCK, −4 CODE, −5 DIST, −6 OVERFLOW, −7 CHECKSUM,
−8 NOMEM, −9 UNSUPPORTED, −10 CRC, −11 CHUNK, −12 DIMS (w/h 0 or > 65535), −13 IO, −14 CANCEL,
−15 LIMIT (pixels > ID_MAXPX, memory budget, nesting, counts, cycles).

**Test harness.** codectest (`python build.py app codectest --out build/<you>`) runs batch records
`{op, arg, data}`; op ranges: 20 JPEG, 40 GIF, 60 TIFF, 80 TGA, 100 DDS, 120 WebP, 140 AVIF, 160 HEIC,
180 JXL, 200 JXR, 220 PDN, 240 infrastructure (meta_ops.s), 260 PNG, 280 BMP. Implement `<x>_op(op,
arg, data, len)` in your `<x>_ops.s` (return 1 when handled, record results with `rb_record` /
`rb_put`; `ct_surface_in` / `ct_surface_out` convert {w, h, fmt, pixels} records). All codecs are
registered in codectest unless `--core-only` is passed. The generic op 240 (img_decode_ex with any
flags / hint / frame / maxpx / cancel) and op 252 (img_encode with any IMGOPT) already cover the
public paths — your own ops are for internals (bitstreams, block decoders, benchmarks).
`--convert-ex IN --to OUT [--straight] [--quality Q]` exercises img_load_ex + img_save_opt.

`tools/imgtest_common.py` (import from your checker): constants parsed from jfgui.inc / img.inc
(`it.C.ERR_LIMIT`, `it.C.IMGF_TIFF`, …), `Runner(target, out, extra=[...])` (Windows exe or the Linux
binary through WSL, crashing records isolated by bisection), `Rec`, `Suite`, `run_suite`, `report`,
`argparser` (`--target --out --no-build --only --fuzz --seed --no-ext`; `$jfgui_BUILD_DIR`),
`decex_input` / `parse_decex` / `expect_decex`, `surface_input` / `parse_surface`, `premul` /
`unpremul` (library-exact), `orient`, `psnr` / `max_abs` / `compare`, `ffmpeg_decode` /
`ffmpeg_encode` / `ffprobe_size`, `Wic` (batched PowerShell + WPF: `decode(path, frame)`,
`encode(bgra, "WmpBitmapEncoder", out, Lossless=True)`, `TiffBitmapEncoder` `Compression=Lzw`, …; one
PowerShell per batch; verified for JPEG XR / TIFF / PNG / JPEG), `wsl_python(code, stdin)` (python3 in
WSL: ctypes on libwebp.so.7 / libjxl.so.0.7 / libheif.so.1 / libdav1d.so.7 / libtiff.so.6), writers
`riff` / `riff_chunk`, `bmff_box` / `bmff_fullbox` / `ftyp`, `tiff_bytes` (classic / BigTIFF, IFD
chains, sub-IFD references), `exif_blob`, `jpeg_seg` / `jpeg_insert`, `png_chunk` / `png_make`, and
`mutate` for fuzzing. See tools/imgmeta_check.py for a complete example.

## 4. Notes for the P1 agents

- **PNG / JPEG / BMP / GIF (core upgrade):** you own imgcore.s now. Today the glue calls the classic
  decoders (PARGB) and the dispatcher unpremultiplies for IMGD_STRAIGHT — lossy for low alpha. Add
  native straight output (return FMT_ARGB when IMGD_STRAIGHT) and straight encoder input, then drop
  the premultiplied temporary in `core_encode`. Keep `png_decode` / `img_decode` / `img_save*`
  behaviour identical (codec_check, jpeg_check, gif_check). The metadata scanners (png_meta,
  jpeg_meta, bmp_meta, gif_meta) already give size / depth / model / flags / dpi / gAMA / sRGB / cICP /
  iCCP / eXIf / iTXt-XMP / acTL / JFIF / EXIF / Adobe / ICC / frames / loops / delays — move them next
  to the decoders if you like. JPEG: jpeg_dec applies EXIF orientation itself; the glue undoes it for
  IMGD_NO_ORIENT using `jpg_orient_compat` (a copy of jd_exif's rules) — a native "don't orient" flag
  in jpeg_dec would be cleaner. The PNG IHDR 16-bit expectation in codec_check must change when you
  round 16→8 (`img_u16_to_u8`). `IC_IOM` of each core codec must list what the encoder honours.
  Test ops: 260..279 (png_ops.s), 280..299 (bmp_ops.s), JPEG 20..39, GIF 40..59.
- **TIFF:** use exif.s `tifr_*` (BigTIFF included, cycle / count limits built in; keep sub-IFD depth
  ≤ 3), `ID_FRAME` = page, `IM_FRAMES` = page count. JPEG-in-TIFF needs `jpeg_decode_abbrev` from the
  JPEG agent. EXIF sub-IFD of TIFF files → IM_EXIF via img_meta_blob. Ops 60..79.
- **TGA + DDS:** tga.s already has the strict `tga_probe` img_detect relies on (keep it strict; the
  checker's detect suite has the positive / negative cases). DDS: `ID_FRAME` = mip level, cube maps
  as a cross, IO_DDSFMT + IOF_SRGB + IOF_MIPMAPS + IO_MIPFILTER, bc_dec.s / bc_enc.s entry points
  are stubbed. `img_row_half_to_u8`, `img_row_f32_to_u8` / `_srgb8` handle float formats. Ops 80..119.
- **PDN:** gzip.s (`gzip_decode` with consumed length for chunk lists, `gzip_isize`,
  `gzip_encode`), nrbf.s stub, pdn.s with PDNDOC / PDNLAYER in img.inc and a working `pdn_free`;
  img_detect reports "PDN3" and bare gzip (legacy) as IMGF_PDN; the flat entry points serve
  img_decode_ex / img_encode. Ops 220..239.
- Everyone: never `.include` jfgui.inc twice with macros of your own in shared .inc files without a
  guard; stubs keep every file assemblable — replace them in place.

## 5. Test status (C0)

- `python tools/imgmeta_check.py --target both --out build/c0`: all 12 suites pass on Windows and Linux
  (gzip 189, exif 67, icc 38, detect 77 + 76 core-only, probe 20, fmtinfo 127 + 127, decex 189,
  pixels 136, encode 43, fuzz 2333 per target; a 20 000-case fuzz run (23 333 records) passes too).
- Old checks unchanged and green: `tools/codec_check.py --target both` (sums, inflate, fuzz, deflate,
  png, pngenc, bmp, bmpenc, imgio), `tools/jpeg_check.py`, `tools/gif_check.py`.
- `python build.py check --out build/c0`: 0 errors (remaining warnings in C0 files are sentinel-
  terminated table walks).
- `python build.py test --out build/c0`: all scripts pass on both targets.
- Memory-safety review (x86-memory-safety-review checklist) of all C0 files: one confirmed finding,
  fixed — `bmff_primary_type` read the version byte of an `infe` box without payload that ends
  exactly at the end of the data (1-byte over-read); regression cases "empty infe / pitm / iinf /
  meta at the end" in the detect suite (guard page directly behind the data).
