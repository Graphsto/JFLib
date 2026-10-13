# U3 – UI-Feinschliff (Bericht)

## Stand je Punkt

| # | Punkt | Stand |
|---|---|---|
| 1 | Werkzeug-Optionsleiste zu breit | **erledigt**: Optionen in horizontaler Scrollansicht (#1030, SC_H/SC_NOBAR), Seitenknöpfe „‹“ #1031 / „›“ #1032 erscheinen nur bei Überlauf (Entscheidung aus letzter Messung + eigener Breite, flackerfrei), Mausrad scrollt, Tab-Fokus wird eingeblendet. Neu: `ui_scroll_extent(id)`. |
| 2 | Popups am Fensterrand abgeschnitten | **erledigt**: Flyouts/Menüs/Kombolisten/Kontextmenüs/Tooltips sekundärer Fenster dürfen das Fenster verlassen (Bereich = Arbeitsbereich des Monitors, headless = virtueller Desktop). Teile außerhalb bekommen einen Slot (Oberfläche mit den verschobenen Popup-/Tooltip-Ebenen) → echtes OS-Popup: Win32 (WS_POPUP owned, TOOLWINDOW|NOACTIVATE, CS_DROPSHADOW, runde Region), X11 (override-redirect, TranslateCoordinates), Wayland (xdg_popup mit Positioner, Slide). Eingaben kommen als Ereignisse des Besitzers zurück. Zu hohe Flyouts scrollen (alle Fenster). Leichtes Schließen über Fenstergrenzen. Headless: `shot desktop` zeigt die Popups. PaintCraft-Workarounds entfernt: Ebenen-Zeilenmenü mit vollen Befehlsnamen+Kürzeln, kein LIST_ROOM im Eigenschaften-Dialog, Palettenmenü mit allen Dateien. |
| 3 | „layout did not settle within 4 passes“ | **erledigt**: Ursache war eine neue Verlaufsliste-Zeile bei Scroll-Follow (Zeile erst im 3. Durchgang, ihr Inhaltskasten brauchte einen 4. zum Messen). Inhaltskästen eigener Listenzeilen sind jetzt BOX_FIXED. Test `settle.uis` (mehrere Auswahlen, Strg+D, neue Ebene, Einfügen+Enter, Farbeimer/Verlauf+Enter, Verschieben). Layout-Trace zeigt jetzt den Durchgang (`layout f<n>:`). |
| 4 | Tasten in Werkzeugfenstern | **erledigt**: Ein Druck in ein Werkzeugfenster lässt es die Tasten „ausleihen“, solange ein Widget darin Fokus hat; unverbrauchte Tasten/Text gehen vor Akzeleratoren/Tab/Esc an den Besitzer (ein Zusatzdurchgang). Mit offenem Popup behält das Fenster alle Tasten. Win32-Mausrad geht an das Fenster unter dem Zeiger (auch Popups). windows.uis angepasst (Pfeil bewegt jetzt den Schieberegler). |
| 5 | Titel schmaler Werkzeugfenster | **erledigt**: Titel füllt die Leiste mit „…“ (TX_ELLIPSIS). |
| 6 | `ui_window_place(0,…)` mit Größe | **erledigt** in `uwp_rect` (Clientgröße nach `plat_win_set_rect` → `ui_resize`); Workaround TMR_SET_FIX in settings.s entfernt. |
| 7 | Enter ohne Primärknopf | **geprüft, kein Fehler** (EV_COMMIT kommt; auch im Einstellungsdialog). Regressionstest `dlgenter.uis` (+ Textfeld im verschachtelten Galerie-Dialog, nur Schließen-Knopf). |
| 8 | `--auto-close` ohne UIC_CLOSE | **erledigt**: neues `UIC_EXIT` (jedes Ende eines echten Fensterlaufs, nie bei --script/--shot); PaintCraft speichert dort. `pn_settings_check.py --window` Szenario `autoclose` (beide Ziele). |
| 9 | X11 inkonsistente y-Werte | **teilweise**: Analyse – X-Root = virtueller Windows-Bildschirm auf 0,0 verschoben (hier Hauptmonitor bei Root-y 1006), Lücken ohne Monitor, Weston verschiebt dorthin angeforderte Fenster (90→1025). get_rect/E_WINPOS sind konsistent (gemessen). `plat_win_work_area` liefert jetzt den Monitor des Fensters (RandR 1.5, `x11_monitor.s`). Offen: Monitorprüfung von settings.s auch unter X11 aktivieren (S1-Logik). |
| 10 | history.uis --verify-dirty | **erledigt**: Ebenen-Vorschaubild – eine freigegebene und an gleicher Adresse neu angelegte SURFACE begann wieder mit SF_AUX=1 → Bildbefehl unverändert. `wl_thumb` vergibt jetzt nie wiederholte Versionen. `--verify-dirty` nennt nun das Fenster. |
| 11 | NK_SQUARE als Scheibe | **erledigt**: Quadrat mit Schatten, Füllung, 1-px-Rand. |

## Tests
* `python build.py test` war zum Schluss durch fremde Baustellen blockiert (CX `jxr_ent.s` assembliert nicht, CE `webp_vp8_enc.s` referenziert fehlendes `ve_assemble`; asmcheck-Fehler in webp/plib/plugins/print). Deshalb alle Skripte beider Ziele über `build/u3/runtests.py` mit lokalen Stubs nur für diese fremden Symbole: **alle bestanden** (Win + Linux). Baseline am Anfang: alle bestanden.
* Neue Skripte: gallery `popups.uis`, `toolkeys.uis`, `wintitle.uis`, `dlgenter.uis`; PaintCraft `toolrow_overflow.uis`, `settle.uis`, `toolkeys.uis`; alle neuen + betroffene auch mit `--verify-dirty` sauber (inkl. history.uis).
* Echte Fenster: `tools/wincheck.py windows` und neu `wincheck.py popups` (Win32), `tools/x11check.py windows` und neu `x11check.py popups` auf X11 und Wayland (WSLg): alle bestanden. `pn_settings_check.py --target both --window`: alle bestanden.
* asmcheck auf allen berührten Dateien: 0 Fehler.

## Geänderte / neue Dateien
Bibliothek: `ui/ui_popup.s` (Bereich, Scroll-Flyouts, Tooltip, `ui_popup_ext/_count`), `ui/ui_win.s` (Slots, Popup-Bereich, Tasten-Ausleihe, Light-Dismiss, uwp_rect), `ui/ui_main.s` (Render-Hooks, UIC_EXIT, Weiterleitungs-Durchgang, Debug), `ui/ui_ctx.s` (Tastenhelfer, Hit-Test mit negativen Punkten), `ui/ui_cmd.s` (`cmd_render_popup`, `cmd_translate`), `ui/ui_list.s` (`ui_scroll_extent`, BOX_FIXED-Zeilen), `ui/ui_chrome.s` (Titel „…“), `ui/ui_layout.s` (Trace), `ui/ui_auto.s` (Desktop-Shot), `inc/ui.inc` (UIC_EXIT), `plat/win/win_popup.s` (neu), `plat/win/win_window.s` (Mausrad, Leave, `plat_win_hwnd`, `win_mods`), `plat/linux/lx_popup.s`, `x11_popup.s`, `x11_monitor.s`, `wl_popup.s` (neu), `x11_window.s` (Routing-Hook, Arbeitsbereich, `g_x11_cmap` global), `wl_input.s` (Popup-Offset), `wl_proto.s` (xdg_positioner/xdg_popup), `tools/implib/user32.def` (+SetWindowRgn), `gdi32.def` (+CreateRoundRectRgn).
PaintCraft: `chrome.s`, `inc/pn_ids.inc` (1030–1032), `nubs.s`, `settings.s`, `inc/pn_set.inc`, `main.s` (UIC_EXIT), `wnd_layers.s`, `dlg_layerprops.s`, `palette.s`; Tests `layers_window.uis`, `colors.uis`.
Galerie: `main.s` (Werkzeugfenster „Popups“, Hook 9104 / `--popups-window`, Dialog-Textfeld), `windows.uis`.
Werkzeuge: `tools/wincheck.py popups`, `tools/x11check.py popups`, `tools/pn_settings_check.py` (autoclose).
Doku: `docs/modules/WINDOWS.md` §7.9–7.11, `WINDOWS_PLATFORM.md` §8–9, `WIDGETS.md`, `GUIDE.md` §6, PaintCraft `TOOLS.md`, `LAYERS.md`, `COLORS.md`, `SETTINGS.md`, `GRADIENT.md`, `docs/wip/_todo.md`.

## Offen / Hinweise
* Popups des **Hauptfensters** bleiben im Fenster (Scrollen bei Übergröße); X11-Popups ohne runde Ecken (kein SHAPE); Wayland-Popups nur ganzzahlige Skalierung, Neuaufbau bei Positionswechsel (reposition braucht v3).
* API.md nicht neu erzeugt (enthielte halbfertige Symbole anderer Agenten) – beim Meilenstein `python tools/gendoc.py`.
* CLAUDE.md „Testing“ könnte `wincheck.py popups` / `x11check.py popups` nennen (geteilte Datei, nicht geändert).
* Tooltips sind weiterhin einzeilig (max. 300 DIP, lange Texte abgeschnitten).
