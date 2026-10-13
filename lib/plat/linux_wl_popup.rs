// linux_wl_popup -- OS popup windows on Wayland (U3)                [src/plat/linux/wl_popup.s]
//
//   Wl::popup(slot, owner, rect, frame)
//        an xdg_popup of the owner's xdg_surface: positioner with the anchor point at owner
//        pixel rect.x, rect.y (logical = px / integer scale of the owner), gravity
//        bottom-right, slide on both axes (the compositor keeps it on screen); no grab (the
//        UI dismisses its popups itself).  A changed position or size creates the popup anew
//        (xdg_popup.reposition needs version 3); the content goes into two wl_shm buffers of
//        one pool per slot (the one the compositor does not hold).  rect None destroys the
//        popup's objects (WSLg: only a destroyed wl_surface removes its RAIL window).  The
//        frame's pixels are kept (the assembly keeps the caller's surface record).
//   Wl::popup_ptr(surface) -> owner index (-1 = not a popup): pointer enter;
//        inp.off_x / off_y = the popup's offset in the owner (physical px), added to every
//        pointer position until the pointer enters another surface
//   wlh_pop_xdg_configure / wlh_pop_done / wlh_pop_release  handlers

use alloc::vec::Vec;

use rt::os::{syscall6, SYS_MUNMAP};

use super::proto::*;
use super::wire::{Args, A};
use super::Wl;
use crate::{Frame, PPW_MAX};

const WQ_MAX: usize = PPW_MAX as usize;
const SHM_XRGB: u32 = 1;
const POS_ANCHOR_TOP_LEFT: u32 = 5;
const POS_GRAVITY_BOTTOM_RIGHT: u32 = 8;
const POS_SLIDE_XY: u32 = 3;

/// a popup slot
#[derive(Default)]
pub(crate) struct Wq {
    surf: u32, // wl_surface (0 = no popup objects)
    xsurf: u32,
    popup: u32,
    owner: i32,    // owner platform window (-1 = hidden)
    rect: [i32; 4], // popup pixel 0, 0 = owner pixel dx, dy; size (px)
    scale: i32,    // integer buffer scale (the owner's)
    conf: bool,    // its first configure was acknowledged
    pool: u32,     // wl_shm_pool (0 = none)
    cap: u32,      // pool bytes (two buffers)
    map: usize,
    buf: [u32; 2],
    busy: [bool; 2], // held by the compositor
    bw: i32,         // buffer size (px)
    bh: i32,
    sf: Option<(Vec<u8>, i32, i32)>, // the frame to show (rows of w * 4 bytes), w, h
}

/// a copy of the frame
fn keep(f: &Frame) -> (Vec<u8>, i32, i32) {
    let (w, h) = (f.w.max(0) as usize, f.h.max(0) as usize);
    let mut v = alloc::vec![0u8; w * h * 4];
    for y in 0..h {
        let s = y * f.stride as usize;
        if let Some(src) = f.px.get(s..s + w * 4) {
            v[y * w * 4..(y + 1) * w * 4].copy_from_slice(src);
        }
    }
    (v, w as i32, h as i32)
}

impl Wl {
    /// show / move / present popup slot `slot` over window `owner`  [wl_plat_popup]
    pub(crate) fn popup(&mut self, slot: u32, owner: i32, rect: Option<[i32; 4]>, f: Option<&Frame>) {
        if slot as usize >= WQ_MAX {
            return;
        }
        let k = slot as usize;
        let Some(rect) = rect else {
            self.wq[k].owner = -1;
            self.wq[k].sf = None;
            self.wq_objects_destroy(k);
            self.w.flush();
            return;
        };
        let Some(o) = self.rec(owner) else { return };
        if self.win[o].xsurf == 0 {
            return; // the owner is not shown
        }
        // integer scale of the owner
        let mut s = ((self.win[o].s120 as u32).wrapping_add(60) / 120) as i32;
        if s == 0 {
            s = 1;
        }
        let q = &self.wq[k];
        if q.surf != 0 && q.owner == owner && q.scale == s && q.rect == rect {
            self.wq[k].sf = f.map(keep);
            if self.wq[k].conf {
                self.wq_draw(k);
            }
        } else {
            // another place / size / owner / scale: a new popup
            if q.surf != 0 {
                self.wq_objects_destroy(k);
            }
            let q = &mut self.wq[k];
            q.owner = owner;
            q.scale = s;
            q.rect = rect;
            q.sf = f.map(keep);
            self.wq_objects_create(k, o);
        }
        self.w.flush();
    }

    /// surface, xdg_surface, positioner, xdg_popup; the initial commit (the content follows
    /// the configure)  [wq_objects_create]
    fn wq_objects_create(&mut self, k: usize, owner: usize) {
        let ku = k as u64;
        self.wq[k].conf = false;
        let surf = self.w.send(self.g.comp, &COMPOSITOR_CREATE_SURFACE, &[A::N(&IFACE_POP_SURFACE)]);
        self.wq[k].surf = surf;
        self.w.set_udata(surf, ku);
        let xs = self.w.send(self.g.xdg_wm, &WM_BASE_GET_XDG_SURFACE, &[A::N(&IFACE_POP_XSURF), A::O(surf)]);
        self.wq[k].xsurf = xs;
        self.w.set_udata(xs, ku);
        // positioner: size and anchor point in logical coordinates of the owner
        let pos = self.w.send(self.g.xdg_wm, &WM_BASE_CREATE_POSITIONER, &[A::N(&IFACE_XDG_POSITIONER)]);
        let s = self.wq[k].scale;
        let r = self.wq[k].rect;
        let lg = |v: i32| v.wrapping_div(s);
        self.w.send(pos, &POSITIONER_SET_SIZE, &[A::I(lg(r[2])), A::I(lg(r[3]))]);
        self.w.send(pos, &POSITIONER_SET_ANCHOR_RECT, &[A::I(lg(r[0])), A::I(lg(r[1])), A::I(1), A::I(1)]);
        self.w.send(pos, &POSITIONER_SET_ANCHOR, &[A::U(POS_ANCHOR_TOP_LEFT)]);
        self.w.send(pos, &POSITIONER_SET_GRAVITY, &[A::U(POS_GRAVITY_BOTTOM_RIGHT)]);
        self.w.send(pos, &POSITIONER_SET_CONSTRAINT_ADJUSTMENT, &[A::U(POS_SLIDE_XY)]);
        let parent = self.win[owner].xsurf;
        let p = self.w.send(xs, &XDG_SURFACE_GET_POPUP, &[A::N(&IFACE_XDG_POPUP), A::O(parent), A::O(pos)]);
        self.wq[k].popup = p;
        self.w.set_udata(p, ku);
        self.w.send(pos, &POSITIONER_DESTROY, &[]);
        if s as u32 > 1 {
            self.w.send(surf, &SURFACE_SET_BUFFER_SCALE, &[A::I(s)]);
        }
        self.w.send(surf, &SURFACE_COMMIT, &[]);
    }

    /// popup, xdg_surface, surface (the buffers stay for reuse)  [wq_objects_destroy]
    fn wq_objects_destroy(&mut self, k: usize) {
        let q = &self.wq[k];
        let (p, x, s) = (q.popup, q.xsurf, q.surf);
        if p != 0 {
            self.w.send(p, &POPUP_DESTROY, &[]);
        }
        if x != 0 {
            self.w.send(x, &XDG_SURFACE_DESTROY, &[]);
        }
        if s != 0 {
            self.w.send(s, &SURFACE_DESTROY, &[]);
        }
        let q = &mut self.wq[k];
        q.popup = 0;
        q.xsurf = 0;
        q.surf = 0;
        q.conf = false;
    }

    /// the frame into a free buffer: attach, damage, commit  [wq_draw]
    fn wq_draw(&mut self, k: usize) {
        let q = &self.wq[k];
        let Some((_, sw, sh)) = q.sf.as_ref() else { return };
        if q.surf == 0 {
            return;
        }
        let w = q.rect[2].min(*sw);
        let h = q.rect[3].min(*sh);
        if w <= 0 || h <= 0 {
            return;
        }
        // buffers of this size in a pool large enough for two
        if (q.bw != w || q.bh != h || q.buf[0] == 0) && !self.wq_buffers(k, w, h) {
            return;
        }
        let q = &mut self.wq[k];
        let b = if !q.busy[0] {
            0
        } else if !q.busy[1] {
            1
        } else {
            return; // both held: the next update draws
        };
        let Some((px, sw, _)) = q.sf.as_ref() else { return };
        let row = w as usize * 4;
        let base = (w as u32).wrapping_mul(h as u32).wrapping_mul(4).wrapping_mul(b as u32) as usize;
        // SAFETY: the pool holds two w x h buffers from q.map (wq_buffers sized it).
        let dst = unsafe { core::slice::from_raw_parts_mut((q.map + base) as *mut u8, row * h as usize) };
        for y in 0..h as usize {
            let s = y * *sw as usize * 4;
            if let Some(src) = px.get(s..s + row) {
                dst[y * row..(y + 1) * row].copy_from_slice(src);
            }
        }
        q.busy[b] = true;
        let (surf, buf) = (q.surf, q.buf[b]);
        self.w.send(surf, &SURFACE_ATTACH, &[A::O(buf), A::I(0), A::I(0)]);
        self.w.send(surf, &SURFACE_DAMAGE_BUFFER, &[A::I(0), A::I(0), A::I(w), A::I(h)]);
        self.w.send(surf, &SURFACE_COMMIT, &[]);
    }

    /// two w x h buffers (a new pool when it is small) -> ok  [wq_buffers]
    fn wq_buffers(&mut self, k: usize, w: i32, h: i32) -> bool {
        // the old buffers go (a held one is released by the compositor later: harmless)
        for i in 0..2 {
            let b = self.wq[k].buf[i];
            if b != 0 {
                self.w.send(b, &BUFFER_DESTROY, &[]);
                self.wq[k].buf[i] = 0;
                self.wq[k].busy[i] = false;
            }
        }
        self.wq[k].bw = 0;
        self.wq[k].bh = 0;
        let bytes = (w as u32).wrapping_mul(h as u32).wrapping_mul(8); // two buffers
        if bytes > self.wq[k].cap {
            // a larger pool (the old one is destroyed: its buffers are gone)
            let (pool, map, cap) = (self.wq[k].pool, self.wq[k].map, self.wq[k].cap);
            if pool != 0 {
                self.w.send(pool, &SHM_POOL_DESTROY, &[]);
                self.wq[k].pool = 0;
                // SAFETY: the old pool's mapping.
                unsafe { syscall6(SYS_MUNMAP, map, cap as usize, 0, 0, 0, 0) };
            }
            let cap = bytes.wrapping_add(65535) & !65535;
            self.wq[k].cap = cap;
            match self.shm_pool_new(cap as usize) {
                Some((p, m)) => {
                    self.wq[k].pool = p;
                    self.wq[k].map = m;
                }
                None => {
                    self.wq[k].cap = 0;
                    return false;
                }
            }
        }
        let pool = self.wq[k].pool;
        for i in 0..2u32 {
            let off = (w as u32).wrapping_mul(h as u32).wrapping_mul(4).wrapping_mul(i);
            let b = self.w.send(pool, &SHM_POOL_CREATE_BUFFER,
                                &[A::N(&IFACE_POP_BUFFER), A::I(off as i32), A::I(w), A::I(h), A::I(w * 4),
                                  A::U(SHM_XRGB)]);
            self.wq[k].buf[i as usize] = b;
            self.w.set_udata(b, (k * 2) as u64 + i as u64); // udata: slot * 2 + buffer
        }
        self.wq[k].bw = w;
        self.wq[k].bh = h;
        true
    }

    /// the owner of the popup with this surface (-1 = not a popup); its offset in
    /// inp.off_x / off_y  [wl_popup_ptr]
    pub(crate) fn popup_ptr(&mut self, surface: u32) -> i32 {
        if surface == 0 {
            return -1;
        }
        match self.wq.iter().position(|q| q.surf == surface) {
            Some(k) => {
                self.inp.off_x = self.wq[k].rect[0];
                self.inp.off_y = self.wq[k].rect[1];
                self.wq[k].owner
            }
            None => -1,
        }
    }
}

// ------------------------------------------------------------------ events

/// xdg_surface.configure: ack, then the content  [wlh_pop_xdg_configure]
pub(crate) fn wlh_pop_xdg_configure(wl: &mut Wl, _id: u32, a: &Args, u: u64) {
    let k = u as usize & (WQ_MAX - 1);
    let xs = wl.wq[k].xsurf;
    if xs == 0 {
        return;
    }
    wl.w.send(xs, &XDG_SURFACE_ACK_CONFIGURE, &[A::U(a.u(0))]);
    wl.wq[k].conf = true;
    wl.wq_draw(k);
    wl.w.flush();
}

/// the compositor dismissed the popup: its objects go (the UI closes it with the next
/// press anyway)  [wlh_pop_done]
pub(crate) fn wlh_pop_done(wl: &mut Wl, _id: u32, _a: &Args, u: u64) {
    wl.wq_objects_destroy(u as usize & (WQ_MAX - 1));
    wl.w.flush();
}

/// wl_buffer.release (udata slot * 2 + buffer)  [wlh_pop_release]
pub(crate) fn wlh_pop_release(wl: &mut Wl, _id: u32, _a: &Args, u: u64) {
    wl.wq[(u as usize >> 1) & (WQ_MAX - 1)].busy[u as usize & 1] = false;
}
