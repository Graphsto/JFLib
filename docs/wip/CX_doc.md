# CX – JPEG XR Decoder (ITU-T T.832 / ISO 29199-2)

Stand: Decoder vollständig in Assembler, bitgenau zu WIC für alle 8-Bit-Formate und für die
nativen Samples aller getesteten Formate; Encoder weiterhin `ERR_UNSUPPORTED`.

## Dateien (Besitz CX)

| Datei | Inhalt |
|---|---|
| `ASMLIB/src/img/jxr.s` | Container (TIFF-artig, Annex A), `jxr_probe` / `jxr_decode` / `jxr_encode` (Stub), Metadaten (IMGMETA, ICC/XMP/EXIF), planares Alpha, `jxr_dbg_planes` (Testhaken), `jxr_codec` |
| `ASMLIB/src/img/jxr_ent.s` | Codestream-Header (Image/Plane/Index-Tabelle), Tiles, Pakete (Spatial + Frequency), adaptive VLC, DC/LP/CBPHP/HP/FLEXBITS, Prädiktion, Dequantisierung, erste Transformstufe |
| `ASMLIB/src/img/jxr_pct.s` | ICT 4x4, Overlap-Postfilter (Stufe 1 auf DC-Gittern inkl. 4:2:0/4:2:2-Chroma, Stufe 2 auf Samples) |
| `ASMLIB/src/img/jxr_color.s` | Ausgabeformatierung 9.10 (InvColorFmtConvert1–4, AddBias, Scaling, PostScalingInt/Fl, Clipping) und Wandlung nach 8-Bit-BGRA wie WIC; SSE2-Schnellpfad BD8, SSE2-Chroma-Upsampling |
| `apps/codectest/src/jxr_ops.s` | Ops 200 `OP_JDUMP` (Ebenen / native Samples), 201 `OP_JTIME` (Decodezeit), 202 `OP_JDTIME` (Zeit pro Stufe) |
| `tools/jxr_check.py` | Prüfer (WIC-Matrix, native Samples, Meta, Fehler, Fuzz, Performance) |
| `ASMLIB/inc/img.inc` (CX-Block am Ende) | `JP_*` (Ebene), `JX_*` (Codestream), `JXF_*`, `JXW_*` (inkl. neu `JXW_QP`) |

Nicht verändert: `exif.s`, `imgio.s`, `imgreg.s`, `imgmeta.s` (Integrator). `jxr_codec` ist in
`imgreg.s` bereits registriert. `ASMLIB/docs/API.md` sollte vom Integrator mit
`python tools/gendoc.py` neu erzeugt werden (öffentliche Funktionen siehe Dateiköpfe).

## Ablauf

1. `jxr_parse` (jxr.s): IFD des Containers, 64-Bit-geprüfte Offsets/Längen, Pixelformat-GUID,
   Abmessungen, DPI, Transformation (`SPATIAL_XFRM_PRIMARY`), Bild-/Alpha-Codestream, ICC/XMP.
2. `cs_decode` → `jxe_decode` (jxr_ent.s): Header, `img_dims_check`, Ebenen-/Gitterpuffer (Heap),
   Tiles in Indextabellen-Reihenfolge; pro MB-Zeile `CANCEL_POLL`. Danach `jxp_recon` (jxr_pct.s).
3. Planares Alpha: zweiter Codestream (YONLY, gleiche Größe, sonst ignoriert).
4. `jxc_output` (jxr_color.s) zeilenweise: Chroma-Upsampling wie WIC (4-Tap-Filter, Kanten
   wiederholt), Farbwandlung nach Norm, native Samples des Pixelformats, dann 8-Bit-BGRA.
   BD8 ohne vormultipliziertes Alpha läuft über `co_fast8` (SSE2, 4 Pixel/Schritt).
5. Orientierung: `IM_ORIENT` aus `SPATIAL_XFRM_PRIMARY` (EXIF-Werte), angewendet vom Dispatcher.

## Unterstützt / nicht unterstützt

* Interne Formate YONLY, YUV420, YUV422, YUV444, YUVK; Ausgabe YONLY, RGB, CMYK, CMYKDIRECT;
  Bittiefen BD1 (weiß/schwarz), BD5, BD565, BD8, BD10, BD16, BD16S, BD16F, BD32S, BD32F.
* Overlap 0/1/2, Tiling (weich), Spatial- und Frequency-Modus, Index-Tabelle, Bänder-Verwerfen
  (`ImageDataDiscardLevel` 0..3), FLEXBITS inkl. TRIM, Windowing, interleaved und planares Alpha,
  vormultipliziertes Alpha (Header-Flag oder P-GUID 0x10 / 0x17 / 0x1A).
* `ERR_UNSUPPORTED`: NCOMPONENT, RGBE, YUV-Ausgabeformate, **Hard Tiling mit Overlap und mehreren
  Tiles** (Overlap-Filter müssten an Tile-Grenzen enden; WIC erzeugt das nicht, keine Testdateien),
  Encoder.

## WIC-Abgleich (FormatConvertedBitmap → Bgra32)

| Quelle | Regel | Ergebnis |
|---|---|---|
| 8 Bit (Bgr24/32, Bgra32, Gray8), BlackWhite, Bgr555/565, Rgb48, Rgba64 | wie decodiert; 16 Bit `v >> 8`; 5/6 Bit Bit-Replikation | exakt |
| Pbgra32 | gerade: `min(255, c * floor(255*65536/a) >> 16)` auf ungeklemmten Werten; PARGB: Farbe ≤ Alpha | exakt |
| Prgba64 | 16-Bit-Unpremultiply gerundet, dann `>> 8` | ±1 (≈0,2 % der Samples) |
| Gray16, Bgr101010 | `round(v*255/65535)` bzw. `round(v*255/1023)` | ±1 (WIC rechnet über lineares Licht mit eigener Näherung) |
| Float/Half/Fixed (Rgba128Float, Prgba128Float, Rgb128Float, Gray32Float) | linear → sRGB über 4096er-Tabelle; P-Formate `c/a` (a == 0 → 0) | ±1 |
| Cmyk32 | `(255-c)(255-k)/255` wie tiff.s/jpeg | weicht ab (WIC farbmanagt); geprüft gegen die Formel auf WICs nativen CMYK-Samples |

Native Samples (vor der 8-Bit-Wandlung, `OP_JDUMP` Modus 3) sind für **alle** Formate bitgenau
zu WICs nativer Ausgabe (244 Dateien).

Bekannte WIC-Fehler: bei interleaved Alpha mit interner Unterabtastung 4:0:0 (und einem
4:2:0-Fall) liefert WIC Alpha 0 bzw. falsches Alpha. Unser Alpha folgt dem Codestream
(Abgleich mit dem unabhängigen Python-Referenzdecoder, verlustfreies Alpha reproduziert).
`jxr_check.py` vergleicht dort nur die Farbe (Notiz in der Ausgabe).

## Erkenntnisse (Norm vs. Praxis)

* Gepackte Formate (BD5/565/10): Rot/Blau-Tausch nach Norm, wenn `RED_BLUE_NOT_SWAPPED` = 0;
  WIC legt Ebene 0 danach in die **oberen** Bits (= R von BGR565 usw.).
* YUVK/YONLY/NCOMPONENT: DC-Werte aller Komponenten werden mit der Luma-Tabelle gelesen (Modell
  und LapMean aber getrennt nach Luma/Chroma) – das war der letzte Fehler bei Cmyk32.
* BD32F/BD16F: `PostScalingFl`; Halbfloat = Vorzeichen | min(|x|, 32767).
* Verlustfrei-Erkennung (`IMF_LOSSLESS`): alle QP = 0, keine verworfenen Bänder, keine
  beschädigten Pakete, keine Chroma-Unterabtastung bei RGB-Ausgabe, keine 32-Bit-Ausgabe.

## Metadaten (IMGMETA)

Größe, Bits/Sample, Modell, Kanäle, `IMF_ALPHA`, `IMF_SRC_PREMUL`, `IMF_HDR`, `IMF_FLOAT`,
`IMF_LOSSLESS` (nur nach vollem Decode bekannt), Orientierung, DPI (+ `IMR_INCH`), ICC
(Tag 0x8773), XMP (0x02BC), EXIF (der Container selbst mit `II*\0`-Kopf, enthält den
EXIF-IFD-Zeiger 0x8769). `IMGD_PROBE` liest nur Container + Headerbytes.

## Sicherheit / Grenzen

Abmessungen ≤ 65535 und `img_dims_check`/`ID_MAXPX`; Tile-Anzahlen und -Grenzen validiert;
Offsets 64-Bit geprüft; VLC-Tabellen begrenzt (≤ 8 Bit pro Lookup, Escape gezählt);
keine Rekursion; Kontexte/Arbeitsrecords auf dem Heap, Stackrahmen < 4 KB; Bitleser liest nie
hinter das Paketende (Auffüllen mit Nullen, `JXW_TRUNC`); `CANCEL_POLL` je MB-Zeile.
asmcheck: **0 Fehler** in allen CX-Dateien (29 BOUNDS-Warnungen = Tabellenindizes, deren
Grenzen sich aus VLC-Alphabeten/Clamps ergeben, geprüft). Speicher-Review nach Checkliste:
Puffergrößen (Zeilenpuffer 3·PW + 5·W, Upsampling 2n = PW), SIMD-Restschleifen, Fensterung,
Alpha-Geometrie, Ganzzahlüberläufe (64-Bit-Multiplikationen bei Adressen) geprüft.

## Tests – `python tools/jxr_check.py --target both --out build/cx`

| Suite | Inhalt | win | linux |
|---|---|---|---|
| wic | 261 WPF-Dateien: ImageQualityLevel 0.1/0.5/0.9/1.0, Lossless, QualityLevel 1..255, Overlap 0–2, Subsampling 0–3, Tiles, FrequencyOrder, InterleavedAlpha, Discard 0–3, AlphaQuality, Rotation/Flips, 18 Pixelformate, 1×1 … 1001×777 | 261/261 | 261/261 |
| native | native Samples vs. WIC CopyPixels | 244/244 | 244/244 |
| meta | IMGMETA, Probe, DPI, Orientierung, ICC/XMP/EXIF | 102/102 | 102/102 |
| errors | defekte Container/Codestreams, Kürzungen, ID_MAXPX, Cancel, Hard Tiling | 29/29 | 29/29 |
| fuzz | 2500 Mutanten (+ Dump-Pfad) | 2500/2500, kein Absturz | 2500/2500 |
| perf | siehe unten | – | – |

Dazu die älteren Bit-Exakt-Sätze (sd, s4, s5, s7, s9, s10, sa; 156 Dateien) weiter exakt.

## Performance (12 MP, ein Thread, `OP_JTIME`)

| Datei | win | linux |
|---|---|---|
| Bgr24 4000×3000, WIC-Standard ImageQualityLevel 0.9 (4:4:4, 11,5 MB, 7,7 bpp) | ~515 ms | ~580 ms |
| Bgr24 4000×3000, QualityLevel 20, 4:2:0, Overlap 2 (5,7 MB) | ~345 ms | ~390 ms |

Ziel < 400 ms wird für mittlere Qualität erreicht, für die sehr hohe Standardqualität (fast
8 bpp) nicht. Messung (vor den letzten Optimierungen): Entropie-Decode der HP-Blöcke ~60 %,
Dequantisierung/Ablage HP (inkl. Page-Faults der frisch allozierten i32-Ebenen) ~15 %,
Rekonstruktion ~10 %, Farbe ~5 %. Bereits umgesetzt: Inline-Bitleser im Blockdecoder,
verzweigungsfreie Verfeinerung (FLEXBITS) und Vorzeichen, Offset-Tabelle in `hpfin`,
SSE2-BD8-Ausgabe und SSE2-Upsampling (vorher 890 ms / 683 ms).
Weitere Ideen: VLC-Tabellen mit eingebetteten Diskriminanten-Deltas (u32-LUT), `place` in den
Blockdecoder ziehen, zeilenweise Rekonstruktion statt ganzer i32-Ebenen (spart Speicher und
Page-Faults; großer Umbau), SIMD für ICT/Overlap.

## Testhaken

`jxr_dbg_planes(data, len, mode, &buf, &len)`: Modus 0 rekonstruierte Ebenen, 1 vor der
inversen Transformation, 2 DC-Gitter, 3 native Ausgabesamples (`i32[h][w][5]`). Die früheren
Stopp-Haken (`jxe_dbgstop`) sind entfernt. Ops 200–202 in `jxr_ops.s`.

## Offen

* Hard Tiling mit Overlap über mehrere Tiles (derzeit `ERR_UNSUPPORTED`).
* NCOMPONENT-/YUV-/RGBE-Ausgabe, Encoder.
* Performance-Ziel für sehr hohe Qualität (siehe oben).
* WIC-Näherungen (Gray16/10-Bit/Float ±1, CMYK farbgemanagt) nicht nachgebaut.
