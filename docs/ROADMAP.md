# Roadmap: Was JFLib zur vollwertigen GUI-Library fehlt

Bestandsaufnahme vom 2026-10-16, erstellt anhand von `lib/ui`, `lib/plat`, `lib/text`,
[`Todo.md`](../Todo.md) und [`docs/wip/_todo.md`](wip/_todo.md). Wo schon etwas existiert,
steht es unter „Vorhanden“, damit klar ist, worauf aufgebaut werden kann.

**Priorität:** **P1** = ohne das ist eine GUI-Library nicht vollwertig · **P2** = wird
erwartet · **P3** = optional / später
**Status:** *fehlt* · *teilweise* (Basis vorhanden) · *in Arbeit* · *erledigt*

Bekannte Grundgrenzen (Entscheidung „kein libc“): kein GPU-Rendering, kein fontconfig,
keine Barrierefreiheits-Schnittstelle (UIA / AT-SPI). Sie stehen unten mit in der Liste,
damit die Liste vollständig ist.

---

## 1. Berechtigungen und Sicherheit (wie bei Tauri)

Vorhanden: [`plat/capabilities.rs`](../lib/plat/capabilities.rs) meldet, welche Dienste der
Host bietet (13 `CAP_*`-Bits). Seit Paket 1 und 2 (2026-10-16) gibt es das Manifest
`apps/NAME/PERMISSIONS` ([PERMISSIONS.md](PERMISSIONS.md),
[`plat/policy.rs`](../lib/plat/policy.rs), [`ui/perm.rs`](../lib/ui/perm.rs)).

- **P1** · *erledigt* · Berechtigungs-Manifest pro App (`apps/NAME/PERMISSIONS` neben
  `TARGETS` / `STYLE`); `build_rs.py` prüft es, `ui::main!` installiert es, `plat` setzt es
  durch. Ohne Datei bleibt alles erlaubt.
- **P1** · *erledigt* · Scopes: Dateien unter Mustern mit `$CONFIG` / `$TEMP` / `$HOME` /
  `$EXE` / `$FONTS`, vom Nutzer gewählte Dateien (`fs.user`: Dialog, Drop, Kommandozeile),
  Prozesse, `open_url`-Schemes, Plugins, Drucken und Netz (`net.connect` / `net.listen`:
  Namen, `*.domain`, IPs, Adressblöcke, `localhost`, Port-Bereiche; Namen werden beim
  Auflösen geprüft, ihre Adressen dann vorgemerkt).
- **P1** · *teilweise* · Zentrale Durchsetzung in `plat` (Dateien, `fs.rs`, Prozesse, Shell,
  Plugins, Drucken, Sockets und Namensauflösung inkl. `netio::dns`) statt in jeder App;
  Verweigerungen als `PERMDENY` auf stderr und in `plat::policy::denials()`,
  `capabilities()` blendet verweigerte Dienste aus. Es fehlt noch: Programme aus `BINS`,
  `rt::main!`-Konsolenprogramme und Android bekommen das Manifest noch nicht.
- **P2** · *fehlt* · Laufzeit-Rückfragen („App möchte auf … zugreifen“) mit gespeicherter
  Entscheidung und Widerruf in den Einstellungen.
- **P2** · *fehlt* · Sicherer Speicher für Geheimnisse: Windows Credential Manager / DPAPI,
  Linux Secret Service (D-Bus), Android Keystore.
- **P2** · *fehlt* · Rechte für native Plugins und Module (heute voller Prozesszugriff) und
  Rechte pro Aktion für die Agent-/MCP-Steuerung (heute nur ein globales Einschalten).
- **P3** · *fehlt* · OS-Sandbox nutzen: Windows AppContainer / MSIX-Capabilities, Linux
  Landlock / seccomp, Flatpak-Portale.
- **P3** · *fehlt* · OS-Berechtigungen für Kamera, Mikrofon, Standort (Android-Laufzeitrechte,
  Portale).

## 2. Virtuelles Dateisystem und Ressourcen

Vorhanden: [`plat/vfs.rs`](../lib/plat/vfs.rs) ([VFS.md](VFS.md)) seit 2026-10-16,
[`plat/web_files.rs`](../lib/plat/web_files.rs) (virtuelle Dateien im Browser),
[`plat/fs.rs`](../lib/plat/fs.rs), [`plat/watch.rs`](../lib/plat/watch.rs),
[`lib/archive`](../lib/archive/lib.rs).

- **P1** · *teilweise* · Einheitliche VFS-Schicht für nativ, Web und Android: Alle
  Datei-Funktionen von `plat` verstehen `schema://pfad`. Fertig sind:
  - `app://` mit den eingebetteten Assets
  - `config://`, `data://`, `cache://` als App-Ordner, durch die Berechtigungen immer erlaubt
  - `mem://` im Speicher
  - eigene Mounts: `DirFs`, `Overlay` für Themes und Mods, `Sub`, `Bundle`, ZIP über
    `archive::ZipFs`

  Es fehlen noch:
  - Datei-Handles aus Portalen bzw. Dokumenten (Flatpak, Android SAF, Browser)
  - `plat::fs` (stat, Links, Zeiten) und `File`-Streams für Speicher-, Bundle- und ZIP-Mounts
  - Whiteouts im Overlay
- **P1** · *erledigt* · Assets beim Build bündeln: `apps/NAME/assets/` wird von
  `tools/assetpack.py` als JFA1 gepackt (LZ4 wie `lib/compress`, „stored“, wenn LZ4 nicht
  mindestens ein Zehntel spart) und per `include_bytes!` eingebettet. Der Inhalts-Hash steuert
  den Rebuild, `ui::main!` mountet das Bundle als `app://`. Programme aus `BINS` und
  Android-Builds bekommen noch keine Assets.
- **P1** · *erledigt* · Pfade normalisieren und `..`-Ausbrüche verhindern
  (`plat::policy::normalize`: Windows-Präfixe, UNC, Laufwerke, Geräte- und Stream-Namen;
  unter Linux wird `..` abgelehnt, weil der Kernel es erst nach Symlinks anwendet). Die
  Prüfung ist lexikalisch: Symlinks in erlaubten Ordnern können noch hinausführen.
- **P2** · *teilweise* · Alle Standardordner: `data_dir`, `cache_dir` und die App-Ordner
  (`vfs::app_dir`) gibt es jetzt neben `config_dir`, `temp_dir`, `home`. Es fehlen noch:
  `log_dir`, Dokumente, Downloads, Bilder (XDG-Benutzerordner, Known Folders;
  `win_inst::known_folder` nur für den Installer).
- **P2** · *fehlt* · In den Papierkorb verschieben statt löschen (Windows `IFileOperation`,
  freedesktop-Trash-Spezifikation).
- **P2** · *fehlt* · Asynchrone bzw. streamende Datei-API mit Fortschritt und Abbruch.
- **P3** · *fehlt* · „Zuletzt verwendet“ auf OS-Ebene (Jump Lists, `recently-used.xbel`).

## 3. Desktop-Integration

- **P1** · *erledigt* · Nur eine Instanz (`AF_SINGLE_INSTANCE`, `plat::instance`): Spätere
  Starts geben Ordner und Kommandozeile an die laufende Instanz (Windows: Named Pipe pro
  Nutzer-SID; Linux: abstrakter Socket pro UID; beide Seiten prüfen den Nutzer). Das Fenster
  kommt nach vorn, die App bekommt `UIC_OPEN`. PaintCraft öffnet so Dateien als Tabs.
- **P1** · *teilweise* · Dateizuordnungen und URL-Schemes: `PACKAGE` `filetypes=` / `schemes=`.
  Das Windows-Setup registriert ProgIDs, „Öffnen mit“ und Schemes unter HKCU (das Deinstallieren
  entfernt sie wieder), das .deb trägt `MimeType=` und `%U` / `%F` ein, eigene Typen bekommen
  ein shared-mime-info-Paket. Es fehlen noch: Registrierung ohne Setup (Entwickler-Builds),
  macOS, MSIX / Flatpak.
- **P1** · *fehlt* · OS-Benachrichtigungen (Windows-Toast, `org.freedesktop.Notifications` /
  Portal). Heute nur Toasts im Fenster ([`ui/notify.rs`](../lib/ui/notify.rs)).
- **P1** · *fehlt* · Symbol im Infobereich (`Shell_NotifyIcon`, StatusNotifierItem über D-Bus).
- **P2** · *teilweise* · Systemeinstellungen live übernehmen:
  - Dark Mode unter Linux (`org.freedesktop.appearance` / `color-scheme`); heute nur Windows
    über `win_shell::theme`
  - Akzentfarbe des Systems
  - hoher Kontrast
  - „Animationen reduzieren“
  - Textskalierung
  - Doppelklickzeit, Blinkrate des Cursors
  - Systemsprache
- **P2** · *teilweise* · Drag & Drop: Heute kommen nur Dateien herein (Windows
  `WM_DROPFILES`, X11 Xdnd als Ziel). Es fehlen Drag aus der App heraus (`DoDragDrop`,
  `wl_data_device.start_drag`, Xdnd als Quelle), Text/Bild/HTML als Drop-Inhalt
  (`IDropTarget`) und allgemeines Drag & Drop zwischen Widgets (nicht nur `LF_REORDER`).
- **P2** · *fehlt* · Strom und Sitzung: Standby verhindern (`SetThreadExecutionState`,
  Inhibit-Portal), Ereignisse bei Ruhezustand / Aufwachen, Abmelden / Herunterfahren
  (`WM_QUERYENDSESSION`).
- **P2** · *fehlt* · Autostart (Run-Schlüssel, XDG-Autostart bzw. Background-Portal).
- **P2** · *fehlt* · Taskleiste: Fortschritt, Overlay-Badge, Jump Lists (`ITaskbarList3`).
- **P3** · *fehlt* · Globale Tastenkürzel; Screenshot- und Farbwähler-Portal (Pipette
  außerhalb des Fensters); öffentliche Monitor-API (Liste, DPI, Arbeitsbereich).
- **P3** · *fehlt* · Audio-Ausgabe (WASAPI, PipeWire / ALSA), auch Systemklänge; später Video,
  Kamera, Mikrofon.
- **P3** · *fehlt* · WebView einbetten (WebView2, WebKitGTK); ohne libc schwierig, bewusst
  zu entscheiden.

## 4. Text und Schrift

Vorhanden: [`text/fontdb.rs`](../lib/text/fontdb.rs) (durchsucht die Font-Ordner selbst),
HarfRust-Shaping, Bidi beim Shaping, IME unter Wayland (text-input-v3), Windows und Android.

- **P1** · *fehlt* · Glyph-Fallback: Fehlt eine Glyphe, wird eine andere Schrift genommen
  (Emoji, CJK, Arabisch, Symbole). In `lib/text` gibt es nur `.notdef`; siehe auch
  `_todo.md` (T5).
- **P1** · *fehlt* · IME unter X11: XIM bzw. IBus / Fcitx über D-Bus.
- **P2** · *fehlt* · fontconfig-Ersatz light: Aliase (`sans-serif` / `serif` / `monospace`),
  Schriftwahl je Sprache, Hinting- und Subpixel-Einstellungen des Systems.
- **P2** · *fehlt* · Widget bzw. Dialog zur Schriftauswahl.
- **P2** · *fehlt* · Rechtschreibprüfung (Hunspell-Wörterbücher selbst lesen; Windows
  `ISpellChecker`).
- **P2** · *prüfen* · Unicode-Segmentierung: Zeilenumbruch nach UAX #14, Grapheme und Wörter
  nach UAX #29 für Cursor, Ctrl+Pfeil und Doppelklick.
- **P3** · *fehlt* · Silbentrennung, Subpixel-Kantenglättung (ClearType-artig), Achsen
  variabler Fonts in der UI.

## 5. Internationalisierung

Vorhanden: [`ui/i18n.rs`](../lib/ui/i18n.rs) mit TSV-Katalogen; Bibliothekstexte nur
`LANG_DE` / `LANG_EN`, Deutsch als Quellsprache; einstellbares Dezimaltrennzeichen.

- **P1** · *fehlt* · RTL-Layout: Boxen, Scrollbars, Icons und Menüs spiegeln. Das Shaping kann
  Bidi, das Layout nicht.
- **P1** · *fehlt* · Pluralregeln (CLDR) und Platzhalter mit Namen bzw. Index statt nur `{}`
  in fester Reihenfolge.
- **P1** · *teilweise* · Locale-Formate für Zahlen, Datum / Uhrzeit, Währung, Einheiten;
  Systemsprache automatisch erkennen. Heute nur Dezimaltrennzeichen und Monatsnamen.
- **P2** · *teilweise* · Bibliothekstexte in weiteren Sprachen; besser Englisch als
  Quellsprache bzw. Schlüssel.
- **P2** · *fehlt* · Sortierung nach Locale (Collation), Unicode-korrekte
  Groß- / Kleinschreibung.
- **P3** · *fehlt* · Werkzeug, das Übersetzungsschlüssel extrahiert und fehlende Einträge
  meldet.

## 6. Widgets und Layout

Gut abgedeckt: Grid, Tree, Ribbon, Splitter, Breadcrumb, Wizard, Charts, Datums- und
Farbwahl, Toasts, mehrere echte Fenster.

- **P2** · *fehlt* · Echtes Grid-Layout (Zeilen / Spalten, Spannen, Ausrichtung); heute nur
  Boxen H / V mit Wrap.
- **P2** · *teilweise* · Docking-Framework (andockbare und schwebende Panels); heute nur in
  PaintCraft ([`dock.rs`](../apps/PaintCraft/src/workspace/panels/dock.rs)).
- **P2** · *teilweise* · Allgemeine Undo/Redo-History für Apps (`ui::History`); heute nur in
  der Textbox und in `doc_core`.
- **P2** · *teilweise* · Fertiges Rich-Text-Widget (heute nur das Low-Level-`textbox_surface`);
  Code-Editor mit Syntaxhervorhebung.
- **P2** · *fehlt* · Popups und Tooltips über den Fensterrand hinaus; laut `_todo.md` werden
  sie am Toolfenster abgeschnitten.
- **P2** · *teilweise* · Animationen mit Easing-Kurven und Übergängen; `anim()` liefert nur
  einen Wert von 0 bis 256.
- **P3** · *fehlt* · SVG-Renderer für Bilder und Icons (heute nur Pfaddaten), Markdown- /
  HTML-Viewer, Bildbetrachter mit Zoom und Verschieben, Listen mit variabler Zeilenhöhe.
- **P3** · *fehlt* · Feste Obergrenzen lockern: `TIMERS_MAX` 16, `KEYQ_MAX` 16, `CMDQ_MAX` 32
  ([`ui/ctx.rs`](../lib/ui/ctx.rs)).

## 7. App-Framework und Architektur

- **P1** · *fehlt* · Logging-Framework: Stufen, Dateirotation, Ringpuffer für Absturzberichte.
  Heute nur `Ui::warn()` auf stderr.
- **P1** · *fehlt* · Lebenszyklus-Hooks: Start, Beenden, Fokus, Ruhezustand, Einstellungen
  geändert; dazu ein Exit-Hook (laut `_todo.md` (S1) fehlt er bei `--auto-close`).
- **P2** · *teilweise* · Typisierte Einstellungen mit Schema, Migration und automatisch
  erzeugter Einstellungsseite; Fensterzustand und Paletten automatisch speichern. Heute nur
  `base::Ini` von Hand.
- **P2** · *teilweise* · Asynchrone Ausführung (Executor in der Ereignisschleife), nicht
  blockierende Netz- und Dateiaufrufe; heute `plat::Job` + `UIC_WAKE`.
- **P2** · *fehlt* · Hilfen für Widget-IDs (ID-Stack bzw. Hashing); heute `u32`-IDs von Hand
  mit Kollisionsgefahr.
- **P3** · *fehlt* · Stil zur Laufzeit wechseln bzw. eigene Themes für Apps ohne neues
  Style-Crate.

## 8. Rendering

- **P2** · *fehlt* · GPU-Rendering (D3D11 / Vulkan / OpenGL / WebGPU); heute rechnet alles
  die CPU.

## 9. Plattformen

- **P1** · *fehlt* · Native macOS-Version (Cocoa über `objc_msgSend` ohne libc ist eigener
  Aufwand); heute macOS nur über das Web.
- **P2** · *fehlt* · Native iOS-Version.
- **P2** · *teilweise* · Android vervollständigen: Dateiauswahl, dauerhafter Speicher,
  System-Zwischenablage.
- **P2** · *fehlt* · Safari / iOS im Web-Testplan.

## 10. Auslieferung und Updates

Vorhanden: `.deb`, Windows-Setup ([`plat/win_inst.rs`](../lib/plat/win_inst.rs),
[`tools/package.py`](../tools/package.py)), Absturzhandler; Update nur in BackupCraft
eingebaut ([`server/update.rs`](../apps/BackupCraft/server/update.rs)).

- **P1** · *teilweise* · Auto-Updater im Framework mit Signaturprüfung (`lib/crypto` ist
  vorhanden) und Rollback.
- **P1** · *fehlt* · Code-Signierung: Windows Authenticode, signierte `.deb` / Repos,
  Release-Schlüssel für Android (heute Debug-Key, auch bei `--release`).
- **P2** · *fehlt* · MSIX / Microsoft Store, Flatpak, RPM, AppImage, AAB für Google Play;
  später Notarisierung für macOS.
- **P2** · *in Arbeit* · Versionsverwaltung.

## 11. Barrierefreiheit

Basis vorhanden: `sem()`-Records (semantischer Widget-Baum), Fokusrahmen, Mnemonics.

- **P1** · *fehlt* · Die Records abbilden auf UIA (Windows), AT-SPI (Linux, D-Bus),
  `AccessibilityNodeInfo` (Android) und einen ARIA-Schattenbaum im DOM (Web).
- **P1** · *fehlt* · Theme mit hohem Kontrast; System-Kontrast übernehmen.
- **P2** · *fehlt* · „Animationen reduzieren“ (heute nur `--anim off`), Textskalierung
  getrennt von der UI-Skalierung, Cursorposition für Bildschirmlupen.
- **P2** · *teilweise* · Vollständige Tastaturbedienung (laut `_todo.md` fehlt sie z. B. im
  Ebenenfenster von PaintCraft).

## 12. Entwickler-Erlebnis

- **P2** · *fehlt* · Cargo-Integration bzw. Veröffentlichung als Crates; heute baut nur
  `build_rs.py` mit `rustc`, fremde Crates sind schwer einzubinden.
- **P2** · *fehlt* · Projekt-Generator (`build_rs.py new NAME`), stabile API mit SemVer,
  Changelog.
- **P2** · *fehlt* · Website und Dokumentation für die Library.
- **P3** · *teilweise* · Hot-Reload, Live-Inspector bzw. Debug-Overlay (`--dump` als Basis).
