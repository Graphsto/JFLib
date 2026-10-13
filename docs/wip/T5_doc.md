# T5 – Text tool (T)

`src/t_text.s` (TOOL_TEXT, spec TEXT-001..020), test `tests/tools_text.uis`.

## Behaviour (paint.net 5.1)
* Left click on the canvas places the text cursor; the anchor is the top of the first line
  (its left end / centre / right end by the alignment).  The edit is **live** (TF_LIVE):
  typing inserts UTF-8 text (TEXT-017), Enter = line break (TEXT-018), Rücktaste / Entf /
  ← → ↑ ↓ / Pos1 / Ende (Strg+Pos1 / Strg+Ende: whole text) (TEXT-019), Strg+V and
  Umschalt+Einfg paste the text clipboard (CR LF → LF, tab → space, controls dropped).
  While live, character keys are text (no tool shortcuts), Entf / Rücktaste do not reach
  the selection commands.
* Caret (dark + light line, blinks 530 ms, always on in headless runs), handle (square nub
  at the lower right corner of the text) drags the whole text (TEXT-014); Strg held hides
  caret and handle.  Status bar while live: paint.net's "Ziehen Sie den Ziehpunkt …" text.
* A click inside the text moves the caret; a click outside commits and starts a new text.
  Fertigstellen / a click outside / another tool or image / a document command = one
  history step "Text" (icon IC_FONT) written into the active layer (TEXT-015, TEXT-020);
  an empty text makes no step.  Esc and Strg+Z drop the live text (no step).
* Colour = primary colour (TEXT-012); the Fülleffekt hatch uses the secondary colour as
  background; Füllmethode incl. Überschreiben (TEXT-013); selection clip via the tool layer.
* Options apply to the live text immediately.

## Tool row
`Schriftart:` [family ▾] [size] [▾] | **B** *I* U S  [Glatt ▾] [≡ ▾]  Fülleffekt, Antialiasing,
Füllmethode, Fertigstellen.

| widget | id | notes |
|---|---|---|
| family drop-down | `ID_FONT_LIST` 0x50000 | flyout: search box 0x50001 (case-insensitive substring; the first match is taken at once), virtualized list 0x50002 (rows 0x50003 + i); "Schriftarten werden geladen..." while the scan runs |
| size box | 3002 | points 1..2000 (integers) |
| size drop-down | 3003 | flyout: [A-] 3001 / [A+] 3004 (Strg: ±5), list 0x8200F of 8…288 (rows 0x82010 + i), metric rows 3006 "Punkte (Bild DPI)" / 3007 "Fest (96 DPI)" |
| Fett / Kursiv / Unterstrichen / Durchgestrichen | 3008..3011 | toggles (real bold / italic faces via fontdb, synthetic otherwise) |
| Text-Rendering-Modus | 3012 (rows 3013..3015) | Glatt, Scharf (Modern), Scharf (Klassisch) |
| Ausrichtung | 3016 (rows 3017..3019) | Linksbündig, Zentrieren, Rechtsbündig |
| Fülleffekt, Antialiasing, Füllmethode | tool.s `opt_fill` / `opt_aa` / `opt_blend` | shared TOOLCFG |

After a click on an option button while live the keyboard focus returns to the canvas
(`ui_focus(ID_CANVAS)`).  Typed text and keys go to the text only while the canvas, a
tool-row button or nothing has the focus (not the size box / search box).

## Implementation
* Size in px = points × dpi / 72 (`view_dpi100`; Fest: 96 dpi).
* Layout: lines split at LF; `text_measure_n` per line; x by the alignment (rounded to
  whole px), line height `text_line_h`, baseline = top + `text_ascent`.
* Raster: the text rectangle + margin (size/2 + 4) clipped to the image, in 64-row bands
  (one TMASK tile row): `text_draw_n` (+ `text_decor` underline / strikeout) into an A8
  surface, `tm_combine_block` into the image-size coverage TMASK; antialiasing off =
  coverage ≥ 128 → 255.  TL = `tl_set_paint` (TLK_SOLID).  Colour fonts (`font_is_color`:
  COLR / CBDT, e.g. Segoe UI Emoji) render into PARGB bands: coverage = alpha, a straight
  colour surface (`TLK_SOURCE`, alpha = primary alpha) keeps the glyph colours.
  Only the tiles of the previous rendering are cleared per update; `tl_set_box` marks old
  and new box.  Commit: `tm_optimize`, `tl_commit(tl, "Text", IC_FONT)`.
* Fonts: `fontdb_load(family, 400/700, italic)` (+ `font_style` FS_SHARP for Scharf);
  all text / font / glyph-cache calls on the UI thread.  The fontdb scan runs once as a
  worker job when the tool is first activated (`fontdb_load_cache` → `fontdb_scan` →
  `fontdb_save_cache`, cache `<plat_config_dir>/PaintCraft/fontcache.fdb`, not written in
  headless runs); fontdb is not touched by the UI thread before the job finished.  Until
  then (or when loading fails) the embedded Segoe UI (FONT_REG / FONT_BOLD, synthetic
  italic) renders – identical outlines to segoeui.ttf.  Default family after the scan:
  Segoe UI, else DejaVu Sans, Noto Sans, Liberation Sans, Ubuntu, Cantarell, Arial, else the
  first family containing "Sans".
* Keys: TD_KEY consumes the frame's keys in queue order (editing keys, Enter, Strg+V,
  character keys); typed text (`ui_text_take`) is taken in TD_TICK.  Framework change (T5):
  `TF_TEXTKEYS` (pn_tooli.inc) – tool.s leaves Enter to TD_KEY for such tools.
* Hooks 840..844 (tool_precmd lets 840..849 pass without committing): 840 test font id
  (-1 = family), 841 size pt, 842 options (bits 0-3 B I U S, 4-5 align, 6-7 render, 8
  metric), 843 anchor x | y << 16, 844: 1 wait for the scan, 2 | i << 8 caret, 3 | i << 8
  family i.  Records `ID_TTEXT_FIRST` + 0..7 (0x82000 = 532480): text (value live),
  "caret,line,ax,ay", family (value pt), "style,align,render,metric" (value font id), TL box
  (value scan state), "families,filtered" (value filtered > 0), font from file, text rect.

## Deviations / open points
* Scharf (Klassisch) = Scharf (Modern) (the library has one grid-fitting mode, FS_SHARP;
  no GDI-like integer advances).  Glatt / Scharf differ only below ~24 px noticeably.
* No text selection inside the live text (paint.net has none either, unverified), no
  Strg+←/→ word jumps, no IME composition window (committed IME text arrives normally).
* Font list rows show the names in the UI font, not in their own typeface.
* Sizes are integers (paint.net accepts fractions); size list 8…288 as in the book, exact
  paint.net list unverified.  Size Inc/Dec buttons live in the size drop-down (tool row
  width at 1280 px); paint.net places them in the row.
* No font fallback: characters missing in the chosen font show its .notdef glyph (emoji need
  an emoji font like Segoe UI Emoji); no GSUB / complex shaping (library limits).
* Hatch fill does not apply to colour-font glyphs (their own colours are kept).
* Handle position (lower right corner) and caret look are not verified against paint.net.

## Framework changes (outside t_text.s, small, marked "T5")
* `inc/pn_tooli.inc`: new flag `TF_TEXTKEYS = 0x1000` (additive).
* `src/tool.s` tool_frame: with a pending edit, Enter is not taken as Fertigstellen when the
  current tool has TF_TEXTKEYS (goes to TD_KEY instead).
* `src/tool.s` tool_precmd: the pass-through range now starts at CMD_TTEXT_FIRST (840) instead
  of CMD_SET_FIRST (850), so the M5 tool hooks 840..849 (t_text, t_clone) no longer commit
  a pending edit.
* Suggestion for TOOLS.md (T1): list TF_TEXTKEYS in §2/§3.

## Tests
`tests/tools_text.uis`: Windows + Linux (Wayland and jfgui_BACKEND=x11), also with
--verify-dirty; all other PaintCraft scripts stay green on both targets.
