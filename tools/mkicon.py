"""mkicon.py -- app icons: renders a glyph on a gradient square with apps/icongen (--shot) in every
icon size, cuts anti-aliased rounded corners and writes

  apps/NAME/res/app.ico          16 .. 256 px (PNG entries), linked into the Windows exe (build.py)
  apps/NAME/res/app_SIZE.png     48, 128, 256 px for Linux (hicolor icon theme, .deb)

  python tools/mkicon.py NAME --glyph E771 --color 1E78E6
"""
import argparse
import os
import struct
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pngutil  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
LINUX_SIZES = [48, 128, 256]


def ico_bytes(images):
    """images: list of (size, png bytes) -> .ico file"""
    head = struct.pack("<HHH", 0, 1, len(images))
    off = 6 + 16 * len(images)
    dirs, data = b"", b""
    for size, png in images:
        b = 0 if size >= 256 else size
        dirs += struct.pack("<BBBBHHII", b, b, 0, 0, 1, 32, len(png), off + len(data))
        data += png
    return head + dirs + data


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("app")
    ap.add_argument("--glyph", default="E771", help="icon font code point (hex, icons.inc)")
    ap.add_argument("--color", default="1E78E6", help="square colour RRGGBB")
    ap.add_argument("--out", default="build/icon", help="work directory")
    a = ap.parse_args()
    work = ROOT / a.out
    work.mkdir(parents=True, exist_ok=True)
    subprocess.run([sys.executable, str(ROOT.parent / "legacy" / "build.py"), "app", "icongen", "--target", "win",
                    "--out", a.out], check=True, cwd=ROOT)
    exe = work / "win" / "icongen.exe"
    res = ROOT / "apps" / a.app / "res"
    res.mkdir(parents=True, exist_ok=True)
    images = []
    for s in SIZES:
        shot = work / f"raw_{s}.png"
        subprocess.run([str(exe), "--shot", str(shot), "--size", f"{s}x{s}", "--glyph", a.glyph,
                        "--color", a.color], check=True, cwd=ROOT, capture_output=True)
        w, h, px = pngutil.read_png(shot)
        if (w, h) != (s, s):
            raise SystemExit(f"icongen rendered {w}x{h} instead of {s}x{s}")
        pngutil.round_corners(w, h, px, max(2.0, s * 0.22))
        png = pngutil.png_bytes(w, h, px)
        images.append((s, png))
        if s in LINUX_SIZES:
            (res / f"app_{s}.png").write_bytes(png)
    (res / "app.ico").write_bytes(ico_bytes(images))
    print(f"mkicon: {res / 'app.ico'} ({len(images)} sizes), app_{{{','.join(map(str, LINUX_SIZES))}}}.png")


if __name__ == "__main__":
    main()
