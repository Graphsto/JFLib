// glyphcache -- rasterized glyph masks, cached by (font, glyph, size, x phase)
//                                                                  [src/text/glyphcache.s]
//
//   gc_get(font, glyph, size_fx, phase) -> Entry / None (nothing to draw) / Direct (too
//        large to cache: draw it with glyph_path + fill).  size_fx: pixel size (em height)
//        in 24.8; phase: 0..3 quarter pixels.  Entry: left, top (relative to pen x /
//        baseline y), w, h, w*h coverage bytes.  An empty glyph is an entry of 0 x 0.
//   gc_get_bitmap(font, glyph, size_fx) -> colour bitmap entry (CBDT fonts): w*h PARGB
//        pixels; bitmaps larger than GC_BIGBMP bytes are not cached (valid until the next one)
//   gc_box(font, glyph, size_fx, phase) -> {left, top, w, h} as gc_get computes it
//   gc_flush()   gc_scale(font, size_fx) -> (sx, sy) pixels per font unit (sy snapped)
//
// Hinting-free: for sizes up to 24 px the vertical scale is adjusted so the x-height lands
// on a whole pixel; horizontally only quarter-pixel phases.  Key: 1<<63 | colour<<62 |
// font<<40 (22 bits) | phase<<36 | size<<16 (20 bits) | glyph (16 bits).  Like the
// assembly the cache starts over when it holds GC_SLOTS / 2 entries or its 2 MB arena is
// full.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use gfx::{Dc, Surface};
use rt::fx::{ceil_i, cvt, floor_i};

use crate::font::{Gp, SGH_CBDT};
use crate::registry::Face;
use crate::Text;

const GC_SLOTS: u32 = 4096;
const GC_ARENA: usize = 2 * 1024 * 1024;
const GC_MAXSIDE: i32 = 1024;
const GC_MAXPIX: i32 = 256 * 1024; // larger masks are drawn directly
const GC_SIZE_LIMIT: i32 = 1 << 20; // size_fx must fit the 20-bit key field
const GC_FONT_MASK: u32 = 0x3F_FFFF;
const GC_BIGBMP: usize = GC_ARENA / 4; // colour bitmaps above this are not cached

/// a cached glyph: placement relative to the pen x / baseline, w x h coverage bytes (masks)
/// or PARGB pixels (bitmaps)
#[derive(Clone, Default)]
pub struct GcEntry {
    pub left: i32,
    pub top: i32,
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
}

pub enum GcRef<'a> {
    None,
    Direct,
    Entry(&'a GcEntry),
}

#[derive(Default)]
pub(crate) struct Gc {
    map: BTreeMap<u64, GcEntry>,
    bytes: usize,
    big: Option<GcEntry>,
}

impl Gc {
    // the arena block of an entry (16-byte granules)
    fn charge(&mut self, n: usize) -> bool {
        let n = (n + 15) & !15;
        if self.bytes + n > GC_ARENA {
            return false;
        }
        self.bytes += n;
        true
    }
}

// gc_key(font, glyph, size_fx, phase) (bit 63 set)
fn gc_key(font: u32, glyph: u32, size: i32, phase: i32) -> u64 {
    1u64 << 63
        | ((font & GC_FONT_MASK) as u64) << 40
        | ((phase & 3) as u64) << 36
        | ((size as u32 & 0xF_FFFF) as u64) << 16
        | (glyph & 0xFFFF) as u64
}

impl Text {
    /// gc_flush(): forget every cached glyph
    pub fn gc_flush(&mut self) {
        self.gc.map.clear();
        self.gc.bytes = 0;
        self.gc.big = None;
    }

    /// gc_scale(font, size_fx) -> (sx, sy): pixels per font unit, sy snapped so the
    /// x-height is whole up to 24 px
    pub fn gc_scale(&self, font: u32, size_fx: i32) -> (f32, f32) {
        let h = self.hdr(font);
        let sx = (size_fx as f32 * (1.0 / 256.0)) / h.upem as f32;
        let mut sy = sx;
        if self.font_raster(font) == crate::FontRaster::Legacy && size_fx <= 24 * 256 && h.xh > 0 {
            let xh = h.xh as f32;
            let t = cvt(xh * sx);
            if t > 0 {
                sy = t as f32 / xh;
            }
        }
        (sx, sy)
    }

    // the mask placement of gc_get {left, top, w, h, sx, sy}, or None when empty  [gc_place]
    fn gc_place(&mut self, font: u32, g: u32, size: i32, phase: i32) -> Option<(i32, i32, i32, i32, f32, f32)> {
        let (sx, sy) = self.gc_scale(font, size);
        let b = self.glyph_box(font, g)?;
        let ph = (phase & 3) as f32 * 0.25;
        let left = floor_i(b[0] as f32 * sx + ph);
        let w = ceil_i(b[2] as f32 * sx + ph).wrapping_sub(left);
        let top = floor_i(b[3].wrapping_neg() as f32 * sy);
        let h = ceil_i(b[1].wrapping_neg() as f32 * sy).wrapping_sub(top);
        if w <= 0 || h <= 0 {
            return None;
        }
        Some((left, top, w, h, sx, sy))
    }

    /// gc_box(font, glyph, size_fx, phase) -> {left, top, w, h} (not cached)
    pub fn gc_box(&mut self, font: u32, g: u32, size: i32, phase: i32) -> Option<[i32; 4]> {
        self.gc_place(font, g, size, phase).map(|p| [p.0, p.1, p.2, p.3])
    }

    /// gc_get(font, glyph, size_fx, phase): the cached mask, rendered on first use
    pub fn gc_get(&mut self, font: u32, g: u32, size: i32, phase: i32) -> GcRef<'_> {
        let phase = phase & 3;
        if size >= GC_SIZE_LIMIT || size < 0 {
            return GcRef::Direct;
        }
        let key = gc_key(font, g, size, phase);
        if self.gc.map.contains_key(&key) {
            return GcRef::Entry(&self.gc.map[&key]);
        }
        // table more than half full: start over
        if self.gc.map.len() as u32 >= GC_SLOTS / 2 {
            self.gc_flush();
        }
        // ---- metrics
        let Some((left, top, w, h, sx, sy)) = self.gc_place(font, g, size, phase) else {
            self.gc.map.insert(key, GcEntry::default());
            return GcRef::Entry(&self.gc.map[&key]);
        };
        if w > GC_MAXSIDE || h > GC_MAXSIDE || w * h > GC_MAXPIX {
            return GcRef::Direct;
        }
        // ---- allocate the entry (retry once after a flush)
        if !self.gc.charge((w * h + 8) as usize) {
            self.gc_flush();
            if !self.gc.charge((w * h + 8) as usize) {
                return GcRef::None;
            }
        }
        // rasterize into a fresh A8 surface: origin at (-left + phase, -top), y up
        let mut sf = Surface::new_a8_packed(w, h);
        let dc = Dc::new(w, h);
        self.gp = Gp { x: phase as f32 * 0.25 + left.wrapping_neg() as f32, y: top.wrapping_neg() as f32, sx, sy, dx: 0, dy: 0 };
        self.path.begin(&dc);
        self.glyph_path(font, g);
        self.path.fill(&mut self.ras, &dc, &mut sf, 0);
        self.gc.map.insert(key, GcEntry { left, top, w, h, px: sf.px });
        GcRef::Entry(&self.gc.map[&key])
    }

    /// gc_get_bitmap(font, glyph, size_fx): the PARGB colour bitmap entry or None
    pub fn gc_get_bitmap(&mut self, font: u32, g: u32, size: i32) -> Option<&GcEntry> {
        if !(0..GC_SIZE_LIMIT).contains(&size) {
            return None;
        }
        match self.face(font)? {
            Face::Tt(_) | Face::Syn(_) => {}
            _ => return None,
        }
        if self.hdr(font).flags & SGH_CBDT == 0 {
            return None;
        }
        let key = gc_key(font, g, size, 0) | 1 << 62;
        if !self.gc.map.contains_key(&key) {
            if self.gc.map.len() as u32 >= GC_SLOTS / 2 {
                self.gc_flush();
            }
            match self.op_bitmap(font, g, size) {
                None => {
                    self.gc.map.insert(key, GcEntry::default());
                }
                Some(b) => {
                    let n = (b.w * b.h * 4 + 8) as usize;
                    let e = GcEntry { left: b.left, top: b.top, w: b.w, h: b.h, px: b.px };
                    if n > GC_BIGBMP {
                        // too big for the arena: a separate block, not entered in the table
                        self.gc.big = Some(e);
                        return self.gc.big.as_ref();
                    }
                    if !self.gc.charge(n) {
                        self.gc_flush();
                        if !self.gc.charge(n) {
                            return None;
                        }
                    }
                    self.gc.map.insert(key, e);
                }
            }
        }
        let e = &self.gc.map[&key];
        if e.w == 0 { None } else { Some(e) }
    }
}
