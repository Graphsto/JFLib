# jfgui Pocket Gallery für Android

Kleine native Test-App mit den vorhandenen jfguiLib-Widgets: Buttons mit Klickzähler,
Checkbox, Schalter, Slider, Fortschrittsanzeige, Akzentfarben, hellem/dunklem Design und
Unicode-Textfeld. Vertikal wischen scrollt die Seite; horizontal ziehen bewegt den Slider.
Die App benötigt keine Berechtigungen und keinen Netzwerkzugang.

## APK bauen

Vom Repository-Verzeichnis:

```powershell
rustup target add aarch64-linux-android
python RustLib/build_rs.py app androidGallery --target android --out build/android-work
```

Ergebnis: `RustLib/build/android-work/android/androidGallery-arm64-v8a-debug.apk`.
Die APK enthält echtes AArch64-Maschinenprogramm in `lib/arm64-v8a/libjfgui_android.so`.
Sie ist für **Android 8.0 / API 26 oder neuer mit ARM64-Unterstützung** gedacht. 32-Bit-ARM
ist nicht enthalten. Der Build richtet ELF-Segmente und unkomprimierte APK-Bibliotheken
auf 16 KiB aus und prüft die APK-Signatur sowie die ZIP-Ausrichtung.

Voraussetzungen: Rust, Python, JDK 17+, Android SDK Platform 36, Build-Tools 35+ und NDK
r29. `ANDROID_SDK_ROOT`/`ANDROID_HOME` und `ANDROID_NDK_HOME` können die automatisch
erkannten SDK-/NDK-Pfade überschreiben. Mit der neuen Android CLI lassen sich die
benötigten Pakete beispielsweise mit `android sdk install ndk/29.0.14206865
platforms/android-36 build-tools/36.0.0` installieren. Es werden weder Cargo-Abhängigkeiten
noch Gradle benötigt. `--release` optimiert den Rust-Code; auch dieser Test-Build nutzt
bewusst den lokalen Debug-Schlüssel. Er ist kein Release für den Play Store.

Installation, wenn gewünscht:

```powershell
adb -s DEVICE_SERIAL install -r RustLib/build/android-work/android/androidGallery-arm64-v8a-debug.apk
adb -s DEVICE_SERIAL shell am start -n org.jfgui.gallery/.GalleryActivity
```

Für einen x86_64-Emulator:

```powershell
rustup target add x86_64-linux-android
python RustLib/build_rs.py app androidGallery --target android --android-abi x86_64 --out build/android-work
```

## Was bereits portiert ist

- `rt` verwendet unter Android Bionic für ausgerichtete Allokationen und Logcat für
  Diagnoseausgaben. Es gibt keine feste Annahme über die Speicher-Seitengröße.
- ARM64 verwendet die vorhandenen portablen Float-/Math- und skalaren SIMD-Implementierungen.
  Spezifische NEON-Kernel sind noch nicht umgesetzt.
- Der bestehende Host-Adapter in `lib/plat/web*.rs` wird für Android nativ kompiliert:
  Ereignisse, Fenster-Komposition und der besitzende UI-Laufzeitkern sind derselbe Code.
- `android/bridge.c` überträgt die BGRA-Pixel in ein Android-Bitmap. Die schmale Java-Hülle
  kümmert sich um Lebenszyklus, Insets, Touch, Hardwaretastatur und IME-Eingabe.
  Widgets, Layout, Schriften und Rendering kommen vollständig aus der Rust-Library.
- Die Java-Hülle zeichnet nur geänderte Bitmaps. JNI sperrt und kopiert ausschließlich
  bei gemeldeten Änderungen; im Leerlauf plant die Library den nächsten Timer statt
  ständig GPU-Uploads auszulösen. Neue Eingaben wecken den Frame-Callback sofort.
- `GalleryInputConnection` spiegelt den fokussierten Rust-Editor als echtes Android-
  `Editable`. Composing-Text erscheint bereits vor `commitText`; Textauswahl, Löschen,
  UTF-16/UTF-8-Positionen und die tatsächlich akzeptierte Textlänge bleiben synchron.
- Der vorhandene Build-Einstieg unterstützt das explizite Ziel `--target android`.
  Weitere Apps müssen Android in `TARGETS` aufnehmen und ein Manifest/Ressourcen ergänzen.

## Grenzen dieses Prototyps

Der Adapter bietet eine einzelne Android-Activity und darin die Library-Oberflächen.
Dateien bleiben im virtuellen Arbeitsspeicher; Galerie-Zustand geht beim Beenden des
Prozesses verloren. Persistenz, Android-Dateiauswahl, System-Clipboard, Drucken, native
Plugins, Hintergrundthreads und Barrierefreiheitsknoten für einzelne Widgets sind noch
nicht angeschlossen. Nicht verfügbare Host-Anfragen enden mit `Unsupported`.
Die IME-Anbindung unterstützt Composing-Text, Commit, Auswahl, umgebenden Text und
Korrekturen. Composing-Unterstreichungen, Android-Auswahlgriffe und weitergehende
Editor-/Tastaturvarianten sind noch nicht umgesetzt. Android zeichnet die Oberfläche
auf seinem UI-Thread; für große Apps sollte Rendering in einen eigenen Thread wechseln.

Diese APK belegt den ersten nativen Android-Port der Rust-Library. Sie bedeutet noch
keine vollständige Android-Unterstützung aller Desktop-Apps und Betriebssystemdienste.
Die eingefrorene x86-Assembler-Referenz in `legacy/` bleibt unverändert.

## Prüfungen

Die gleiche Galerie kann auf dem Desktop über die vorhandene Skriptsprache geprüft werden:

```powershell
python RustLib/build_rs.py app androidGallery --target win --out build/android-work
cd RustLib
build/android-work/win/androidGallery.exe --script apps/androidGallery/tests/basic.uis --size 390x1100 --theme light --anim off --verify-dirty
```

Das Skript prüft Klickzähler/Reset, Checkbox, deaktivierten Slider, Sliderwerte,
Farbauswahl, Theme-Wechsel, Texteingabe und die Sichtbarkeit des fokussierten Textfelds
bei verkleinerter Fensterhöhe. Zusätzlich muss die native APK auf Android
gestartet werden; ein erfolgreicher Desktop-Test ersetzt den Android-Laufzeittest nicht.

Der gemeinsame Host lässt sich außerdem als WASM-Modul prüfen:

```powershell
python RustLib/build_rs.py app androidGallery --target web --web-mode scalar --out build/android-work
cd RustLib
node apps/androidGallery/tests/host.mjs build/android-work
```

Am 12.10.2026 geprüft: ARM64-APK auf einem Samsung SM-A566B mit Android 16 / API 36
installiert und gestartet; Darstellung und Widget-Bedienung auf dem Gerät beobachtet.
Auf einem x86_64-Emulator mit Android 15 / API 35 wurden zusätzlich Touch, Scrollen,
Slider, Theme-Wechsel, Bildschirmtastatur, Texteingabe, Drehung und Rückkehr aus dem
Hintergrund geprüft, einschließlich einer Wischgeste direkt nach dem Kaltstart.
Desktop-Skripttest mit `--verify-dirty` und WASM-Host-Test bestanden. Die Build-Pipeline
prüft ELF-Architektur, 16-KiB-Ausrichtung und APK-Signatur. Ein Gerät mit 16-KiB-Seiten
und ältere Android-Versionen wurden nicht zur Laufzeit getestet.

## Regressionstest für Android-Eingabe und Leerlauf

Die Debug-APK enthält einen explizit startbaren Instrumentation-Test. Er startet die
Galerie und verändert ausschließlich ihren Testtext:

```powershell
adb -s DEVICE_SERIAL shell am instrument -w -r org.jfgui.gallery/.GalleryInstrumentation
```

78 Prüfungen auf dem SM-A566B bestanden: sichtbare Komposition vor Commit, Ersetzung
und Abschluss ohne doppelte Zeichen, Umlaute, Emoji, Löschen nach Codepoints, Auswahl,
Korrektur eines Wortes, Batch-Edits, Textkapazität, veraltete Eingabesitzungen und
ausbleibende Bitmap-Updates bei unverändertem Inhalt. Der Test misst zusätzlich 60
Kompositionsänderungen einschließlich JNI, Rust-Layout und nativem Rasterisieren:
Median 0,84 ms, P95 1,75 ms, Maximum 9,45 ms. Das ist keine vollständige Messung vom
Fingerkontakt bis zur Ausgabe durch das Android-Display.

Bei der Kontrolle des ruhenden Vordergrundprozesses sank die beobachtete CPU-Auslastung
von etwa 68 % auf 0–1 %. In 32 Sekunden ohne Eingabe wurden keine neuen UI-Frames
gezeichnet. Die sichtbare Eingabe wurde zusätzlich mit der Samsung-Tastatur geprüft.
