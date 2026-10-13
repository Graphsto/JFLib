# PR1 – Drucken (Kopie von ASMLIB/docs/modules/PRINT.md + apps/PaintCraft/docs/PRINT.md)

# Printing (PR1)

`plat_print(job)` prints one image on one page with the system's print dialog.
Include `print.inc` after `jfgui.inc`.

| File | Content |
|---|---|
| `inc/print.inc` | `PRINTJOB` (`PJ_*`), `PPAGE` (`PG_*`), `PLACE` (`PL_*`), `PDFOPT` (`PO_*`), flags |
| `src/plat/print.s` | OS-independent: `print_page_init`, `print_layout`, `print_row_white`, `print_pdf`, `print_dpi` |
| `src/img/pdfwrite.s` | `pdf_write_image(sf, opt, &buf, &len)`: one-page PDF 1.4 |
| `src/plat/win/win_print.s` | `plat_print`: PrintDlgExW + GDI |
| `src/plat/linux/lx_print.s` | `plat_print`: xdg-desktop-portal Print with a PDF in a memfd |
| `src/plat/linux/portal.s` (section "PR1") | `portal_print_prepare / _send / _end` (D-Bus) |

## API

```
plat_print(job) -> eax  0 printed | ERR_CANCEL | ERR_UNSUPPORTED (no print system, headless)
                        | ERR_IO | ERR_NOMEM | ERR_DIMS
PRINTJOB  PJ_SRC      SURFACE* FMT_ARGB (straight), FMT_PARGB or FMT_XRGB
          PJ_TITLE    UTF-8 document name (spooler job name, PDF /Title); 0 = "Bild"
          PJ_DPI_X/Y  f64 pixels per inch (<= 0: 96); print_dpi(unit, dpu) converts a
                      document resolution (0 none, 1 inch, 2 cm)
          PJ_FLAGS    PJF_LANDSCAPE, PJF_CENTER, PJF_SMASK (PDF only)
          PJ_SCALE    PJS_FIT (as large as the printable area allows, aspect kept),
                      PJS_DPI (pixels / DPI inches), PJS_PERCENT (PJ_PERCENT 1..10000 % of that)
          PJ_MARGIN   1/100 mm on every side (the printer's unprintable edge wins when larger)
          PJ_PAPER    PAPER_A4 / _LETTER / _A5 / _LEGAL / _A3 suggested to the dialog
          out: PJ_COPIES, PJ_PAGE_W / PJ_PAGE_H (pt of the page used)
```

`print_layout(job, page, place)` is what both back ends use (and an app's preview):
area = page minus max(margin, unprintable edge) per side; the image rectangle (pt from the
page's top-left) is centred in the area or at its top-left corner; everything outside the
area is clipped.  Pixels go out composited over white (`c*a/255 + 255 - a`, rounded;
premultiplied: `c + 255 - a`).

## Windows

`wp_load` takes `CreateDCW DeleteDC StartDocW StartPage EndPage EndDoc AbortDoc
SetStretchBltMode SetBrushOrgEx IntersectClipRect` from gdi32 with GetProcAddress (no new
import entries; `StretchDIBits` / `GetDeviceCaps` were already in `gdi32.def`), `PrintDlgExW`
from comdlg32, `GetDefaultPrinterW` from winspool.drv.  The dialog gets a DEVMODE (default
printer's name, `dmOrientation`, `dmPaperSize`); if it rejects it, it is called again with the
defaults.  Flags `PD_RETURNDC | PD_NOPAGENUMS | PD_NOSELECTION | PD_NOCURRENTPAGE |
PD_USEDEVMODECOPIESANDCOLLATE` (the driver makes the copies).  Page geometry from
`GetDeviceCaps` (PHYSICALWIDTH/HEIGHT, PHYSICALOFFSETX/Y, HORZRES/VERTRES, LOGPIXELSX/Y),
`HALFTONE` stretching, `IntersectClipRect` = printable area, the image as bottom-up 24-bit DIB
bands of at most 4 MB (`StretchDIBits`, band edges computed from the same rounding so bands
abut; bands outside the clip are skipped).  StretchDIBits' 13 arguments need 104 bytes of
outgoing space: the frame keeps locals from `[rsp+112]`.

## Linux

`PreparePrint(parent, title, settings{orientation}, page_setup{Name, DisplayName, Width,
Height, Orientation}, options{handle_token, modal})` -> Response `results{settings,
page-setup, token}`; the page setup's Width / Height / Margin* (mm) and Orientation define the
page (landscape swaps).  The job is laid out on it, written as PDF (`print_pdf`) into a memfd
and sent with `Print(parent, title, h fd, options{handle_token, modal, token})` (SCM_RIGHTS;
requires NEGOTIATE_UNIX_FD).  One bus connection for both calls.  No bus / no portal / no
descriptor passing -> ERR_UNSUPPORTED; cancelled -> ERR_CANCEL; Print response 2 -> ERR_IO.
The window keeps being serviced while the portal dialog is open (like the file dialog).

## PDF writer

`pdf_write_image(sf, opt, &buf, &len)`: objects 1 Catalog, 2 Pages, 3 Page (MediaBox), 4
Contents (`q [clip re W n] w 0 0 h x y cm /Im1 Do Q`), 5 Image (/DeviceRGB 8 bit
/FlateDecode via `zlib_deflate`), 6 Info (/Producer, /Title as UTF-16BE hex), 7 optional
/SMask (/DeviceGray alpha, with PJF_SMASK and FMT_ARGB: colours stay straight); classic xref
with 20-byte entries.  Coordinates are written with 3 decimals.

## Test variants (never a dialog, never paper)

| Variable | Effect |
|---|---|
| `jfgui_PRINT_TO=FILE` | Linux: the PDF goes to FILE (paper = PJ_PAPER). Windows: the printer's output goes to FILE (`DOCINFO.lpszOutput`), no dialog |
| `jfgui_PRINT_PRINTER=NAME` | Windows with jfgui_PRINT_TO: that printer (default "Microsoft Print to PDF"), `CreateDCW` + DEVMODE |
| `jfgui_PRINT_SMASK=1` | Linux with jfgui_PRINT_TO: alpha as /SMask |
| `jfgui_PRINT_PORTAL=1` | Linux: allow the portal in a headless run (fake bus tests) |

A headless run (`ui_headless()`) without these returns ERR_UNSUPPORTED before any dialog or
bus contact.

## Tests

`python tools/print_check.py [--target win|linux|both] [--out build/pr1]` (needs PaintCraft
built into `--out`): Linux PDFs parsed by an own parser (xref offsets, trailer, MediaBox,
placement and clip vs. `print_layout` replayed in Python, decompressed pixels = composite over
white, /SMask variant, /Title), `pdfinfo` / `qpdf --check` when WSL has them; the portal path
with a fake session bus (`tools/print_portal_bus.py`, inside WSL: arguments of PreparePrint /
Print, token, descriptor content, cancel, no portal, Print failing, no fd passing, headless
guard, no bus); Windows "Microsoft Print to PDF" into files (1 page, MediaBox of paper and
orientation from the DEVMODE; skipped if the printer is missing); headless guard on both.

Untested: a real printer, the real Windows print dialog (PrintDlgExW path, DEVMODE retry,
copies) and the real portal dialog / GTK print backend (the fake bus replays the documented
protocol).

---

# Drucken (PR1, Plan M8)

Datei ▸ Drucken… (Strg+P, `CMD_PRINT` 106), aktiv nur mit offenem Bild.  paint.net 5.1 druckt
das zusammengefügte Bild über den Windows-Fotodruck-Assistenten; PaintCraft zeigt stattdessen
einen eigenen modalen Dialog und ruft dann `plat_print` der Bibliothek auf
(`ASMLIB/docs/modules/PRINT.md`: Windows-Druckdialog + GDI, Linux-Portal + PDF).

## Dialog (`src/print.s`, Fenster `WINID_DLG0 + 48` = 0x6040 = 24640)

| Id | Element | Vorgabe |
|---|---|---|
| 6210 | Vorschau: Seite (weiß), druckbarer Bereich (Linie), Bild (Thumbnail über Weiß, Umriss) | |
| 6201 | Papierformat: A4 / Letter / A5 / Legal / A3 (Vorschlag für den Systemdialog) | A4 |
| 6202 / 6203 | Ausrichtung: Hochformat / Querformat | Hochformat |
| 6204 / 6205 / 6206 | Druckgröße: An Seite anpassen / Originalgröße (Pixeldichte) / Prozentual: | anpassen |
| 6207 | Prozent (1..1000, nur bei „Prozentual:“) | 100 |
| 6208 | Zentrieren (sonst links oben im druckbaren Bereich) | an |
| 6209 | Rand: n mm (0..50; der unbedruckbare Rand des Druckers gilt, wenn größer) | 5 |

Darunter „Bild: W × H Pixel, n dpi“ und „Druckgröße: b × h cm“ (aus `print_layout`, dieselbe
Rechnung wie beim Drucken; im Systemdialog kann das Papier noch wechseln).  „Drucken…“ schließt
den Dialog und druckt; Abbrechen / Esc / Schließen verwerfen nichts (die Optionen bleiben für
die Sitzung).  Pixeldichte = Auflösung des Dokuments (`DOC_DPU_*`, Zentimeter umgerechnet,
ohne Einheit 96 dpi).  Fehler (kein Drucker/Druckdienst, Speicher, Druckfehler) → Fehlerleiste;
Abbruch im Systemdialog → nichts.

Texte: „Drucken“, „Druckgröße“, „Prozentual:“ aus paint.net (`CommonAction.Print`,
`ResizeDialog.PrintSizeHeader`, `ResizeDialog.PercentRB`); die übrigen eigene.

## Einbindung

`fileio.s` (markierte PR1-Stellen): `fio_states` → `prt_states`, `fio_frame` → `prt_frame`,
`fio_cmd` leitet `CMD_PRINT` und `CMD_PRT_DONE` (140, `pn_ids.inc`, Arg 1 = drucken) an
`prt_cmd` weiter.

## Tests

- `tests/print.uis`: Menüzustand, Dialog, alle Optionen mit der Druckgröße, Esc/Abbrechen,
  Optionen bleiben, headless „Drucken…“ ohne Testvariable → Fehlerleiste (nie ein Dialog),
  ohne Bild deaktiviert.  `skeleton.uis`: „Drucken…“ jetzt aktiv.
- `python tools/print_check.py --target both`: echte Ausgabe über die Testvarianten
  (`jfgui_PRINT_TO`, Windows „Microsoft Print to PDF“), Portal mit Fake-Bus.
