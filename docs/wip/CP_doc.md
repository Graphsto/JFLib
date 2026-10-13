# CP — paint.net .pdn (pdn.s, nrbf.s): confirmed format, API, tests

Agent CP. Files: `ASMLIB/src/img/pdn.s`, `ASMLIB/src/img/nrbf.s`, `apps/codectest/src/pdn_ops.s`,
`tools/pdn_check.py` (independent reader + checks), `tools/pdn_template.py` (template extractor),
`build/cp/`. Everything below section 1 was verified on the five paint.net 5.1.12 samples in
`booksAndContext/PaintCraft/` (savedWithVersion 5.112.9563.32325) with `tools/pdn_check.py --dump
--graph` before any assembly was written.

## 1. File layout (confirmed)

```
"PDN3"                      4 bytes magic
n                           u24 little endian: byte length of the XML header
XML header                  n bytes UTF-8, no BOM, no newline:
  <pdnImage width="W" height="H" layers="N" savedWithVersion="5.112.9563.32325"><custom>
  <thumb png="BASE64" /></custom></pdnImage>                        (one line in the file)
00 01                       marker: an uncompressed MS-NRBF stream follows
                            (legacy: 1F 8B = gzip of everything after the header;
                             no "PDN3" at all = paint.net <= 2.0, the whole file is gzip)
MS-NRBF stream              .NET BinaryFormatter, ends with MessageEnd (0x0B)
deferred MemoryBlocks       one payload per MemoryBlock with deferred = true, in the order the
                            MemoryBlock objects appear in the NRBF stream (= layer order)
```
Nothing follows the last payload (0 trailing bytes in every sample).

Thumbnail: PNG written by WIC (sRGB, gAMA, pHYs chunks; RGB8 for opaque images, a 1x1 image
is a 4-bit palette PNG with an eXIf chunk). Size: the longer side is 256, the other
floor(side * 256 / longer) (800x600 -> 256x192, 4096x4097 -> 255x256, 852x480 -> 256x144);
images <= 256 keep their size (1x1 -> 1x1). The thumbnail is the flattened image (mean
|diff| 1..4 against a box-filtered composite of our reader).

### 1.1 Deferred MemoryBlock payload
```
u8   format                 0 = every chunk is a gzip member, 1 = raw chunks
u32  chunk size, BIG endian 0x00040000 = 262144 in every sample
chunks, ceil(length64 / chunk size) of them, in ANY order (ascending in the samples):
  u32 BE chunk index        0 .. count-1, each exactly once
  u32 BE data size          bytes that follow
  data                      gzip member (format 0) or raw bytes (format 1) that decompress to
                            exactly min(chunk size, length64 - index * chunk size) bytes
```
gzip members written by paint.net: `1F 8B 08 00 00000000 00 0A` (FLG 0, MTIME 0, XFL 0,
OS 10), deflate, CRC32, ISIZE. Pixel data = the Surface rows, stride = width * 4, straight
(non-premultiplied) BGRA = FMT_ARGB.

### 1.2 NRBF object graph of paint.net 5.1 (a_1x1.pdn; ids as written)
```
SerializationHeader root 1, header -1, version 1.0
BinaryLibrary 2  "PaintDotNet.Data, Version=5.112.9563.32325, Culture=neutral, PublicKeyToken=null"
#1 PaintDotNet.Document (ClassWithMembersAndTypes, lib 2)
     isDisposed Boolean | layers PaintDotNet.LayerList (lib 2) | width Int32 | height Int32 |
     savedWith System.Version (SystemClass) | userMetadataItems KeyValuePair`2[String,String][] (SystemClass)
#3 PaintDotNet.LayerList: parent Document (-> #1) | ArrayList+_items Object[] | ArrayList+_size Int32 |
     ArrayList+_version Int32 (= number of layers in the samples)
#4 System.Version (SystemClassWithMembersAndTypes): _Major 5, _Minor 112, _Build 9563, _Revision 32325
#5 BinaryArray single, rank 1, element SystemClass "System.Collections.Generic.KeyValuePair`2[[...]]":
     inline structs, id < 0: first SystemClassWithMembersAndTypes (key String, value String),
     then ClassWithId; key / value BinaryObjectString
#7 ArraySingleObject (the ArrayList buffer, capacity 4 / 8 / 16 ...): MemberReference per layer,
     then ObjectNullMultiple256 for the unused slots
BinaryLibrary 21 "PaintDotNet.Core, Version=5.112.9563.32325, Culture=neutral, PublicKeyToken=null"
#20 PaintDotNet.BitmapLayer (lib 2): properties BitmapLayer+BitmapLayerProperties | surface
     PaintDotNet.Surface (lib Core) | Layer+isDisposed Boolean | Layer+width Int32 |
     Layer+height Int32 | Layer+properties PaintDotNet.Layer+LayerProperties
#22 BitmapLayer+BitmapLayerProperties: blendOp PaintDotNet.UserBlendOps+NormalBlendOp (lib 2)
#23 PaintDotNet.Surface (lib Core): width, height, stride (Int32), scan0 PaintDotNet.MemoryBlock (lib Core)
#24 Layer+LayerProperties: name String | userMetadataItems KVP[] | visible Boolean |
     isBackground Boolean | opacity Byte | blendMode PaintDotNet.LayerBlendMode (lib 2)
     blendMode is an inline struct (id < 0): ClassWithMembersAndTypes "PaintDotNet.LayerBlendMode",
     one member value__ Int32
#25 PaintDotNet.UserBlendOps+NormalBlendOp: no members (one object per layer)
#26 PaintDotNet.MemoryBlock (lib Core): length64 Int64 (= stride * height), hasParent Boolean
     (false), deferred Boolean (true)
#28 KVP[] of the layer (empty)
MessageEnd
```
Object ids follow .NET's writer exactly (reproduced by our writer, see 3.3): every object
reference consumes the next id (an already known object too), inline value types get the
negated next id, library ids come from the same counter when a class record needs them;
objects are written breadth first (FIFO): Document, LayerList, Version, doc metadata array,
layer array, BitmapLayers, then per layer BitmapLayerProperties / Surface / LayerProperties,
then per layer NormalBlendOp / MemoryBlock / (new) metadata array. The second and later
instance of a class is a ClassWithId record. All layers share ONE empty metadata array
(c_layers: #44 referenced 5 times) - paint.net uses the same empty-array instance.

### 1.3 Metadata and DPI
There are no dpuUnit / dpuX / dpuY members in 5.1 (nor a colour profile member): resolution is
EXIF metadata in `Document.userMetadataItems`, items `("$exif.tagN[0]", "<exif id=\"ID\"
len=\"L\" type=\"T\" value=\"BASE64\" />")` (N = an index, not contiguous: tag3..6 in a new
image, tag0 / tag4..6 in e_photo; ID = EXIF tag, T = EXIF type, value = the raw little-endian
bytes). Every sample has 305 Software "Paint.NET 5.1.12\0" (ASCII), 296 ResolutionUnit
SHORT 2 (inch), 282 / 283 X / YResolution RATIONAL 96/1. e_photo (from a JPEG without EXIF / ICC)
has the same four items only. Layer metadata arrays are empty in all samples.

### 1.4 Layer properties seen in c_layers.pdn (5 layers, 800x600)
| # | name (UTF-8) | visible | isBackground | opacity | blendMode | blendOp |
|---|---|---|---|---|---|---|
| 0 | Hintergrund | 1 | 1 | 255 | 0 | NormalBlendOp |
| 1 | Ä | 1 | 0 | 255 | 0 | NormalBlendOp |
| 2 | ö | 0 | 0 | 255 | 0 | NormalBlendOp |
| 3 | ü | 1 | 0 | 128 | 0 | NormalBlendOp |
| 4 | ß | 1 | 0 | 128 | 0 | NormalBlendOp |

**The sample contains no Multiply / Overlay layer** (every blendMode value__ is 0 and every
blendOp is NormalBlendOp), so the LayerBlendMode numbering could not be confirmed from a file.
Layer 0 is the opaque white background; layers 2 and 3 are empty. The composite of our
reader matches the embedded thumbnail (see 1).

`apps/PaintCraft/tests/fixtures/pdn/Unbenannt1.png` (800x600, "Hallo" + rectangles) is not the flattened
image of any sample (max |diff| 255 against b_basic and c_layers), so there is no exported
reference composite; the composite is checked against the thumbnails and our formula instead.

### 1.5 Blend modes: file enum <-> library BM_*
LayerBlendMode (paint.net 4.x / 5.x, inline struct `value__`) and the 3.x UserBlendOps class
names use the order of the paint.net UI and the feature specification:

| value | paint.net (EN / DE UI; DE unverified except Multiplizieren, Überlagern) | UserBlendOps class (3.x) | PDNB_* = BM_* |
|---|---|---|---|
| 0 | Normal / Normal | NormalBlendOp | 0 NORMAL |
| 1 | Multiply / Multiplizieren | MultiplyBlendOp | 1 MULTIPLY |
| 2 | Additive / Additiv | AdditiveBlendOp | 2 ADDITIVE |
| 3 | Color Burn / Farbig nachbelichten | ColorBurnBlendOp | 3 COLORBURN |
| 4 | Color Dodge / Farbig abwedeln | ColorDodgeBlendOp | 4 COLORDODGE |
| 5 | Reflect / Reflektieren | ReflectBlendOp | 5 REFLECT |
| 6 | Glow / Leuchten | GlowBlendOp | 6 GLOW |
| 7 | Overlay / Überlagern | OverlayBlendOp | 7 OVERLAY |
| 8 | Difference / Differenz | DifferenceBlendOp | 8 DIFFERENCE |
| 9 | Negation / Negation | NegationBlendOp | 9 NEGATION |
| 10 | Lighten / Aufhellen | LightenBlendOp | 10 LIGHTEN |
| 11 | Darken / Abdunkeln | DarkenBlendOp | 11 DARKEN |
| 12 | Screen / Negativ multiplizieren | ScreenBlendOp | 12 SCREEN |
| 13 | Xor / Xor | XorBlendOp | 13 XOR |

The mapping is the identity (table `pdn_bm_map` in pdn.s, used in both directions); values
outside 0..13 decode as Normal. **Only value 0 is confirmed by a real file** (1.4); 1..13 follow
the documented order (the independent reader pypdn uses the same numbering) and must be
confirmed with `build/cp/for_user/cp_blendmodes.pdn` (layer names say which mode each layer
must show). The feature specification lists exactly these 14 modes; no further 5.1 modes are
known (Overwrite is a tool mode, BM_OVERWRITE is never written). In 3.x files the mode comes
from the blendOp class name when LayerProperties has no blendMode member.

What paint.net writes into `BitmapLayerProperties.blendOp` for a non-Normal layer is unknown
(every sample layer is Normal). Default of our writer: NormalBlendOp for every layer (the class
paint.net 5.1 demonstrably writes and reads) + the mode in LayerProperties.blendMode.
`pdn_encode_ex(..., PDNE_BLENDOP)` instead writes the matching UserBlendOps class (with its own
BitmapLayerProperties class record, as .NET does when member runtime types differ) --
`cp_blendmodes_blendop.pdn` tests whether paint.net 5.1 accepts that too.

## 2. Variants handled by the decoder (not in the samples, by member name with fallbacks)
* `PDN3` + header + `1F 8B`: gzip-compressed NRBF (+ payloads), and bare gzip files (<= 2.0).
* Document: `userMetadataItems` (KVP[]) or `userMetaData` (3.x NameValueCollection: Keys /
  Values arrays, values ArrayList or String).
* LayerList: `ArrayList+_items` / `_items`, `ArrayList+_size` / `_size`.
* Layer properties: `Layer+properties` / `properties` of class `...LayerProperties`;
  blend mode `blendMode` (struct value__ or plain Int32) else the blendOp class name.
* MemoryBlock: deferred (payloads) or inline `chunk0..chunkN` / `pointerData` byte arrays;
  `hasParent = true` -> ERR_UNSUPPORTED.

## 3. API (pdn.s, nrbf.s; constants in img.inc, CP block)

### 3.1 Documents
```
pdn_decode(data, len, doc, dec) -> eax 0 / ERR_*        .pdn -> PDNDOC (dec may be 0)
pdn_decode_ex(data, len, doc, dec, flags) -> eax        PDND_PROBE (1): size / resolution /
                                                        metadata only, PD_N = 0
pdn_encode(doc, opt, &buf, &len) -> eax 0 / ERR_*       PDNDOC -> file (mem_alloc'd)
pdn_encode_ex(doc, opt, &buf, &len, flags) -> eax       PDNE_BLENDOP (img.inc)
pdn_free(doc)                                           frees everything the document owns
pdn_flatten(doc, sf) -> eax 0 / ERR_*                   new FMT_ARGB composite of the visible
                                                        layers (k_bm_row, opacity, blend mode)
pdn_probe / pdn_decode_flat / pdn_encode_flat           IMGCODEC entries (pdn_codec)
```
PDNDOC / PDNLAYER as declared by C0 (unchanged): PD_W / PD_H / PD_N, PD_UNIT (PDU_PIXEL 1 /
INCH 2 / CM 3 = EXIF ResolutionUnit 1 / 2 / 3), PD_DPUX / PD_DPUY (f64 dots per unit, from
EXIF 282 / 283; default inch 96), PD_LAYERS (bottom first), PD_NVC / PD_NVCLEN (document
metadata items as `{u32 klen, key, u32 vlen, value}*`, UTF-8, file order), PD_META (IMGMETA:
IM_FMT = IMGF_PDN, size, IM_DPIX / IM_DPIY 16.16 dpi, IM_RESUNIT, IM_ICC from EXIF 34675),
PD_XML / PD_XMLLEN (the file's XML header). PDNLAYER: PL_SF (FMT_ARGB, owned), PL_NAME (UTF-8,
NUL-terminated), PL_OPACITY, PL_VISIBLE, PL_BLEND (PDNB_* = BM_*), PL_ISBG, PL_META / PL_METALEN
(layer metadata items, same blob format).

Decoding errors: ERR_TRUNC, ERR_HDR (not a .pdn / malformed graph / missing members / wrong
surface size), ERR_CHUNK (a chunk decompresses to the wrong size), ERR_CHECKSUM (gzip CRC),
ERR_DIMS, ERR_LIMIT (ID_MAXPX, ID_MAXMEM = table budget and n * w * h * 4 layer bytes, NRBF
limits), ERR_UNSUPPORTED (MemoryBlock with a parent), ERR_CANCEL (ID_CANCEL polled per layer and
per chunk), ERR_NOMEM. The input data is never modified.

Encoding: layers of any surface format (ARGB, PARGB, XRGB, A8 via img_get_row) with the document
size; IO_EFFORT 1..9 = deflate level of the chunks (0 = 1, the fastest); IO_CANCEL polled per
chunk. Metadata written: every PD_NVC item in order, except EXIF 296 / 282 / 283 whose values
are regenerated from PD_UNIT / PD_DPU* (existing items keep their key; missing ones are appended
as `$exif.tag<max+1>[0]`), and EXIF 34675 from PD_META's ICC profile when there is one. A new
document therefore gets `$exif.tag0..2` = 296 / 282 / 283 (paint.net's own new images also
carry 305 Software, which we do not claim). Rationals: 96.0 -> 96/1, 118.1102 -> 1181102/10000
(4 decimals, trailing zeros removed; values outside (0, 400000] -> 96/1).

img_encode (pdn_encode_flat): one visible background layer named "Background", resolution from
IO_META IM_DPIX / IM_DPIY (inch), ICC profile with IOF_EMBED_ICC. img_decode_ex
(pdn_decode_flat): the composite (FMT_ARGB; the dispatcher premultiplies unless IMGD_STRAIGHT),
IM_W / IM_H / IM_DPIX / IM_DPIY / IM_RESUNIT, IM_ICC with IMGD_ICC, IMGD_PROBE without pixels.
pdn_codec: IC_CAPS = IFC_DECODE | IFC_ENCODE | IFC_LAYERS, IC_IOM = IOM_EFFORT | IOM_ICC.

### 3.2 MS-NRBF reader (nrbf.s)
```
nrbf_parse(data, len, maxmem, &nb) -> eax 0 / ERR_*   one stream, header .. MessageEnd
nrbf_free(nb)   nrbf_end(nb) -> rax offset after MessageEnd   nrbf_root(nb) -> eax index
nrbf_nobj(nb) -> eax             objects in stream order (inline records included)
nrbf_obj(nb, ix) -> rax NRO_*    nrbf_class_name(nb, ix) -> rax, rdx   nrbf_is(nb, ix, name)
nrbf_member(nb, ix, name) -> rax NRV_* slot   nrbf_get_obj / nrbf_get_int / nrbf_get_str
nrbf_str(nb, ix)   nrbf_count(nb, ix)   nrbf_elem(nb, ix, i)   nrbf_elem_obj(nb, ix, i)
nrbf_prim_data(nb, ix) -> rax data, edx NRT_*   nrbf_visit(nb, ix) -> eax 1 first visit
```
Records 0..17 (header, ClassWithId, the four class-with-members forms, BinaryObjectString,
BinaryArray (all 6 kinds, rank <= 8, lower bounds skipped), MemberPrimitiveTyped,
MemberReference, ObjectNull(Multiple(256)), MessageEnd, BinaryLibrary, ArraySinglePrimitive /
Object / String); 21 / 22 -> ERR_UNSUPPORTED, others ERR_HDR. Tables: objects (NRO_*, 32 B),
value slots (NRV_*, 16 B; member values and array elements), classes, member descriptions,
libraries, id hash (open addressing, <= 50 % load). Limits (img.inc): depth 64, 1 M objects,
4 M value slots, 64 K classes, 1 M member descriptions, 4096 libraries, 65536 members per
class, optional byte budget `maxmem`; every length / count is checked against the remaining
data before anything is allocated (null runs only fill slots of an array already bounded by
NRBF_MAXVAL). Ids are unique (ERR_HDR on duplicates); references are resolved after parsing by
one hash lookup each (dangling -> ERR_HDR); cycles are legal (Document <-> LayerList) and
harmless because consumers only follow fixed member paths. Strings / names point into the data.

### 3.3 NRBF writer (nrbf.s) and the template (tools/pdn_template.py)
`nw_init(nw, libs)`, `nw_free`, `nw_take(nw, &len)`, `nw_reserve(nw, n)`, `nw_bytes / nw_u8 /
nw_i32 / nw_be32 / nw_i64 / nw_lps`, `nw_id` (every reference consumes an id, like .NET),
`nw_header(nw, root)`, `nw_class(nw, tpl, id)` (first instance of a template slot: the full
class record + BinaryLibrary records not yet written, library ids patched in; later instances:
ClassWithId), `nw_str`, `nw_ref`, `nw_nulls` (ObjectNull / NullMultiple256 / NullMultiple),
`nw_end`. Errors stick in NW_ERR. `python tools/pdn_template.py [--src FILE]` copies the class
records, library names, the KeyValuePair array element type and the savedWith version of a real
5.1 file into the generated block at the end of pdn.s (37 slots: Document, LayerList, Version,
KeyValuePair, BitmapLayer, Surface, LayerProperties, LayerBlendMode, BitmapLayerProperties x 14
(blendOp member type per mode), UserBlendOps x 14, MemoryBlock). pe_nrbf in pdn.s emits the
graph in .NET's breadth-first order with .NET's id numbering (section 1.2), so a re-encoded
paint.net file has a **byte-identical NRBF part** (tested on all five samples).

## 4. Implementation notes
* Decode: unwrap (PDN3 / legacy gzip, size from ISIZE bounded by 1032:1), nrbf_parse, Document
  members by name, metadata blob + EXIF resolution / ICC, px_payloads walks every deferred
  payload once (chunk count <= remaining bytes / 8, so bitmap and scratch sizes are bounded by
  the input), then per layer: properties, surface (w, h = document, w*4 <= stride <= w*4 + 64,
  length64 = stride * h), pixels: chunks in any order, each index once (bitmap), gzip_decode
  straight into the surface when the file stride equals the surface stride (width % 16 == 0),
  else through a scratch chunk and a row scatter.
* Encode: validation, generated metadata values, thumbnail (pdn_flatten, rs_resize RS_FANT to
  fit 256 x 256 with paint.net's floor rule, png_encode_ex), XML header, NRBF (template),
  payloads: per layer format 0, chunk 262144, chunks 0..n-1 as gzip members (gzip_encode; header
  bytes patched to XFL 0 / OS 10 as paint.net writes them), rows gathered from any surface
  format; a chunk of zeros (transparent area) is compressed once per file and reused.
* Every function is reentrant and keeps no global state (tables are .rodata): chunks are
  independent and can be spread over the thread pool (core/thread.s) later; the decoder's
  per-chunk loop (px_pixels) and the encoder's per-chunk loop (pe_payload) are the places to split.
* Frames < 4 KB (largest: pdn_encode_ex, 496 bytes of locals); nb_rec recursion <= 64 levels
  (about 180 bytes each).

## 5. Tests (tools/pdn_check.py; codectest ops 220..228 in apps/codectest/src/pdn_ops.s)
Ops: 220 pdn_decode_ex -> doc record, 221 pdn_encode_ex(doc record), 222 decode + encode,
223 nrbf_parse summary, 224 pdn_flatten, 225 decode benchmark, 226 synthetic encode benchmark
(+ decode and exact compare), 227 gzip_encode benchmark, 228 allocation benchmark.

Results (`python tools/pdn_check.py --target both --out build/cp --fuzz 3000`), win and linux:

| suite | cases | result |
|---|---|---|
| samples | 35 | every layer of the 5 samples bit-exact vs the Python reader; names, visibility, opacity, blend, isBackground, metadata items, XML, resolution; pdn_flatten and img_decode_ex (straight / premultiplied) equal to the Python composite; probes |
| roundtrip | 56 | paint.net's 5 files re-encoded: **NRBF byte-identical** to paint.net's, same payload layout, same thumbnail size, pixels / properties / metadata equal; 19 synthetic documents (5 layers with Unicode / empty / 300-char names, hidden, opacity 0 / 1 / 128, modes; 14 modes with and without PDNE_BLENDOP; cm / pixel units; metadata + ICC; PARGB / XRGB / A8 layers; 1x1, 300x2, 2x300, 600x520 multi-chunk; levels 1 / 3 / 6 / 9) parsed by the Python reader and decoded again by the library |
| flat | 5 | img_encode ARGB / PARGB, cancel flag, pixel limit |
| legacy | 2 | PDN3 + gzip, bare gzip |
| v3 | 5 | synthetic 3.x shape: NameValueCollection (ArrayList / string values), blendOp classes, inline chunk0..2, raw deferred payload |
| nrbf | 64 | structural mutants: empty / no header / two headers / missing root, bad 7-bit lengths, 2^31 strings / counts / arrays, null runs past the end / 0, rank 0 / 99, product > limit, duplicate and extreme ids, dangling / self / cyclic references, bad ClassWithId, unknown / duplicate library, record types 18..23 / 255, MethodCall / MethodReturn, bad binary / primitive types, Char / Decimal members, nesting 60 / 64 / 65 / 10 000, 1M + 1 objects, table budget; object counts of all samples equal to the Python reader |
| fuzz | 5353 (+ a 15 853-case run, seed 777) | truncations (every 7th byte of a_1x1, random cuts of c_layers), 3000 random mutations, 1500 NRBF i32 overwrites, 200 chunk header / data mutants, gzip garbage, 100 img_decode_ex mutants; inputs end at a page end (guard page): no crash, no hang |
| dotnet | 22 | Microsoft's BinaryFormatter (.NET Framework 4.8 via Windows PowerShell 5.1, Add-Type offline, binder mapping PaintDotNet.* to an ISerializable dump class) deserializes the NRBF part of all 5 samples, of our 5 re-encoded samples (identical results) and of all 7 user files; each stream is consumed completely |
| perf | 2 | d_big.pdn decode 136 ms (win) / 167 ms (linux); 4000x3000x10 encode 1.75 s (win) / 2.08 s (linux), 63 MB, decoded again and verified exact (single thread; with the machine loaded by other agents up to 5 s, hence best of 5 runs) |

App scripts: every apps/*/tests/*.uis passes on win and linux with this library (the PaintCraft
app opens / saves .pdn through pdn_decode / pdn_encode). `python tools/asmcheck/asmcheck.py pdn.s nrbf.s pdn_ops.s`: 0 errors, 0 warnings (annotations:
`ok BOUNDS` x 3 for the 10-digit decimal buffers, `assume rax in 0..13` x 3 after the blend-mode
clamps). `python build.py check --out build/cp` reports errors only in files of other agents.

Memory-safety review (x86-memory-safety-review checklist) of pdn.s / nrbf.s; findings fixed:
(1) pdn_b64dec clobbered its source pointer (rcx) in the bit extraction: read from a wild
address (crashed on the first sample, caught by the samples suite); (2) pdn_encode_flat read r8 /
r9 after a mem_zero call (UNINIT, asmcheck; only with opt = 0); (3) the legacy gzip size came
from ISIZE unbounded: an 18-byte file could request 4 GB; now limited to 1032 x the compressed
size. Invariants relied on: chunk count <= remaining / 8 for every deferred block (px_payloads),
which bounds the bitmap and scratch sizes; `index * chunk < total` and `expected = min(chunk,
total - offset)` for every write; file stride <= w * 4 + 64 and length64 = stride * h before any
pixel is written; the NRBF tables are re-read through their base pointers after every nested
nb_rec call (they move when they grow).

## 6. Files for the acceptance test in paint.net (build/cp/for_user/, written by the `user` suite)

| file | content / what to check |
|---|---|
| cp_3layers.pdn | 400x300, 4 layers: "Hintergrund" (background, checker), "Ebene Ä (versteckt)" hidden red circle, "Multiplizieren 50%" Multiply opacity 128 gradient, "Halbtransparent ß" green alpha-128 rectangle; 300 dpi |
| cp_blendmodes.pdn | background + 14 layers "N Mode = Name", layer N has blend mode N (default writer: NormalBlendOp + blendMode): the layer properties dialog must show the named mode |
| cp_blendmodes_blendop.pdn | the same with PDNE_BLENDOP (blendOp = UserBlendOps+<Mode>BlendOp) |
| cp_1001x777_alpha.pdn | 1001x777 (odd width: stride conversion), one layer with an alpha gradient, 118.1102 pixels/cm |
| cp_1x1.pdn | 1x1 red background |
| cp_c_layers_reencoded.pdn | the sample c_layers.pdn decoded and re-encoded by us (must look identical) |
| cp_img_encode_flat.pdn | img_encode of a surface (one layer "Background") |

Acceptance: each file opens without error; layers, names, visibility, opacity, blend modes and
the resolution (Image > Resize) as described; saving it again in paint.net and running
`python tools/pdn_check.py --dump` on it shows the same layers.

## 7. Changes to shared files
* `ASMLIB/inc/img.inc`: CP block after PDNB_* (PDND_PROBE, PDNE_BLENDOP, NRBF_MAX*, NRO_* / NRK_* / NROF_*,
  NRV_* / NRVK_*, NRT_*, NW_*). PDNDOC / PDNLAYER unchanged.
* Nothing else (imgio / imgreg / gzip / deflate untouched). The integrator should rerun
  `tools/gendoc.py` (new public functions in pdn.s / nrbf.s).

## 8. Open issues
* Blend modes 1..13 and the blendOp choice are unconfirmed until the user opens
  cp_blendmodes*.pdn in paint.net (section 1.5); if paint.net needs the matching blendOp
  class, flip the default in pe_layer_blend (pdn.s).
* No sample has an ICC profile or a cm / pixel unit: EXIF 34675 and ResolutionUnit 1 / 3 are
  implemented from the EXIF definitions, not confirmed by a paint.net file.
* The document EXIF items stay items; building an EXIF TIFF blob (IM_EXIF) for exports is not done.
* pdn_decode_flat keeps all layers in memory until the composite is done (a layer-streaming
  composite would lower the peak for img_decode_ex).
* Per chunk, gzip_encode / deflate allocate and clear about 600 KB of state (about 0.45 ms of the
  ~1.1 ms per chunk in the encode benchmark). A deflate entry point with a caller-owned reusable
  state (deflate.s, integrator) plus the thread pool would bring the 4000x3000x10 encode well
  below 1 s.
* Unbenannt1.png is not a composite of any sample; a PNG exported by paint.net from a file with
  real Multiply / Overlay layers would give an end-to-end check of the blend formulas.
* apps/PaintCraft (fileio.s, not CP's) drops PD_NVC / PD_META when it opens a .pdn, so document
  metadata (EXIF items, ICC) is lost on save; keeping PD_NVC / PD_META in the app document and
  passing them back to pdn_encode preserves it (pdn_encode already merges the resolution).
