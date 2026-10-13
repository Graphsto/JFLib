# F — runtime fonts (doc fragment for FONTS.md / DESIGN.md §3 and §7)

Agent F: `ASMLIB/src/text/*` (font.s, glyphcache.s, text.s + new ttf.s, cff.s, colr.s,
outline.s, fontdb.s, font_int.inc), `ASMLIB/inc/font.inc`, `apps/fonttest/**`,
`tools/font_check.py`, `tools/font_fixtures.py`.

## Runtime font loading (no system font API)

The library reads TrueType / OpenType files itself and feeds their outlines to the existing
rasterizer. Embedded SGF1 fonts keep ids 0–3 (`FONT_REG`, `FONT_SEMI`, `FONT_BOLD`,
`FONT_ICON`) and render exactly as before; every text API accepts any font id.

`.include "font.inc"` (after jfgui.inc) for the constants below. **Integrator:** please add
`.include "font.inc"` to jfgui.inc (it is guarded) and regenerate API.md.

| function | |
|---|---|
| `font_load_mem(data, len, face) -> eax id / ERR_*` | sfnt from memory; `data` must stay valid until `font_unload` |
| `font_load_file(path, face) -> eax id / ERR_*` | reads the whole file (owned, freed by `font_unload`) |
| `font_face_count(data, len) -> eax` | 1 for a plain sfnt, n for a TTC / OTC, else ERR_* |
| `font_unload(id) -> eax 0 / -1` | ids ≥ `FONT_EMBEDDED` only; also unloads `font_style` fonts derived from it; flushes the glyph cache; ids are reused later |
| `font_style(id, FS_*) -> eax id / ERR_*` | derived font: `FS_SYN_BOLD`, `FS_SYN_ITALIC`, `FS_SHARP`; the same (id, flags) returns the same id; works on embedded fonts too |
| `font_metrics(id, buf) -> eax 0 / -1`, `font_metric(id, FM_x) -> eax` | FONTMETRICS (below) |
| `font_name(id, FN_x) -> rax` | UTF-8, never 0: `FN_FAMILY` 1, `FN_STYLE` 2, `FN_FULL` 4, `FN_TYPO_FAMILY` 16, `FN_TYPO_STYLE` 17 (typographic names fall back to 1 / 2) |
| `font_is_color(id) -> eax` | COLR layers or CBDT bitmaps |
| `font_layers(id, glyph, buf, cap) -> eax` | COLR layers `{u32 glyph \| LAYER_FG, u32 argb}` (0 = not a colour glyph) |
| `font_kern_table(id, g1, g2) -> eax` | the 'kern' table value only (tests; `font_kern` prefers GPOS) |
| `glyph_box(id, glyph, box) -> eax 1 / 0` | control box, i32 font units |
| `font_count()`, `font_valid(id)` | registry size / validity |
| `sfnt_name(table, len, id, out, cap) -> eax` | name record → UTF-8 (German 0x0407 > German variants > English 0x0409 > other Windows > Unicode platform > Mac Roman) |
| `text_decor(x, y, w, TD_UNDERLINE \| TD_STRIKEOUT)` | underline / strikeout rectangles of the current font and size (≥ 1 px) |
| `gc_get_bitmap(id, glyph, size_fx) -> rax` | colour bitmap cache entry (PARGB) |
| `gc_box(id, glyph, size_fx, phase, out) -> eax` | mask placement as the cache computes it |

Unchanged APIs now valid for every font: `font_glyph`, `font_has`, `font_adv`, `font_kern`,
`font_upem`, `font_data` (for runtime fonts: the context, which starts with an SGF1-compatible
64-byte header: upem, asc, desc, gap, capH, xH, ulPos, ulThick), `glyph_path` (g_gp transform,
DX/DY honoured), `gc_get`, `gc_scale`, `text_font`, `text_draw*`, `text_measure*`,
`text_width*`, `text_hit`, `text_caret_x`, `text_ascent/descent/line_h`.
`font_register` (SGF1) is no longer limited to 8 fonts. Missing codepoints give glyph 0
(`.notdef`) for runtime fonts (SGF1 fonts keep their fallback glyph).

FONTMETRICS (`FM_SIZE` 80, i32 font units, y up): `FM_UPEM`, `FM_ASC/DESC/GAP` (hhea — what
text_ascent / text_descent / text_line_h use), `FM_TYPO_ASC/DESC/GAP`, `FM_WIN_ASC/DESC`,
`FM_XH`, `FM_CAPH` (OS/2 v2, else yMax of 'x' / 'H'), `FM_UL_POS/THICK` (post: top of the line),
`FM_ST_POS/THICK` (OS/2: top of the stroke), `FM_WEIGHT`, `FM_WIDTH`, `FM_NGLYPHS`,
`FM_ADVMAX`, `FM_FLAGS` (`FMF_ITALIC 1, BOLD 2, OBLIQUE 4, COLOR 8, CFF 16, VAR 32, SYMBOL 64,
MONO 128, TYPO 256, EMBED 512, SYN 1024, BITMAP 2048, KERN 4096`). Missing values get
defaults (hhea 0.8/−0.2 em, underline thickness upem/14 at −2×thickness, strikeout at half the
x-height).

### Supported tables (ttf.s, cff.s, colr.s)
* sfnt `0x00010000` / `true` / `OTTO`, collections `ttcf` (face index), `head` (upem 16..16384),
  `maxp`, `hhea`, `hmtx` (numberOfHMetrics clamped to the table), `loca` short/long.
* `glyf`: simple glyphs (all flag / repeat / short / same encodings), composites with offsets,
  scale, x/y scale, 2×2, point matching, SCALED_COMPONENT_OFFSET (Apple) vs default unscaled;
  nesting ≤ 16, ≤ 4096 components per glyph, ≤ 1 M points (`OL_MAX_PTS`).
  **Origin like GDI / FreeType:** outlines are shifted by `hmtx lsb − glyf xMin` (phantom point
  pp1; a USE_MY_METRICS component supplies it). Fonts with lsb = xMin (all Segoe UI glyphs) are
  unaffected.
* `CFF `: Type 2 charstrings — all path operators incl. flex / hflex / hflex1 / flex1, hints
  counted and skipped (hstem/vstem(hm), hintmask, cntrmask), callsubr / callgsubr with bias
  107 / 1131 / 32768, nesting ≤ 10, argument stack 48, transient array 32, arithmetic / storage
  operators, ≤ 200 000 operators per glyph; CID-keyed fonts (FDArray ≤ 256, FDSelect formats 0
  and 3, per-FD local subrs); FontMatrix applied unless exactly 1/upem (GDI does the same);
  width arguments dropped (advances from hmtx); seac-style endchar accents ignored.
  **Not supported:** CFF2 (such faces load with ERR_UNSUPPORTED), charstring type 1.
* `cmap`: formats 0, 4, 6, 12, 13; preference (3,10) fmt 12 > (0,x) fmt 12 > (3,1) fmt 4 >
  (0,x) fmt 4 > (3,0) symbol > fmt 6 / 0 > (1,0) Mac Roman. Symbol fonts additionally map
  U+0000..U+00FF and the Windows-1252 characters (€, ‚, …, œ) to U+F000 + code-page byte (GDI).
* Kerning: GPOS 'kern' feature (DFLT + latn default language systems, else the first script;
  PairPos formats 1 and 2, Extension lookups; first applicable subtable per lookup, lookups
  summed; x advance of the first glyph only) when present, else `kern` version 0 / Apple
  version 1 format 0 subtables (horizontal, not cross-stream; 16-bit length overflow handled).
* `OS/2`, `post`, `name`, `fvar` (flag only).
* Variable fonts: the default instance (gvar / HVAR / MVAR / avar ignored).
* Robustness: every table offset / length is clamped to the file, every read inside a table
  is bounds-checked (`t16`/`t32` helpers return −1), malformed glyphs come out empty, recursion
  and counts are limited. 1 760 mutated files (truncation, bit flips, header / table-targeted
  corruption) ran without a crash on Windows and Linux; 150 corrupted fontdb caches were all
  rejected.

### Rendering
* Outlines go through one emitter (`ol_emit`, outline.s) into the path API (float API +
  own cubic flattening with the path_cubic rule). TrueType contours are emitted with exactly the
  float operations of the SGF1 `glyph_path`, so **segoeui/seguisb/segoeuib/SegoeIcons.ttf loaded
  at run time give bit-identical glyph masks to the embedded fonts** (fonttest --cmpsgf: 28 480
  masks per weight, 13 520 icon masks, 0 differences; advances identical).
* Same hinting-free rules as SGF1: y-scale snapped to the x-height up to 24 px, quarter-pixel x
  phases. `FS_SHARP` ("Scharf") additionally snaps descender, baseline, x-height, cap height and
  ascender to whole pixels at every size (piecewise-linear y fitting; glyph boxes grow by
  upem/16 + 2 units vertically to hold the moved points).
* Synthetic styles (`font_style`): bold = FreeType `FT_Outline_EmboldenXY(upem/30)` (edges
  move out by upem/60, left / bottom edges stay, advance + upem/30), italic = shear
  x += y·tan 12°. Bold is applied before italic.
* Glyph cache key: `1<<63 | colour<<62 | font<<40 (22 bits) | phase<<36 | size_fx<<16 (20 bits:
  < 4096 px) | glyph (16 bits)` — unique for 4M font ids. Masks > 1024 px a side or
  > 256 K pixels, and sizes ≥ 4096 px, are not cached: `gc_get` returns `GC_DIRECT` (1) and
  text_run fills the outline directly (same placement as a cache entry).
* Colour glyphs (`text_draw` handles them; no separate API needed):
  * COLR v0 / CPAL palette 0: text_run draws the layers one by one through the normal A8 cache
    (layer colour = palette colour with alpha × the text colour's alpha; palette index 0xFFFF =
    the current text colour). COLR v1 fonts that still carry v0 base glyph records (Segoe UI
    Emoji) render in colour; pure v1 paint graphs are not supported (monochrome fallback).
  * CBDT / CBLC (index subtables 1..5, image formats 17 / 18 / 19): the PNG is decoded with
    `png_decode`, scaled to the text size (area average when shrinking, bilinear when enlarging)
    and cached as a PARGB entry (`gc_get_bitmap`, key bit 62); bitmaps > 512 KB live in one
    uncached heap block. Drawn with `k_blit_over` × text alpha × global opacity into XRGB /
    PARGB targets (not into A8 targets). Strike: smallest ppem ≥ size, else the largest.
  * No GSUB: emoji ZWJ sequences, skin-tone modifiers and flags render as their components
    (complex shaping is out of scope, see the plan).
  * `glyph_path` of a colour glyph gives its monochrome outline (apps that fill outlines
    themselves, e.g. paint's textool, stay monochrome; use text_draw for colour).

## fontdb (system font list)

| function | |
|---|---|
| `fontdb_scan(flags) -> eax faces` | `FDB_RESCAN`: ignore a loaded cache |
| `fontdb_family_count()`, `fontdb_family_name(i)` | families sorted case-insensitively |
| `fontdb_family_faces(i, &u32 count) -> rax` | `FDFACE` array (48 B: `FDF_PATH`, `FDF_STYLE`, `FDF_FULL`, `FDF_FAMILY`, `FDF_TFAMILY` pointers, `FDF_FACE` u32, `FDF_WEIGHT` u16, `FDF_FLAGS` u16 `FDFF_ITALIC 1, BOLD 2, OBLIQUE 4, CFF 8, COLOR 16, VAR 32, SYMBOL 64`) sorted by weight, then upright / italic |
| `fontdb_face_count()`, `fontdb_face(i)` | all faces in family order |
| `fontdb_find(family, weight, italic) -> rax FDFACE*, edx FS_SYN_*` | family by legacy name, else typographic name (case-insensitive); closest weight (ties: heavier for ≥ 500, lighter below), italic/oblique first; edx requests synthetic bold (weight ≥ 600 but face < 600) / italic |
| `fontdb_load(family, weight, italic) -> eax id` | find + `font_load_file` (+ `font_style`), cached per (face, style) until the next scan — do not `font_unload` these ids |
| `fontdb_save_cache(path)`, `fontdb_load_cache(path) -> eax 0 / -1` | |
| `fontdb_clear()` | |

* Folders: `jfgui_FONTDIRS` (`;`, on Linux also `:`), else Windows `%WINDIR%/Fonts` and
  `%LOCALAPPDATA%/Microsoft/Windows/Fonts`; Linux `/usr/share/fonts`, `/usr/local/share/fonts`,
  `$XDG_DATA_HOME/fonts` (else `~/.local/share/fonts`), `~/.fonts`. Recursive (depth ≤ 8,
  ≤ 20 000 files; Linux visits each directory once by device + inode: symlink loops).
  `.ttf .otf .ttc .otc` (≤ 64 faces per collection); only the table directory, `name`, `OS/2`
  and `head` are read (plat_file_open / read_at). Faces without glyf / CFF / CBDT (bitmap-only,
  CFF2) are skipped.
* Families group by the **legacy family name (name ID 1)**, as GDI and paint.net's list do
  ("Segoe UI Semibold" is its own family; each family has ≤ 4 R/B/I/BI styles typically).
  The typographic family is kept in `FDF_TFAMILY` (fontdb_find falls back to it).
  Duplicates (same family, full name, weight, style flags — e.g. a font installed twice or
  reached through symlinks) are listed once (the first found).
* Variable fonts are listed once (their default instance); GDI additionally lists named
  instances ("Bahnschrift SemiBold", "Sitka Banner", …) which cannot be rendered without gvar.
* Cache file (little endian): `"FDB1"`, u32 version 2, u32 files, u32 faces, u32 pool bytes,
  u64 FNV-1a hash of everything after this 28-byte header; files `{u32 path, u32 size, u32 first
  face, u32 faces}`; faces `{u32 file, u32 face index, u16 weight, u16 flags, u32 family, u32
  style, u32 full, u32 typo family, u32 0}`; pool of NUL-terminated UTF-8 strings. Validated
  completely on load (hash, offsets, counts, NUL at the end; 150 corrupted caches were all
  rejected); an entry is reused when path and size match (plat_dir_list reports no mtime); new
  / removed files are always picked up because the folders are listed on every scan.

## Tests

* `apps/fonttest` (run with `--shot x.png` so no window opens): `--dump F [face]`,
  `--outline F face CP|gGID`, `--outlines F face LIST OUT`, `--kern F face LIST OUT`,
  `--render F face SIZE TEXT OUT.png [--style FS] [--fg/--bg RRGGBB] [--decor TD]`
  (`F` = `@N` for embedded font N), `--cmpsgf F face SGFID`, `--fuzz F`, `--bench F face`,
  `--scan [OUT] [--cache F] [--rescan]`, `--find FAMILY W I`, `--selftest`.
* `apps/fonttest/tests/selftest.uis` (build.py test, both targets): known answers on synthetic
  fixture fonts (`tools/font_fixtures.py`, freely redistributable): TrueType (all glyf
  encodings, composites incl. nested scaled, lsb shift, cmap 4, kern, COLR/CPAL, German name
  records), CFF (width, all curve operators, hints + hintmask, local / global subrs, all flex
  variants, fixed-point operand, cmap 12, GPOS format 1 + format 2 through an extension
  lookup), CID-keyed CFF (FDArray with per-FD local subrs, FDSelect format 3), Mac Roman (1,0)
  cmap + Apple kern v1, TTC face 1, CBDT sizes, font_style ids / advances / unload of
  dependents, fontdb (families, dedupe, find, synthetic flags, load, cache round trip),
  truncated fixtures.
  The expected outlines come from an independent Python Type 2 / glyf evaluator.
* `tools/font_check.py [--all]`: GDI comparison on 38 installed fonts (TrueType, TTC faces,
  8 CFF .otf, variable Segoe UI Variable / Bahnschrift, Segoe UI Emoji, Symbol / Wingdings /
  Webdings, CJK MS Gothic / SimSun / Malgun, Segoe Fluent Icons): 9 669 outlines point by point
  (`GetGlyphOutlineW` GGO_NATIVE / GGO_BEZIER, unhinted, em size), cmap (`GetGlyphIndicesW`),
  advances, 'kern' pairs (`GetKerningPairsW`): **0 mismatches**. SGF1 identity (above).
  Families vs `EnumFontFamiliesExW`: 278 common, the 47 GDI-only names are named instances of
  variable fonts, 1 fontdb-only (MTEXTRA.TTF: in the Fonts folder but not registered).
  Linux (WSL): fontdb vs `fc-list` (16/16 files, names agree), Windows and Linux renders of the
  same file are byte-identical. Fuzzing (both targets) and CBDT / COLR pixel checks.

## Performance (this machine, Windows)
* Outline extraction: 0.6–0.9 µs per glyph (Segoe UI, Arial), 2 µs (CJK), 3 µs per CFF glyph.
* Rasterization into the glyph cache at 16 px: ~4–5 µs per glyph (9 µs CFF).
* `font_load_file`: dominated by reading the file (Arial 0.7 ms, MS Gothic TTC 9 MB 74 ms).
* `fontdb_scan` of C:\Windows\Fonts + user fonts (433 files, 456 faces, 280 families): cold
  (disk) 1.33 s, warm 50 ms, with a cache 4–5 ms.

## Limits / open points
* No GSUB (ligatures, contextual forms, emoji sequences) and no complex-script shaping.
* No TrueType bytecode hinting (by design); "Scharf" is vertical grid fitting only.
* COLR v1 paint graphs, sbix, SVG glyphs, CFF2 and variation instances other than the default
  are not supported.
* A TTC face loaded with `font_load_file` reads the whole collection file per face.
* Runtime fonts, the glyph cache and fontdb are UI-thread only (like the rest of text / gfx).
