# Colour management (agent I1)

ICC colour transforms in pure assembly: `src/img/icc_xform.s` (+ `inc/icc.inc`), the monitor
profile per platform (`src/plat/win/win_icc.s`, `src/plat/linux/lx_icc.s`), checker
`tools/icc_check.py`, test ops 320..326 in `apps/codectest/src/icc_ops.s`.
`src/img/icc.s` (C0) keeps the container helpers (`icc_check`, JPEG APP2).

## API (`.include "icc.inc"`)

```
icc_xform_build(src, slen, dst, dlen, intent, flags) -> rax handle | ERR_*
     src / dst = profile bytes, 0 = built-in sRGB.  intent ICC_PERCEPTUAL / _RELATIVE /
     _SATURATION / _ABSOLUTE; flags ICF_BPC (black point compensation), ICF_NOIDENT,
     ICF_LUT (tests: never identity / always the 3D LUT).  UI thread (allocates, uses the
     worker pool for LUT grids).
icc_xform_row(h, src, dst, n)   n straight BGRA pixels (8 bit), alpha copied, src == dst ok;
                                pure kernel: any thread, no allocation
icc_xform_free(h)               h <= 0 ignored
icc_xform_kind(h) -> eax        ICX_IDENTITY (rows copied) / ICX_MATRIX / ICX_LUT
icc_builtin(id, &len) -> rax    ICC_BI_SRGB, _ADOBE, _P3, _PROPHOTO, _GRAY22 (static bytes,
                                generated on the first call -- call it on the UI thread first)
icc_describe(icc, len, buf, cap) -> eax   'desc' text as UTF-8 (v2 desc; v4 mluc: German,
                                else English, else the first record); 0 = none
plat_monitor_icc(win, &buf, &len) -> eax 1 / 0   monitor profile (mem_alloc'd, icc_check'ed)
plat_monitor_key(win) -> rax    identity of the window's monitor (poll; re-query on change)
```
Errors: ERR_TRUNC / ERR_HDR (broken header, tag table, element sizes), ERR_UNSUPPORTED (CMYK,
Lab or other device spaces, link / abstract / named-colour classes, unknown tag types, para
type > 4, gray LUT profiles, singular colorant matrix, intent > 3), ERR_LIMIT (curv > 2^20
entries), ERR_NOMEM.

## Profiles understood

* Classes mntr / scnr / prtr / spac, colour spaces RGB and GRAY, PCS XYZ or Lab, v2 and v4.
* Matrix/TRC: rXYZ gXYZ bXYZ + rTRC gTRC bTRC; gray kTRC (Y * D50 from the G channel of
  our BGRA pixels; a gray destination writes R = G = B = inverse kTRC(Y)).  Curves: curv
  identity / gamma (u8.8) / table (linear interpolation), para types 0..4 (ICC formulas,
  `t = a x + b`, `t <= 0 -> 0`).
* LUT tags A2B0..2 / B2A0..2 as lut8 (mft1), lut16 (mft2), lutAtoB (mAB) and lutBtoA (mBA),
  3 inputs / 3 outputs: A/M/B curves, CLUT (8/16 bit, per-dimension grids, tetrahedral), 3x4
  matrix; the lut16 / lut8 matrix only on an XYZ input (B2A with PCS XYZ).  PCS encodings:
  XYZ u1.15 (`v * 65535/32768`), Lab v4 (mAB/mBA/lut8: `L = 100 v`, `a = 255 v - 128`) and the
  legacy v2 16-bit Lab of lut16 (`L = 100 * 65535/65280 v`, `a = 65535/256 v - 128`).
* Tag choice as LittleCMS: the intent's LUT tag (absolute uses *1), else *0, else matrix/TRC.
* Intents: matrix/TRC profiles use the same colorimetry for every intent; absolute scales
  XYZ by `wtpt_src / wtpt_dst` (media white; D50 for v2 display profiles like lcms); BPC with
  the lcms formula `xyz' = a xyz + b` (`a = (bd - D50) / (bs - D50)`,
  `b = -D50 (bd - bs) / (bs - D50)`) using the source black (device 0 through the source)
  and the destination black (its A2B at device 0 for LUT destinations, else 0) -- a no-op
  between matrix/TRC profiles.  Not emulated: lcms' forced BPC with the fixed v4 perceptual
  black in the perceptual intent of v4 LUT profiles, chad-based adaptation variants.
* Robustness: `icc_check` first (declared size, 'acsp', version 2..5, every tag inside the
  profile, 64-bit arithmetic), then every element's own size (curves, lut tables, CLUT =
  prod(grid) * 3 * precision, mAB offsets + element ends, mluc records) is checked in
  64 bits before use; grid < 2 / precision not 1..2 -> ERR_HDR.  Fuzzing: 1500 mutated
  profiles per target, no crash.

## Transform paths

* **Identity** -- byte-identical profiles (and a matrix path whose combined matrix is I and
  whose tables map every 8-bit value to itself, e.g. two different sRGB profiles): rows are
  copied (memcpy, ~2 GPixel/s).
* **ICX_MATRIX** (both sides matrix/TRC or gray, no BPC offset): f32 linearisation tables
  (3 x 256, the source TRCs), one f32 3x3 matrix `dst^-1 * diag(adapt) * src`, per channel
  clamp to 0..1, index `round(sqrt(v) * 4096)` into u8 tables (3 x 4097) of the destination's
  inverse TRC -- built by inverting 8193 forward samples (made monotonic) with a walk, so
  every curve kind works; the sqrt companding keeps the step below 0.13 codes even for
  gamma 1.0 / 2.2 / 1.8 near black.  SSE: 4 pixels per step (gathers into SoA registers).
* **ICX_LUT** (any LUT tag, or ICF_LUT): the float64 pipeline (source -> PCS XYZ -> intent /
  BPC -> destination) is sampled on a 33^3 grid whose node positions per axis are
  `(k/32)^2` (dense near black, where TRCs are steep), built with `thr_parallel_for`; nodes
  are f32 B, G, R, 0 (16 bytes, 575 KB).  Per pixel: axis tables give `{k * stride, frac}`
  for every 8-bit value, the tetrahedron is chosen by ordering the three fractions, f32
  interpolation.  Matrix/TRC destinations store the destination's *linear* values unclamped
  and finish like the matrix path (clamp, sqrt index, exact inverse TRC tables): no kink at
  the gamut border inside a cell.  B2A destinations store device values (0..1) -> `* 255`.

## Accuracy (tools/icc_check.py, both targets identical)

| comparison | result |
|---|---|
| matrix/TRC (built-ins, 40 synthetic profiles with curv/para/tables, v2/v4, all intents, BPC) vs float64 reference | max 1, 98.7 % exact |
| gray (kTRC gamma/para/table) in / out vs float64 | max 1, 99.2 % exact |
| 3D-LUT path vs a numpy model of the same grid (f32 op order) | bit-exact (100 %) |
| ICF_LUT (matrix profiles through the grid) vs exact | max 2, p99 1 |
| A2B sources (lut16 XYZ/Lab, lut8 Lab, mAB with/without CLUT) vs exact pipeline | p99 <= 2 |
| B2A destinations (mBA, lut16 XYZ / Lab) vs exact pipeline | p99 <= 8, max 24 (the destination LUT is resampled: double interpolation, worst near black / gamut border) |
| vs LittleCMS 2 (WSL liblcms2, ctypes) matrix/TRC RGB | max 2 (lcms' 8-bit optimised matrix-shaper path is the one off near black; ours is within 1 of float64) |
| vs LittleCMS RGB -> gray | max 5 (lcms resamples into a CLUT, steep gamma 2.2 near black) |
| vs LittleCMS LUT profiles (relative intent) | p99 <= 6, max 20 |

Absolute intent with a non-D50 media white in a v4 profile is checked only against the
float64 reference (lcms resamples such pipelines into a clipped CLUT and differs by up to
20 at gamut borders).

## Speed (one thread, 1 MPixel rows, both targets similar; the display and Konvertieren run on the worker pool)

| path | MPixel/s |
|---|---|
| ICX_MATRIX | 130..155 (90 with the machine busy) |
| ICX_LUT | 40..54 (28 busy) |
| identity | 1300..2500 |
| build: matrix | ~5 ms (8193 x 3 curve samples); LUT grid ~50 ms on the pool |

## Built-in profiles

Generated at run time (`bi_make`, own numbers; no copied files): ICC v4.3, class mntr, PCS
XYZ, D50 wtpt, desc/cprt as mluc ("... compatible", "Public domain, generated by jfguiLib"),
chad (Bradford from the space's white), colorants = chad * RGB->XYZ(primaries, white) as
s15.16, TRC as para:

| id | desc | primaries / white | TRC |
|---|---|---|---|
| ICC_BI_SRGB | sRGB IEC61966-2.1 compatible | Rec.709 / D65 | para 3: 2.4, 1/1.055, 0.055/1.055, 1/12.92, 0.04045 |
| ICC_BI_ADOBE | Adobe RGB (1998) compatible | 0.64/0.33, 0.21/0.71, 0.15/0.06 / D65 | gamma 563/256 |
| ICC_BI_P3 | Display P3 compatible | DCI-P3 / D65 | sRGB curve |
| ICC_BI_PROPHOTO | ProPhoto RGB compatible | ROMM / D50 | gamma 1.8 |
| ICC_BI_GRAY22 | Gray Gamma 2.2 | GRAY, D50 | gamma 2.2 |

## Monitor profile

* Windows: `MonitorFromWindow` (main window; `ICC_MON_PRIMARY` = -1 -> primary monitor)
  -> `GetMonitorInfoW` (device name) -> `CreateDCW` -> `GetICMProfileW` -> `plat_file_read`.
  CreateDCW / DeleteDC / GetICMProfileW via GetProcAddress (gdi32; not in the import lib).
  No window (headless) -> 0.  `plat_monitor_key` = HMONITOR (poll every frame).
* Linux X11: root property `_ICC_PROFILE` (InternAtom only-if-exists, GetProperty up to
  1 MB; screen 0 only -- `_ICC_PROFILE_n` of other Xinerama screens is not read).
  Wayland: 0 (no colour-management protocol in the core; assume sRGB).  Not dispatched
  through disp.s: `disp_backend()` decides (no table slot needed).
* Tested: primary monitor on Windows (header check only, profile not copied), no-crash runs
  with real windows on Windows, X11 and Wayland (WSLg has no `_ICC_PROFILE`).

## Tests

```
python tools/icc_check.py --target both --out build/i1      # all suites (~1 min)
python tools/icc_check.py --only matrix,lut --no-build --target win
```
Suites: builtin (headers, colorants within 1e-4 of the standards, descriptions incl. mluc
German preference / surrogates / truncation), matrix, gray, lut, ident, rows (row lengths
1..33, in place, 0 pixels), errors, fuzz, lcms (skipped without liblcms2 in WSL), speed,
monitor.

---

# Farbprofil / Farbmanagement (PaintCraft, agent I1, plan M8, spec 2.12)

Module `src/colormgmt.s`; library: `ASMLIB/docs/modules/COLOR_MANAGEMENT.md`.

## Bild ▸ Farbprofil… (CMD_COLOR_PROFILE 278)

Modal dialog window "Farbprofil" (WINID_DLG_PROFILE 0x6038, widgets ID_DLG_PROFILE 6300..):

* **Aktuell:** description of the document profile (DOC_ICC), "sRGB (kein Profil
  eingebettet)" without one.  (paint.net's own resource string is the untranslated
  "Current:"; German is used here.)
* **Neu:** list: sRGB, Adobe RGB (1998), Display P3, ProPhoto RGB, Graustufen (Gamma 2,2)
  (built-in, generated profiles), "Monitor: <desc>" when the system has a monitor profile,
  "Importiert: <desc>" after **Import…** (file dialog `*.icc;*.icm` "Farbprofil", id block
  ID_FD_ICC 0x24000; an unusable file -> "Die Datei ist kein verwendbares Farbprofil.").
  **Export…** writes the document profile (sRGB built-in when none) to a file.
* **Priorität:** Perzeptiv / Relativ farbmetrisch / Sättigung / Absolut farbmetrisch;
  **Tiefenkompensierung verwenden** (default: Perzeptiv + Tiefenkompensierung).
* Buttons **Konvertieren** (accent) / **Zuweisen** / **Abbrechen** (paint.net 5.1:
  "Kon&versieren", "Zu&weisen").

Actions (one history step each, IC_COLOR, names from paint.net's resources):
* **Zuweisen** -> "Farbprofil zuweisen": DOC_ICC = a copy of the chosen profile, pixels
  unchanged (the step is carried by `hist_push_sel`; the selection stays as it is).
* **Konvertieren** -> "Farbprofil konvertieren": every layer converted from the current
  profile (none = sRGB) with `icc_xform_row` on the worker pool (64-row bands x layers), new
  surfaces installed like imgops.s does (`hist_push_doc`, old surfaces to the history with
  `hist_release`), then DOC_ICC = the new profile.  A profile that cannot be used ->
  the dialog comes back with "Das gewählte Farbprofil kann nicht für die Konvertierung
  verwendet werden." (paint.net string).
* Undo / redo of DOC_ICC: the history has no DOC_ICC slot, so colormgmt.s records every
  step {document, token in DOC_CSPACE, HE_SEQ of the entry, old profile, new profile} and
  `cm_sync` (every frame) sets DOC_ICC to the state of the history position; records whose
  entries were dropped / truncated / cleared are freed, and so are records of closed
  documents (token mismatch).

## Display (canvas)

The canvas rows (view.s, after `k_view_row` / `k_view_row_pm`, block "I1") go through
`cm_view_row`: document profile -> monitor profile of the main window (`plat_monitor_icc`,
queried again when `plat_monitor_key` changes = the window moved to another monitor; none /
Wayland / headless = sRGB), perceptual intent.  The composite is never changed.  Transforms
are cached per (document profile, monitor profile) by CRC-32 (4 entries); identical or
equivalent profiles cost nothing (no transform).  Colour management is **on** by default
(paint.net 5.1 always colour-manages the canvas).  Not colour-managed: thumbnails (tabs,
layers), the colour picker / palette, rulers and overlays.

## Interfaces

* imgops.s (A5) forwards (block "I1"): `cm_states`, `cm_frame` (in iop_frame), commands 278,
  280..283 and 290..299 -> `cm_cmd` (before its doc check).
* Settings (S1, page "Farbmanagement"): `cm_enabled() -> eax 1 on`, `cm_set_enabled(on)`
  (redraws the canvas with the next frame).
* fileio.s (K1) already does what is needed: open copies IM_ICC into DOC_ICC, save embeds
  DOC_ICC (IOF_EMBED_ICC).  Suggested later: formats without ICC support could offer
  paint.net's "In sRGB umwandeln" on save (BMP / GIF / TGA ConfigUI strings exist).

## Test hooks (cmd ID ARG) and probes

| cmd | effect |
|---|---|
| 280 / 281 row | Zuweisen / Konvertieren with list row (0..4 built-in, then monitor, then imported) |
| 282 / 283 | Import… / Export… file dialog |
| 290 id | monitor profile = built-in id 0..4; 255 = none (sRGB); 65535 = real query |
| 291 on | colour management on / off |
| 292 i + (b << 8) | dialog intent i, Tiefenkompensierung b |
| 293 x + (y << 16) | display probe: composite pixel (x, y) as shown -> #533249 "AARRGGBB" |
| 294 id | built-in id as the "imported" profile |

Probes: #533248 (0x82300) text document profile description ("" = none), value DOC_ICC_LEN ·
#533249 display probe, value = colour management on · #533250 monitor profile description,
value display transform kind (-1 none, 1 matrix, 2 3D LUT) · #533251 dialog error text,
value = profile history records.

Script: `tests/colorprofile.uis` (assign, convert sRGB -> Adobe RGB / Graustufen with pixel
checks against the icc_check reference, undo / redo, simulated P3 monitor, on / off, the
dialog through the combo box, imported row, error path); Windows and Linux, also with
`--verify-dirty`.  A one-off screenshot check confirmed the canvas shows sRGB red as
(234, 51, 35) on a simulated Display P3 monitor and (255, 0, 0) with management off.
