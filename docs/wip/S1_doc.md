# S1 – M7 Einstellungsdialog + Persistenz (Bericht: apps/PaintCraft/docs/SETTINGS.md, Kopie)

Ergebnisse: tests/settings.uis Win+Linux (auch --verify-dirty) grün; tools/pn_settings_check.py --target both --window grün; alle übrigen PaintCraft-Skripte grün außer skeleton.uis Z. 29/30/78-80 (#160/#161 Ausschneiden/Kopieren: clip.s von K1, nicht S1); asmcheck 0 Fehler.


Dateien: `src/settings.s` (Laden/Speichern), `src/dlg_settings.s` (Dialog, Modulvertrag
`set_init/states/frame/cmd`), `inc/pn_set.inc` (IDs, Befehle, `g_st`-Layout), Tests
`tests/settings.uis`, `tools/pn_settings_check.py`.

## 1. Speicherort, Umleitung für Tests

| Fall | Datei |
|---|---|
| `--config DIR` | `DIR/settings.ini` |
| sonst `$PaintCraft_CONFIG_DIR` | `$PaintCraft_CONFIG_DIR/settings.ini` |
| sonst headless (`--shot`, `--script`) | **keine** – nichts wird gelesen oder geschrieben |
| sonst (echtes Fenster) | `plat_config_dir()/PaintCraft/settings.ini` (Windows `%APPDATA%/PaintCraft`, Linux `$XDG_CONFIG_HOME` bzw. `~/.config/PaintCraft`) |

Tests übergeben immer `--config build/...`; ohne Angabe bleibt die Konfiguration des Users
unberührt (`pn_settings_check.py nocfg` prüft, dass sie nicht entsteht).

Lesen: `set_init` (nach `tool_init`, vor den Werkzeugfenster-Inits) liest die Datei und wendet
Theme, Akzent, Animationen, Farben, Palette, Ansicht, Werkzeug-Standards, zuletzt geöffnete
Dateien, Verlauf, Threads und Effekte sofort an; der erste `set_states` die vier
Werkzeugfenster und das Standardwerkzeug (deren Module initialisieren sich erst nach
`set_init`); das Hauptfenster wird per Timer nach dem ersten Frame platziert.
Kommandozeile gewinnt: `--theme`, `--accent`, `--anim`, `--threads`, `--size` überschreiben
die Datei.

Schreiben (`core/ini.s`: Temp-Datei `settings.ini.tmp~` + `plat_file_sync` + Rename, Ordner
wird angelegt): beim Beenden (Strg+Q und Schließen des Hauptfensters: `set_save_quit` in
`main.s`, vor der „Änderungen speichern?“-Kette), beim Schließen des Einstellungsdialogs
(Schließen, Esc, ×) und per Testbefehl. Das INI-Objekt bleibt geladen: Kommentare,
unbekannte Zeilen/Abschnitte/Schlüssel überleben das Speichern.

Die Datei ist Eingabe: Zahlen müssen komplett passen (`ini_get_int`, Überlauf = ungültig) und
im Bereich liegen, sonst gilt der bisherige Wert; Fenstergrößen werden geklemmt; Farben genau
6 oder 8 Hexziffern; Zeichenketten werden vor dem Kopieren auf Länge geprüft (zu lang =
ignoriert); Aufzählungen nur über bekannte Namen. `version` > 1 wird gelesen wie 1
(unbekannte Schlüssel ignoriert) und beim Speichern auf 1 gesetzt.

## 2. Inhalt (`[Abschnitt] Schlüssel`)

| Abschnitt | Schlüssel | Bedeutung / Bereich |
|---|---|---|
| PaintCraft | version | 1 |
| UI | theme, accent, animations | `light`/`dark`/`system`; `RRGGBB` oder leer; 0/1 |
| MainWindow | x, y, width, height, maximized | Client-Ursprung in Bildschirm-px; Client-Größe in DIP (640..16384 × 400..16384, geklemmt); normale Größe auch wenn maximiert. Nur mit echtem Fenster (headless unverändert) |
| ToolsWindow, HistoryWindow, LayersWindow, ColorsWindow | visible, x, y (+ width, height bei Verlauf/Ebenen; expanded bei Farben) | WINRECT der wnd_*.s-Module: DIP relativ zum Client-Ursprung des Hauptfensters, ±20000 (sonst Standardplatz); Größe 120..4000 × 80..4000 |
| Colors | primary, secondary, palette | `AARRGGBB`; Palette = bis 96 Farben, kommagetrennt (ungültige Einträge übersprungen) |
| View | units, grid, rulers | `pixel`/`inch`/`cm`; 0/1; 0/1 |
| Tools | default, width, hardness, spacing, antialias, blend, fill, tolerance, flood, sampling, selection_mode, selection_quality, pressure, smoothing | Standardwerkzeug (englischer Name, z. B. `Paintbrush`, `Zoom`) + TOOLCFG-Standards (width f64 1..2000, sonst TC_*-Bereiche aus pn_tooli.inc) |
| RecentFiles | file1..file10 | neueste zuerst; < 1040 Byte |
| History | budget_mb, temp_dir | 0 = automatisch (min(40 % RAM, 4 GB)), bis 262144; Ordner der Auslagerungsdatei, leer = System-Temp |
| Performance | threads | 0 = automatisch, 1..64 (wirkt nur, wenn der Pool noch nicht lief → praktisch nach Neustart) |
| Effekte | … | `fx_settings_save/_load` (fx.s, EFFECTS.md) |

**Werkzeuge wie paint.net:** gespeichert werden die *Standardeinstellungen* (Dialogseite
Werkzeuge), nicht die zuletzt benutzte Symbolleiste. Beim Start gilt: aktives Werkzeug =
Standardtool, Symbolleiste = gespeicherte Standards. „Aus der Symbolleiste übernehmen“ kopiert
die aktuelle TOOLCFG in die Standards (das Standardtool bleibt), „Zurücksetzen“ setzt Standards
und Standardtool auf Werkseinstellungen (Farbpinsel, Werte aus `tool_init`) zurück; beides
ändert die laufende Symbolleiste nicht.

Fenster außerhalb aller Monitore (nur Windows): 800 ms nach dem Start prüft
`st_check_screens` mit `plat_win_get_rect` + `plat_win_work_area` (nächster Monitor), ob
mindestens 48 × 24 px im Arbeitsbereich liegen; sonst Hauptfenster zentriert, Werkzeugfenster
auf ihren Standardplatz (`*_reset_pos`). Unter X11/Wayland übernimmt das der Fenstermanager
(die gemeldeten X11-Positionen unter WSLg waren nicht konsistent, s. §7).

## 3. Dialog „Einstellungen“ (Alt+X, Titelleistenknopf; CMD_SETTINGS)

Modales, vergrößerbares Dialogfenster `WINID_SETTINGS` (0x6030 = 24624, 820 × 560 DIP), links
Seitenliste (`ID_SET_NAV`, Zeilen #5501..#5508), rechts die Seite in einer Scroll-Ansicht,
unten „Schließen“. Änderungen wirken sofort (wie paint.net), Schließen/Esc/× speichert.

| Seite | Inhalt |
|---|---|
| Benutzeroberfläche | Farbschema (Standard = System / Hell / Dunkel) #5530, Akzentfarbe (RRGGBB, leer = Standard; gültige Eingabe wirkt sofort, ungültige wird beim Verlassen zurückgesetzt) #5531, Sprache (nur Deutsch, deaktiviert) #5532, Animationen #5533 |
| Werkzeuge | Einleitung, Standardtool #5540, Zusammenfassung der Standards (5 Zeilen, Gruppen wie paint.net), Aus der Symbolleiste übernehmen #5541, Zurücksetzen #5542 |
| Verlauf (eigene Seite, HIST-012..014) | Speicherbudget MB #5550 (0 = automatisch, Anzeige des Automatikwerts), belegter Speicher RAM/Datei, Ordner der Auslagerungsdatei #5551 |
| Grafik | Rendering-Gerät „CPU – <Name>“, Befehlssätze (nur Anzeige; SIMD-Wahl nur per `--simd sse2`), Worker-Threads #5560 |
| Farbmanagement | Status „PaintCraft arbeitet im sRGB-Modus.“ + Hinweis (kein HDR/WCG) |
| Aktualisierung | alle Elemente deaktiviert + Begründung (keine Netzwerk-/TLS-Implementierung) |
| Diagnose | schreibgeschützter mehrzeiliger Text #5580 (Version, OS, CPU, Befehlssätze, Prozessoren/Threads, RAM, Verlauf, DPI/Theme, Bilder, Pfade: Einstellungsdatei, Temp, Paletten, geladene Module, Effektanzahl), „In die Zwischenablage kopieren“ #5581 |
| Plugin-Fehler | „Es gab keine Fehler beim Laden der Plugins.“ + leere Pluginliste (Platzhalter bis M8) |

Texte aus `PaintDotNet.Strings.3.DE.resources` (SettingsDialog.*, Form.CloseButton,
ExceptionDialog.CopyToClipboardButton, Program.ShowDiagnostics.Message; „Paint.NET“ →
„PaintCraft“, Emoji und `&`-Mnemonics entfernt). Eigene Texte (keine paint.net-Quelle):
Seite Verlauf, Akzentfarbe, Befehlssätze, Worker-Threads, alle Fußnoten außer Sprache/Updates,
Diagnose-Zeilen. Nicht übernommen: Leinwand, Effekte (Standardqualität), Stift & Tablett,
Farbschema „Blau“, Transparente Fenster, Overscroll u. ä. (keine Entsprechung in PaintCraft).

## 4. Befehle (850..899) und Timer

| ID | Befehl |
|---|---|
| 850 CMD_SET_SAVE | Datei jetzt schreiben (Testhook) |
| 851 CMD_SET_RELOAD | Werkszustand + Datei neu lesen (In-Prozess-Rundreise) |
| 852 CMD_SET_FACTORY | Werkszustand aller persistierten Werte (Theme/Akzent werden nur als Wert zurückgesetzt) |
| 853 CMD_SET_PAGE arg | Dialog auf Seite SP_* öffnen |
| 854 / 855 | Aus der Symbolleiste übernehmen / Zurücksetzen |
| 856 | Diagnose in die Zwischenablage |
| 857 | Monitorprüfung jetzt |

Timer `TMR_SET` + 0 Monitorprüfung, +1 Maximieren, +2 `--settings-save-at MS` (Testhook für
echte Fenster: `--auto-close` beendet ohne UIC_CLOSE), +3 Hauptfenster platzieren, +4
Framebuffer an neue Client-Größe anpassen (§7).

## 5. Ergänzte M7-Funktionen in fremden Dateien (additiv, markiert)

`view.s` `view_set_opts(units, grid, rulers)`; `palette.s` `pal_set_all(colors, n)`;
`wnd_tools/history/layers.s` `wtools_rect/whist_rect/wlay_rect() -> &WINRECT`;
`wnd_colors.s` `wcol_rect`, `wcol_more`, `wcol_set_more(on)`; `fileio.s` (Block am Ende)
`fio_recent_count`, `fio_recent_path(i)`, `fio_recent_add(path)`; `main.s` zwei Aufrufe von
`set_save_quit` (vor CMD_EXIT-Routing und vor UIC_CLOSE → fio_cmd). `tests/skeleton.uis`
Zeile 22: `#710 enabled` statt `disabled`.

## 6. Tests

* `tests/settings.uis` (`#!args --config build/tests/pn_settings`): Dialog, alle Seiten,
  Theme/Akzent/Animationen, Standardtool + Übernehmen/Zurücksetzen, Budget, Temp-Ordner,
  Threads, deaktivierte Update-Elemente, Diagnose → Zwischenablage, Schließen/Esc speichern,
  Fenster verschieben/verstecken/Größe, Farben, Palette, Ansicht, dann 850 → 852 → 851 und
  Prüfung aller Werte. Win + Linux, auch `--verify-dirty`.
* `python tools/pn_settings_check.py [--target win|linux|both] [--out build/s1] [--window]`:
  echte Zwei-Prozess-Rundreise (Prozess 1 endet mit Strg+Q), Kommentar/unbekannte Abschnitte
  bleiben, zuletzt geöffnete Dateien; feindliche Datei (NaN, Überlauf, falsches Hex, 2000-Byte-
  Strings, unbekannte Namen); headless ohne Konfiguration schreibt nichts (User-Ordner wird
  geprüft); `$PaintCraft_CONFIG_DIR`; `--window`: echte Fenster (Hauptfenster bei -50000 → wieder
  auf dem Monitor, Werkzeugfenster → Standardplatz, maximiert + normale Größe bleiben;
  Linux mit `jfgui_BACKEND=x11`). Arbeitsordner `build/<out>/pn_settings_check/<target>/`.

Proben (versteckte K_TEXT-Records, `ID_SET_PROBE` = 5900 + k):
0 Datei (Wert 1 = Persistenz an) · 1 Anzahl geschriebener Dateien, Text ok/Fehler · 2 Datei
gelesen · 3 ST_THEME, Text Hell/Dunkel (angezeigt) · 4 Akzent `RRGGBB` · 5 Animationen,
Text `anim_off=N` · 6 Standardtool (Name) · 7 `tool_cur()` · 8 Standard-TOOLCFG
(`w=2 h=75 …`) · 9 aktuelle TOOLCFG · 10 `Primär Sekundär`, Wert Palettengröße · 11 erste und
letzte Palettenfarbe · 12 Einheit, Text `grid= rulers=` · 13 Budget-Einstellung MB, Text
Temp-Ordner · 14 Threads-Einstellung, Text `budget=` (aktiv, MB) · 15 zuletzt geöffnete
Dateien (Anzahl, neueste) · 16 Hauptfenster `x y w h max` · 17 Dialogseite · 18 Länge des
Diagnosetexts.

## 7. Offene Punkte / Bibliothekslücken

1. `ui_window_place(0, rect)` mit Größe: das OS-Fenster wird vergrößert, aber Win32 meldet
   das eigene WM_SIZE nicht (WW_SELF), der Framebuffer behält die alte Größe. Umgangen mit
   `plat_win_get_rect` + `ui_resize` 150 ms später (TMR_SET_FIX). Besser in `uwp_rect`.
2. Kein Exit-Hook in der Bibliothek (`g_quit` nicht global, `--auto-close` springt ohne
   UIC_CLOSE heraus): gespeichert wird bei Strg+Q/UIC_CLOSE über `main.s`.
3. SIMD-Wahl zur Laufzeit nicht möglich (`g_cpu_feat` privat, nur `--simd`): nur Anzeige.
4. Einzeiliges Textfeld in einem WF_DIALOG-Fenster ohne Primärknopf liefert bei Enter kein
   EV_COMMIT (Fokusverlust schon) – Akzentfarbe/Temp-Ordner werden daher schon bei EV_ACT
   übernommen.
5. X11 unter WSLg: `plat_win_get_rect`/`plat_win_work_area` lieferten für das Hauptfenster
   y-Werte, die nicht zur angeforderten Position passen (z. B. 90 → 1025); Monitorprüfung und
   Positionsabfrage daher nur unter Windows, Linux speichert den WINRECT der Bibliothek.
6. `fx_settings_load` prüft nur Länge/Hex der Effektparameter, nicht deren Bereiche (A6).
7. Monitorprüfung der Werkzeugfenster nutzt `uiw_index`/`uiw_pw` (global, aber intern).
