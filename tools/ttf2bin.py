#!/usr/bin/env python3
"""ttf2bin.py -- convert static TrueType fonts into jfguiLib "SGF1" outline fonts.

The assembly text engine rasterizes the quadratic outlines itself; this tool only
extracts them (unhinted, font units) into the compact .bin layout of DESIGN.md §3.2
(summary in legacy/ASMLIB/docs/FONTS.md).  stdlib + numpy only, no fontTools.

  python tools/ttf2bin.py --all
        regenerate legacy/ASMLIB/styles/winui/assets/font_{reg,semi,bold}.bin, icons.bin and
        legacy/ASMLIB/inc/icons.inc from C:\\Windows\\Fonts (segoeui, seguisb, segoeuib,
        SegoeIcons) and tools/icons.txt
  python tools/ttf2bin.py --font F.ttf --out F.bin [--charset text|all|20-7E,A0-17F,...]
        any static TrueType font (e.g. Selawik) with the text charset (default)
  python tools/ttf2bin.py --font F.ttf --out I.bin --icons tools/icons.txt [--inc I.inc]
        icon font (flags.b0) containing only the codepoints listed in the icon file
  python tools/ttf2bin.py --dump F.bin [--glyphs]
        print a summary of an SGF1 file (and every glyph record with --glyphs)

Every written file is re-read with tools/refrender.py and its outlines are compared
point-by-point against the TrueType source (self check).
"""
import argparse
import math
import os
import re
import struct
import sys

import numpy as np

TOOLS = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(TOOLS)
FONTDIR = os.path.join(os.environ.get("WINDIR", r"C:\Windows"), "Fonts")
ASSETS = os.path.join(ROOT, "legacy", "ASMLIB", "styles", "winui", "assets")
INCDIR = os.path.join(ROOT, "legacy", "ASMLIB", "inc")
ICONS_TXT = os.path.join(TOOLS, "icons.txt")

SGF_VERSION = 1
FLAG_ICON = 1
FLAG_KERN = 2
KERN_UNIT = 4                    # kern matrix entries are in units of 4 font units

# DESIGN §3.2 text charset.
TEXT_EXTRA = ("ƒ ˆ ˜ Ș ș Ț ț – — ‘ ’ ‚ “ ” „ † ‡ • … ‰ ‹ › € № ™ ← ↑ → ↓ "
              "− × ≈ ≠ ≤ ≥ ∞ ● \ufffd")


def text_charset():
    cps = set(range(0x20, 0x7F)) | set(range(0xA0, 0x180))
    cps |= {ord(c) for c in TEXT_EXTRA.split()}
    return sorted(cps)


class FontError(Exception):
    pass


# ---------------------------------------------------------------------------------------------
# TrueType parsing
# ---------------------------------------------------------------------------------------------
def _u16(b, o): return struct.unpack_from(">H", b, o)[0]
def _i16(b, o): return struct.unpack_from(">h", b, o)[0]
def _u32(b, o): return struct.unpack_from(">I", b, o)[0]


def _f2dot14(b, o):
    return _i16(b, o) / 16384.0


class Glyph:
    """One TrueType glyph.  kind: 'empty' | 'simple' | 'comp'."""
    __slots__ = ("gid", "kind", "ends", "pts", "comps", "bbox")

    def __init__(self, gid):
        self.gid = gid
        self.kind = "empty"
        self.ends = []          # simple: last point index of each contour
        self.pts = []           # simple: (x, y, on)
        self.comps = []         # comp: (gid, dx, dy, matrix(a,b,c,d) or None, flags)
        self.bbox = (0, 0, 0, 0)


class TTFont:
    def __init__(self, path, index=0):
        self.path = path
        with open(path, "rb") as f:
            d = f.read()
        self.d = d
        base = 0
        if d[:4] == b"ttcf":
            n = _u32(d, 8)
            if index >= n:
                raise FontError(f"{path}: collection has {n} fonts, index {index} requested")
            base = _u32(d, 12 + 4 * index)
        if d[base:base + 4] == b"OTTO":
            raise FontError(f"{path}: CFF-based OpenType font (OTTO) is not supported")
        if d[base:base + 4] not in (b"\x00\x01\x00\x00", b"true"):
            raise FontError(f"{path}: not a TrueType font")
        ntab = _u16(d, base + 4)
        self.tables = {}
        for i in range(ntab):
            o = base + 12 + 16 * i
            tag = d[o:o + 4].decode("latin-1")
            self.tables[tag] = (_u32(d, o + 8), _u32(d, o + 12))
        for bad in ("CFF ", "CFF2"):
            if bad in self.tables:
                raise FontError(f"{path}: has a '{bad}' table (PostScript outlines) -- not supported")
        if "fvar" in self.tables:
            raise FontError(f"{path}: variable font ('fvar') -- use a static instance")
        for req in ("head", "maxp", "hhea", "hmtx", "cmap", "loca", "glyf"):
            if req not in self.tables:
                raise FontError(f"{path}: missing required table '{req}'")
        self._parse_head()
        self._parse_hhea_hmtx()
        self._parse_loca()
        self._parse_os2_post()
        self._glyphs = {}

    def table(self, tag):
        o, n = self.tables[tag]
        return self.d[o:o + n]

    def _parse_head(self):
        h = self.table("head")
        self.upem = _u16(h, 18)
        self.head_bbox = (_i16(h, 36), _i16(h, 38), _i16(h, 40), _i16(h, 42))
        self.loca_long = _i16(h, 50) == 1
        self.num_glyphs = _u16(self.table("maxp"), 4)

    def _parse_hhea_hmtx(self):
        h = self.table("hhea")
        self.asc, self.desc, self.line_gap = _i16(h, 4), _i16(h, 6), _i16(h, 8)
        nhm = _u16(h, 34)
        m = self.table("hmtx")
        self.adv, self.lsb = [], []
        for i in range(self.num_glyphs):
            if i < nhm:
                self.adv.append(_u16(m, 4 * i))
                self.lsb.append(_i16(m, 4 * i + 2))
            else:
                self.adv.append(self.adv[nhm - 1])
                self.lsb.append(_i16(m, 4 * nhm + 2 * (i - nhm)))

    def _parse_loca(self):
        l = self.table("loca")
        n = self.num_glyphs + 1
        if self.loca_long:
            self.loca = list(struct.unpack_from(f">{n}I", l, 0))
        else:
            self.loca = [2 * v for v in struct.unpack_from(f">{n}H", l, 0)]

    def _parse_os2_post(self):
        self.x_height = self.cap_height = 0
        self.os2_version = -1
        if "OS/2" in self.tables:
            o = self.table("OS/2")
            self.os2_version = _u16(o, 0)
            if self.os2_version >= 2 and len(o) >= 90:
                self.x_height, self.cap_height = _i16(o, 86), _i16(o, 88)
        self.ul_pos = self.ul_thick = 0
        if "post" in self.tables:
            p = self.table("post")
            self.ul_pos, self.ul_thick = _i16(p, 8), _i16(p, 10)

    # -- cmap ------------------------------------------------------------------------------
    def cmap(self):
        c = self.table("cmap")
        n = _u16(c, 2)
        subs = {}
        for i in range(n):
            pid, eid, off = _u16(c, 4 + 8 * i), _u16(c, 6 + 8 * i), _u32(c, 8 + 8 * i)
            subs[(pid, eid)] = off
        for key in ((3, 10), (0, 6), (0, 4), (3, 1), (0, 3), (0, 2), (0, 1), (0, 0), (3, 0)):
            if key in subs:
                off = subs[key]
                fmt = _u16(c, off)
                if fmt == 12:
                    return self._cmap12(c, off), key, fmt
                if fmt == 4:
                    return self._cmap4(c, off), key, fmt
        raise FontError(f"{self.path}: no usable cmap subtable (need format 4 or 12)")

    @staticmethod
    def _cmap4(c, off):
        segx2 = _u16(c, off + 6)
        seg = segx2 // 2
        ends = off + 14
        starts = ends + segx2 + 2
        deltas = starts + segx2
        ranges = deltas + segx2
        m = {}
        for i in range(seg):
            end, start = _u16(c, ends + 2 * i), _u16(c, starts + 2 * i)
            delta, ro = _u16(c, deltas + 2 * i), _u16(c, ranges + 2 * i)
            for cp in range(start, end + 1):
                if cp == 0xFFFF:
                    continue
                if ro == 0:
                    g = (cp + delta) & 0xFFFF
                else:
                    g = _u16(c, ranges + 2 * i + ro + 2 * (cp - start))
                    if g:
                        g = (g + delta) & 0xFFFF
                if g:
                    m[cp] = g
        return m

    @staticmethod
    def _cmap12(c, off):
        n = _u32(c, off + 12)
        m = {}
        for i in range(n):
            s, e, g = struct.unpack_from(">III", c, off + 16 + 12 * i)
            for cp in range(s, e + 1):
                if g + cp - s:
                    m[cp] = g + cp - s
        return m

    # -- kern ------------------------------------------------------------------------------
    def kern_pairs(self):
        """{(left gid, right gid): value} from a version-0 'kern' table (format 0 subtables)."""
        pairs = {}
        if "kern" not in self.tables:
            return pairs
        k = self.table("kern")
        if _u16(k, 0) != 0:
            print(f"  note: {os.path.basename(self.path)}: kern table version != 0 (Apple) ignored")
            return pairs
        n = _u16(k, 2)
        o = 4
        for _ in range(n):
            length, cov = _u16(k, o + 2), _u16(k, o + 4)
            fmt = cov >> 8
            horiz, minimum, cross, override = cov & 1, cov & 2, cov & 4, cov & 8
            if fmt == 0 and horiz and not minimum and not cross:
                npairs = _u16(k, o + 6)
                for i in range(npairs):
                    l, r, v = struct.unpack_from(">HHh", k, o + 14 + 6 * i)
                    if override:
                        pairs[(l, r)] = v
                    else:
                        pairs[(l, r)] = pairs.get((l, r), 0) + v
                o += 14 + 6 * npairs          # 'length' is only 16 bits; recompute
            else:
                o += length
        return pairs

    # -- glyf ------------------------------------------------------------------------------
    def glyph(self, gid):
        g = self._glyphs.get(gid)
        if g is None:
            g = self._parse_glyph(gid)
            self._glyphs[gid] = g
        return g

    def _parse_glyph(self, gid):
        if not 0 <= gid < self.num_glyphs:
            raise FontError(f"glyph id {gid} out of range")
        g = Glyph(gid)
        go = self.tables["glyf"][0]
        start, end = go + self.loca[gid], go + self.loca[gid + 1]
        if end <= start:
            return g
        d = self.d
        nc = _i16(d, start)
        g.bbox = (_i16(d, start + 2), _i16(d, start + 4), _i16(d, start + 6), _i16(d, start + 8))
        p = start + 10
        if nc > 0:
            g.kind = "simple"
            g.ends = list(struct.unpack_from(f">{nc}H", d, p))
            p += 2 * nc
            p += 2 + _u16(d, p)                           # skip instructions
            npts = g.ends[-1] + 1
            flags = []
            while len(flags) < npts:
                f = d[p]; p += 1
                flags.append(f)
                if f & 8:
                    r = d[p]; p += 1
                    flags.extend([f] * r)
            flags = flags[:npts]
            xs, ys = [], []
            v = 0
            for f in flags:
                if f & 2:
                    dv = d[p]; p += 1
                    v += dv if f & 16 else -dv
                elif not f & 16:
                    v += _i16(d, p); p += 2
                xs.append(v)
            v = 0
            for f in flags:
                if f & 4:
                    dv = d[p]; p += 1
                    v += dv if f & 32 else -dv
                elif not f & 32:
                    v += _i16(d, p); p += 2
                ys.append(v)
            g.pts = [(x, y, f & 1) for x, y, f in zip(xs, ys, flags)]
        elif nc < 0:
            g.kind = "comp"
            while True:
                fl, cg = _u16(d, p), _u16(d, p + 2)
                p += 4
                if not fl & 0x0002:
                    raise FontError(f"glyph {gid}: composite component {cg} uses point matching "
                                    f"(ARGS_ARE_XY_VALUES not set) -- not supported")
                if fl & 0x0001:
                    a1, a2 = _i16(d, p), _i16(d, p + 2); p += 4
                else:
                    a1, a2 = struct.unpack_from(">bb", d, p); p += 2
                mat = None
                if fl & 0x0008:
                    s = _f2dot14(d, p); p += 2
                    mat = (s, 0.0, 0.0, s)
                elif fl & 0x0040:
                    mat = (_f2dot14(d, p), 0.0, 0.0, _f2dot14(d, p + 2)); p += 4
                elif fl & 0x0080:
                    mat = (_f2dot14(d, p), _f2dot14(d, p + 2), _f2dot14(d, p + 4), _f2dot14(d, p + 6))
                    p += 8
                if mat == (1.0, 0.0, 0.0, 1.0):
                    mat = None
                g.comps.append((cg, a1, a2, mat, fl))
                if not fl & 0x0020:
                    break
        return g

    def resolved(self, gid, depth=0):
        """Fully flattened outline of glyph gid: (ends, pts) with float coordinates."""
        if depth > 16:
            raise FontError(f"glyph {gid}: composite nesting too deep (cycle?)")
        g = self.glyph(gid)
        if g.kind == "simple":
            return list(g.ends), [(float(x), float(y), on) for x, y, on in g.pts]
        ends, pts = [], []
        if g.kind == "comp":
            for cg, dx, dy, mat, fl in g.comps:
                ce, cp = self.resolved(cg, depth + 1)
                if mat is not None:
                    a, b, c, dd = mat
                    if fl & 0x0800 and not fl & 0x1000:      # SCALED_COMPONENT_OFFSET
                        dx, dy = dx * math.hypot(a, b), dy * math.hypot(c, dd)
                    cp = [(a * x + c * y, b * x + dd * y, on) for x, y, on in cp]
                base = len(pts)
                pts += [(x + dx, y + dy, on) for x, y, on in cp]
                ends += [e + base for e in ce]
        return ends, pts


# ---------------------------------------------------------------------------------------------
# SGF1 writer
# ---------------------------------------------------------------------------------------------
HDR = struct.Struct("<4sHHHhhhhhhhHHHHIIIIH14x")
assert HDR.size == 64
RANGE = struct.Struct("<IHH")
GREC = struct.Struct("<HhhhhHI")
assert GREC.size == 16
COMP = struct.Struct("<Hhhh")


def _round(v):
    return int(math.floor(v + 0.5))


def _bbox(pts):
    if not pts:
        return (0, 0, 0, 0)
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    return (min(xs), min(ys), max(xs), max(ys))


def _kern_classes(K):
    """Group identical rows / columns of K (int matrix). Class 0 is always the all-zero class."""
    def group(M):
        keys = {bytes(M.shape[1] * M.itemsize): 0}
        reps = [np.zeros(M.shape[1], M.dtype)]
        cls = np.zeros(M.shape[0], np.int32)
        for i, row in enumerate(M):
            b = row.tobytes()
            c = keys.get(b)
            if c is None:
                c = keys[b] = len(reps)
                reps.append(row)
            cls[i] = c
        return cls, np.array(reps)
    lcls, rows = group(K)                 # rows: L x n
    rcls, cols = group(np.ascontiguousarray(rows.T))   # cols: R x L
    mat = np.ascontiguousarray(cols.T)    # L x R
    assert (mat[lcls][:, rcls] == K).all()
    return lcls, rcls, mat


def build_sgf(font, cps, icon=False, fallback_cp=None, verbose=True):
    """Build an SGF1 image for the given codepoints.  Returns (bytes, stats)."""
    cmap, cmap_key, cmap_fmt = font.cmap()
    mapped = [(cp, cmap[cp]) for cp in sorted(set(cps)) if cmap.get(cp)]
    missing = [cp for cp in sorted(set(cps)) if not cmap.get(cp)]
    if not mapped:
        raise FontError("none of the requested codepoints is in the font")
    nmapped = len(mapped)

    # local glyph ids: mapped codepoints in codepoint order, then unmapped components
    local_ttf = [g for _, g in mapped]
    first_local = {}
    for i, g in enumerate(local_ttf):
        first_local.setdefault(g, i)
    todo, seen, info = list(dict.fromkeys(local_ttf)), set(), {}
    k = 0
    nflat = 0
    while k < len(todo):
        tg = todo[k]; k += 1
        if tg in seen:
            continue
        seen.add(tg)
        g = font.glyph(tg)
        if g.kind == "comp" and any(m is not None for _, _, _, m, _ in g.comps):
            ends, pts = font.resolved(tg)                     # flatten transformed components
            pts = [(_round(x), _round(y), on) for x, y, on in pts]
            info[tg] = ("simple", ends, pts)
            nflat += 1
        elif g.kind == "comp":
            for cg, _, _, _, _ in g.comps:
                if cg not in first_local:
                    first_local[cg] = len(local_ttf)
                    local_ttf.append(cg)
                    todo.append(cg)
            info[tg] = ("comp", g.comps)
        elif g.kind == "simple":
            info[tg] = ("simple", g.ends, g.pts)
        else:
            info[tg] = ("empty",)
    nglyphs = len(local_ttf)
    if nglyphs > 0xFFFF:
        raise FontError("too many glyphs")

    # advances (USE_MY_METRICS honoured) and control boxes (from the resolved points)
    def advance(tg):
        g = font.glyph(tg)
        if g.kind == "comp":
            for cg, _, _, _, fl in g.comps:
                if fl & 0x0200:
                    return advance(cg)
        return font.adv[tg]

    bbox_cache = {}

    def bbox(tg):
        if tg not in bbox_cache:
            ends, pts = font.resolved(tg)
            if info.get(tg, ("",))[0] == "simple":
                pts = info[tg][2]
            bbox_cache[tg] = tuple(_round(v) for v in _bbox(pts))
        return bbox_cache[tg]

    # outlines (one copy per TrueType glyph)
    out = bytearray()
    offs = {}
    stats = dict(points=0, contours=0, composites=0, simple=0, empty=0, flattened=nflat)
    for tg in dict.fromkeys(local_ttf):
        kind = info[tg][0]
        if kind == "empty":
            offs[tg] = (0, 0)
            stats["empty"] += 1
            continue
        offs[tg] = (len(out), None)
        if kind == "simple":
            _, ends, pts = info[tg]
            n = len(ends)
            if n >= 0x8000:
                raise FontError(f"glyph {tg}: too many contours")
            out += struct.pack(f"<{n}H", *ends)
            if n & 1:
                out += b"\0\0"
            for x, y, on in pts:
                if not (-16384 <= x <= 16383 and -32768 <= y <= 32767):
                    raise FontError(f"glyph {tg}: coordinate ({x},{y}) out of range")
                out += struct.pack("<hh", (x << 1) | (on & 1), y)
            offs[tg] = (offs[tg][0], n)
            stats["points"] += len(pts)
            stats["contours"] += n
            stats["simple"] += 1
        else:
            comps = info[tg][1]
            for cg, dx, dy, _, _ in comps:
                out += COMP.pack(first_local[cg], dx, dy, 0)
            offs[tg] = (offs[tg][0], 0x8000 | len(comps))
            stats["composites"] += 1

    recs = bytearray()
    lsb_mismatch = 0
    for tg in local_ttf:
        off, n = offs[tg]
        if n == 0:
            off = 0
        x0, y0, x1, y1 = bbox(tg)
        if info[tg][0] == "simple" and font.glyph(tg).kind == "simple" and font.lsb[tg] != x0:
            lsb_mismatch += 1
        recs += GREC.pack(advance(tg), x0, y0, x1, y1, n or 0, off)

    # ranges: runs of consecutive codepoints (glyph ids are consecutive by construction)
    ranges = []
    for i, (cp, _) in enumerate(mapped):
        if ranges and ranges[-1][0] + ranges[-1][1] == cp and ranges[-1][1] < 0xFFFF:
            ranges[-1][1] += 1
        else:
            ranges.append([cp, 1, i])

    # kerning
    kern = b""
    kl = kr = 0
    flags = FLAG_ICON if icon else 0
    kstats = dict(pairs=0, pairs_in_set=0, lclasses=0, rclasses=0, qerr=0)
    pairs = {} if icon else font.kern_pairs()
    kstats["pairs"] = len(pairs)
    if pairs:
        rows = {}
        for i in range(nmapped):
            rows.setdefault(local_ttf[i], []).append(i)
        K = np.zeros((nmapped, nmapped), np.int8)
        clip = 0
        for (l, r), v in pairs.items():
            if l in rows and r in rows and v:
                q = _round(v / KERN_UNIT)
                if not -128 <= q <= 127:
                    clip += 1
                    q = max(-128, min(127, q))
                kstats["pairs_in_set"] += 1
                kstats["qerr"] = max(kstats["qerr"], abs(q * KERN_UNIT - v))
                for i in rows[l]:
                    K[i, rows[r]] = q
        if clip:
            print(f"  warning: {clip} kern values clipped to i8")
        if K.any():
            lcls, rcls, mat = _kern_classes(K)
            kl, kr = mat.shape
            if kl > 256 or kr > 256:
                print(f"  warning: {kl}x{kr} kern classes exceed u8 -- kerning dropped")
            else:
                lc = np.zeros(nglyphs, np.uint8); lc[:nmapped] = lcls
                rc = np.zeros(nglyphs, np.uint8); rc[:nmapped] = rcls
                kern = lc.tobytes() + rc.tobytes() + mat.astype(np.int8).tobytes()
                flags |= FLAG_KERN
                kstats["lclasses"], kstats["rclasses"] = kl, kr
        if not kern:
            kl = kr = 0

    # fallback glyph
    fb = 0
    if fallback_cp is None:
        fallback_cp = 0xE9CE if icon else 0x3F
    for i, (cp, _) in enumerate(mapped):
        if cp == fallback_cp:
            fb = i
            break
    else:
        print(f"  warning: fallback U+{fallback_cp:04X} not in set, fallback glyph = 0")

    xh, ch = font.x_height, font.cap_height
    if xh <= 0 and 0x78 in cmap:
        xh = font.glyph(cmap[0x78]).bbox[3]
    if ch <= 0 and 0x48 in cmap:
        ch = font.glyph(cmap[0x48]).bbox[3]

    off_ranges = HDR.size
    off_glyphs = off_ranges + RANGE.size * len(ranges)
    off_outl = off_glyphs + len(recs)
    off_kern = off_outl + len(out) if kern else 0
    hdr = HDR.pack(b"SGF1", SGF_VERSION, flags, font.upem, font.asc, font.desc, font.line_gap,
                   ch, xh, font.ul_pos, font.ul_thick, nglyphs, len(ranges), kl, kr,
                   off_ranges, off_glyphs, off_outl, off_kern, fb)
    img = hdr + b"".join(RANGE.pack(*r) for r in ranges) + recs + out + kern
    stats.update(dict(cps=len(set(cps)), mapped=nmapped, missing=missing, glyphs=nglyphs,
                      components=nglyphs - nmapped, ranges=len(ranges), outline_bytes=len(out),
                      kern_bytes=len(kern), size=len(img), cmap=f"{cmap_key} fmt {cmap_fmt}",
                      lsb_mismatch=lsb_mismatch, fallback=fb, **{"kern_" + k: v for k, v in kstats.items()}))
    return bytes(img), stats


def verify(font, img, cps):
    """Re-read img with refrender and compare every mapped glyph's resolved outline and metrics."""
    sys.path.insert(0, TOOLS)
    import refrender
    f = refrender.SGFFont(img)
    cmap = font.cmap()[0]
    bad = 0
    for cp in sorted(set(cps)):
        tg = cmap.get(cp)
        gid = f.lookup(cp)
        if not tg:
            if gid is not None:
                bad += 1
                print(f"  verify: U+{cp:04X} unexpectedly present")
            continue
        if gid is None:
            bad += 1
            print(f"  verify: U+{cp:04X} missing")
            continue
        ends, pts = font.resolved(tg)
        want = [[(_round(x), _round(y), on) for x, y, on in pts[s:e + 1]]
                for s, e in zip([0] + [e + 1 for e in ends[:-1]], ends)]
        got = f.contours(gid)
        if got != want:
            bad += 1
            print(f"  verify: U+{cp:04X} outline mismatch")
        if f.advance(gid) != font.adv[tg] and font.glyph(tg).kind != "comp":
            bad += 1
            print(f"  verify: U+{cp:04X} advance mismatch")
    if bad:
        raise FontError(f"self check failed ({bad} problems)")
    return f


# ---------------------------------------------------------------------------------------------
# icons.txt / icons.inc
# ---------------------------------------------------------------------------------------------
def read_icons(path):
    icons = []
    names = set()
    for ln, line in enumerate(open(path, encoding="utf-8"), 1):
        s = line.split("#", 1)[0].strip()
        if not s:
            continue
        parts = s.split()
        if len(parts) != 2:
            raise FontError(f"{path}:{ln}: expected 'Name codepoint'")
        name, cps = parts
        cps = re.sub(r"^(0x|U\+)", "", cps, flags=re.I)
        cp = int(cps, 16)
        if name in names:
            raise FontError(f"{path}:{ln}: duplicate name {name}")
        names.add(name)
        icons.append((name, cp))
    return icons


def const_name(name):
    if re.fullmatch(r"[A-Z0-9_]+", name):
        return "IC_" + name
    s = re.sub(r"(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])", "_", name)
    return "IC_" + s.upper()


def write_icons_inc(path, icons, font_path):
    lines = ["# icons.inc -- GENERATED by tools/ttf2bin.py from tools/icons.txt; do not edit.",
             f"# Icon codepoints of legacy/ASMLIB/styles/winui/assets/icons.bin (from {os.path.basename(font_path)},",
             "# SGF1 flags.b0 = icon font).  Draw with the icon font like text: one codepoint = one icon.",
             ""]
    for name, cp in icons:
        lines.append(f"{const_name(name):<28}= 0x{cp:04X}")
    data = ("\n".join(lines) + "\n").encode("ascii")
    with open(path, "wb") as f:
        f.write(data)


# ---------------------------------------------------------------------------------------------
# driver
# ---------------------------------------------------------------------------------------------
def parse_charset(spec, font=None):
    if spec in (None, "text"):
        return text_charset()
    if spec == "all":
        return sorted(font.cmap()[0])
    if spec.startswith("@"):
        spec = open(spec[1:], encoding="utf-8").read()
    cps = set()
    for part in re.split(r"[\s,]+", spec.strip()):
        if not part:
            continue
        part = re.sub(r"(?i)(0x|U\+)", "", part)
        if "-" in part:
            a, b = part.split("-")
            cps |= set(range(int(a, 16), int(b, 16) + 1))
        else:
            cps.add(int(part, 16))
    return sorted(cps)


def report(name, st):
    print(f"{name}: {st['size']} bytes  {st['glyphs']} glyphs ({st['mapped']} codepoints + "
          f"{st['components']} components) {st['ranges']} ranges, {st['points']} points, "
          f"{st['contours']} contours, {st['composites']} composites ({st['flattened']} flattened), "
          f"outlines {st['outline_bytes']} B, kern {st['kern_bytes']} B "
          f"[{st['kern_lclasses']}x{st['kern_rclasses']} classes, {st['kern_pairs_in_set']}/"
          f"{st['kern_pairs']} pairs, max quant err {st['kern_qerr']} fu]  cmap {st['cmap']}")
    if st["missing"]:
        print("  missing codepoints (-> fallback): " + " ".join(f"U+{c:04X}" for c in st["missing"]))
    if st["lsb_mismatch"]:
        print(f"  note: {st['lsb_mismatch']} glyphs with hmtx lsb != xMin (origin kept at x=0)")


def convert(font_path, out_path, cps, icon=False, fallback_cp=None):
    font = TTFont(font_path)
    img, st = build_sgf(font, cps, icon=icon, fallback_cp=fallback_cp)
    verify(font, img, cps)
    os.makedirs(os.path.dirname(os.path.abspath(out_path)), exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(img)
    report(os.path.relpath(out_path, ROOT) if out_path.startswith(ROOT) else out_path, st)
    return st


def dump(path, glyphs=False):
    sys.path.insert(0, TOOLS)
    import refrender
    f = refrender.SGFFont(path)
    h = f.hdr
    print(f"{path}: {len(f.data)} bytes, SGF1 v{h['version']} flags 0x{h['flags']:X}"
          f" ({'icon' if h['flags'] & 1 else 'text'}{', kern' if h['flags'] & 2 else ''})")
    print(f"  upem {h['upem']} asc {h['asc']} desc {h['desc']} lineGap {h['lineGap']} "
          f"capH {h['capH']} xH {h['xH']} ulPos {h['ulPos']} ulThick {h['ulThick']}")
    print(f"  glyphs {h['nGlyphs']} ranges {h['nRanges']} kern {h['kernL']}x{h['kernR']} "
          f"fallback {h['fallback']}")
    print(f"  offsets ranges {h['offRanges']} glyphs {h['offGlyphs']} outlines {h['offOutlines']} "
          f"kern {h['offKern']}")
    ncp = sum(c for _, c, _ in f.ranges)
    print(f"  {ncp} codepoints: " + " ".join(
        f"{a:04X}" + (f"-{a + c - 1:04X}" if c > 1 else "") for a, c, _ in f.ranges))
    if glyphs:
        rev = {}
        for a, c, g in f.ranges:
            for i in range(c):
                rev.setdefault(g + i, a + i)
        for gid in range(h["nGlyphs"]):
            adv, x0, y0, x1, y1, n, off = f.recs[gid]
            cp = rev.get(gid)
            kind = "comp" if n & 0x8000 else "simple"
            extra = ""
            if n & 0x8000:
                extra = " " + " ".join(f"[{g} {dx:+d},{dy:+d}]" for g, dx, dy in f.components(gid))
            print(f"  {gid:4d} {'U+%04X' % cp if cp is not None else '  comp':>7} adv {adv:5d} "
                  f"box {x0:5d} {y0:5d} {x1:5d} {y1:5d}  {kind} n={n & 0x7FFF} off {off}{extra}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--all", action="store_true", help="regenerate all library font/icon assets")
    ap.add_argument("--font", help="input TrueType font")
    ap.add_argument("--out", help="output .bin")
    ap.add_argument("--charset", help="text (default) | all | ranges like 20-7E,A0-17F,20AC | @file")
    ap.add_argument("--icons", help="icon list (Name codepoint per line) -> icon font")
    ap.add_argument("--inc", help="with --icons: also write the IC_* include file")
    ap.add_argument("--fallback", help="fallback codepoint (hex); default '?' or E9CE for icons")
    ap.add_argument("--dump", help="print a summary of an SGF1 file")
    ap.add_argument("--glyphs", action="store_true", help="with --dump: list every glyph record")
    a = ap.parse_args()
    try:
        if a.dump:
            dump(a.dump, a.glyphs)
            return 0
        fb = int(a.fallback, 16) if a.fallback else None
        if a.all:
            os.makedirs(ASSETS, exist_ok=True)
            cs = text_charset()
            print(f"text charset: {len(cs)} codepoints")
            for src, dst in (("segoeui.ttf", "font_reg.bin"), ("seguisb.ttf", "font_semi.bin"),
                             ("segoeuib.ttf", "font_bold.bin")):
                convert(os.path.join(FONTDIR, src), os.path.join(ASSETS, dst), cs, fallback_cp=fb)
            icons = read_icons(ICONS_TXT)
            ipath = os.path.join(FONTDIR, "SegoeIcons.ttf")
            st = convert(ipath, os.path.join(ASSETS, "icons.bin"), [c for _, c in icons], icon=True)
            if st["missing"]:
                raise FontError("icons.txt lists codepoints missing from the icon font")
            write_icons_inc(os.path.join(INCDIR, "icons.inc"), icons, ipath)
            print(f"legacy/ASMLIB/inc/icons.inc: {len(icons)} icons")
            return 0
        if not a.font or not a.out:
            ap.error("need --all, --dump FILE or --font F.ttf --out F.bin")
        if a.icons:
            icons = read_icons(a.icons)
            st = convert(a.font, a.out, [c for _, c in icons], icon=True, fallback_cp=fb)
            if a.inc:
                write_icons_inc(a.inc, icons, a.font)
        else:
            convert(a.font, a.out, parse_charset(a.charset, TTFont(a.font)), fallback_cp=fb)
        return 0
    except FontError as e:
        print("error:", e, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
