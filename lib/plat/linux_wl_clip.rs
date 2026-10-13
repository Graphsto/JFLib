// linux_wl_clip -- clipboard and file drops through the Wayland selection (wl_data_device)
//                                                                   [src/plat/linux/wl_clip.s]
//
//   Wl::clip_set(fmt, data) -> ok      CLIP_TEXT (UTF-8) / CLIP_PNG / CLIP_IMAGE (K1: data =
//        a CLIPIMG record, clipimg_split: offered as image/png + image/bmp).  The data is
//        copied; we offer it until another client takes the selection.
//   Wl::clip_get(fmt) -> bytes         our own selection is returned directly; otherwise the
//        offer is read through a pipe while Wayland events keep being dispatched (3 s
//        timeout, at most CLIP_MAX bytes).  Text preference: text/plain;charset=utf-8,
//        UTF8_STRING, text/plain, STRING / TEXT.  CLIP_PNG: image/png.  CLIP_IMAGE (K1):
//        image/png, else image/bmp (through clip_bmp_fix: paint.net's alpha rule).
//   Wl::clip_has(fmt) -> bool          our selection / the offer has the format (K1)
//   Wl::clip_init()                    the data device (after the globals are bound)
//   Wl::clip_pollfds(pollfds) -> n     pollfds of pending transfers (to other clients, and
//                                      the running drop transfer)
//   Wl::clip_service(pollfds)          continue those transfers (non-blocking)
//   wlh_dd_* / wlh_offer_* / wlh_src_*  event handlers
//
// Offers are server-created objects (ids >= 0xFF000000); their udata holds a bit mask of the
// MIME types offered.  Drag and drop (K1): enter accepts text/uri-list (action copy, data
// device v3); drop receives it through a non-blocking pipe served by the wait loop; at its
// end the file:// URIs become the pending drop list (drop_uri_list) and E_DROP (x, y =
// position in the window's pixels, data = count) is queued; then finish + destroy the offer.

use alloc::vec;
use alloc::vec::Vec;

use rt::os::{syscall3, SYS_READ, SYS_WRITE};

use super::proto::*;
use super::wire::{poll, sys_close, Args, PollFd, A, EAGAIN, EINTR, POLLERR, POLLHUP, POLLIN, POLLOUT};
use super::window::WLWF_USED;
use super::Wl;
use crate::{CLIP_DOCX, CLIP_HTML, CLIP_RTF, CLIP_IMAGE, CLIP_PNG, CLIP_TEXT, E_DROP, PW_MAX};

const MB_UTF8: u64 = 1; // text/plain;charset=utf-8
const MB_UTF8S: u64 = 2; // UTF8_STRING
const MB_TEXT: u64 = 4; // text/plain
const MB_STRING: u64 = 8; // STRING / TEXT
const MB_PNG: u64 = 16; // image/png
const MB_BMP: u64 = 32; // image/bmp
const MB_HTML: u64 = 128;
const MB_RTF: u64 = 256;
const MB_DOCX: u64 = 512;
const MB_HTML_UTF8: u64 = 1024;
const MB_RTF_APP: u64 = 2048;
const MB_URI: u64 = 64; // text/uri-list (K1)
const MB_ANYTEXT: u64 = MB_UTF8 | MB_UTF8S | MB_TEXT | MB_STRING;
const WR_MAX: usize = 8; // concurrent transfers to other clients
const READ_TIMEOUT: u64 = 3_000_000; // us
const CLIP_MAX: usize = 1 << 29; // largest clipboard transfer we read (K1)
const DND_MAX: usize = 1 << 20; // largest text/uri-list of a drop (K1)
const DND_ACTION_COPY: u32 = 1;
const SYS_FCNTL: usize = 72;
const SYS_PIPE2: usize = 293;
const F_SETFL: usize = 4;
const O_NONBLOCK: usize = 0x800;
const O_CLOEXEC: usize = 0x80000;

const MIME_UTF8: &[u8] = b"text/plain;charset=utf-8";
const MIME_UTF8S: &[u8] = b"UTF8_STRING";
const MIME_TEXT: &[u8] = b"text/plain";
const MIME_STRING: &[u8] = b"STRING";
const MIME_PNG: &[u8] = b"image/png";
const MIME_BMP: &[u8] = b"image/bmp";
const MIME_URI: &[u8] = b"text/uri-list";
const MIME_HTML: &[u8] = b"text/html";
const MIME_RTF: &[u8] = b"text/rtf";
const MIME_DOCX: &[u8] = b"application/x-jfgui-docx-fragment";
const MIME_HTML_UTF8: &[u8] = b"text/html;charset=utf-8";
const MIME_RTF_APP: &[u8] = b"application/rtf";
const RICH_MIMES: [(u32, &[u8], u64); 5] = [
    (CLIP_HTML, MIME_HTML, MB_HTML), (CLIP_RTF, MIME_RTF, MB_RTF), (CLIP_DOCX, MIME_DOCX, MB_DOCX),
    (CLIP_HTML, MIME_HTML_UTF8, MB_HTML_UTF8), (CLIP_RTF, MIME_RTF_APP, MB_RTF_APP),
];
// offered for text, in this order
const TEXT_MIMES: [&[u8]; 5] = [MIME_UTF8, MIME_UTF8S, MIME_TEXT, MIME_STRING, b"TEXT"];

static MIME_TABLE: [(&[u8], u64); 13] = [(MIME_UTF8, MB_UTF8), (MIME_UTF8S, MB_UTF8S), (MIME_TEXT, MB_TEXT),
                                       (MIME_STRING, MB_STRING), (b"TEXT", MB_STRING), (MIME_PNG, MB_PNG),
                                       (MIME_BMP, MB_BMP), (MIME_URI, MB_URI), (MIME_HTML, MB_HTML), (MIME_RTF, MB_RTF), (MIME_DOCX, MB_DOCX), (MIME_HTML_UTF8, MB_HTML_UTF8), (MIME_RTF_APP, MB_RTF_APP)];

/// a transfer to another client (fd 0 = free slot; fds are never 0 here)
#[derive(Clone, Copy, Default)]
struct Writer {
    fd: i32,
    off: usize,
    bmp: bool, // sends the image/bmp copy, else the data
    rich: u32, // clipboard_platform: rich format, 0 = primary text/image
}

#[derive(Default)]
pub(crate) struct Clip {
    src_data: Option<Vec<u8>>,
    src_bmp: Option<Vec<u8>>, // CLIP_IMAGE: the image/bmp bytes
    src_fmt: u32,
    src_rich: Vec<(u32, Vec<u8>)>, // clipboard_platform: alternatives of the same selection
    dnd_buf: Option<Vec<u8>>, // drop transfer: text/uri-list bytes
    writers: [Writer; WR_MAX],
    ddev: u32,
    src: u32,
    sel_offer: u32,
    dnd_offer: u32, // offer of the drag over our window
    dnd_action: u32,
    dnd_ok: bool, // it offers text/uri-list
    dnd_win: i32,
    dnd_x: i32,
    dnd_y: i32,
    dnd_xoffer: u32, // offer of the running drop transfer
    dnd_xaction: u32,
    dnd_xwin: i32,
    dnd_xx: i32,
    dnd_xy: i32,
    dnd_rfd: i32, // its pipe (0 = none)
}

/// MB_* bit of a MIME type name (0 = not used)  [mime_bit]
fn mime_bit(s: &[u8]) -> u64 {
    MIME_TABLE.iter().find(|m| m.0 == s).map_or(0, |m| m.1)
}

fn pipe2(flags: usize) -> Option<[i32; 2]> {
    let mut p = [0i32; 2];
    // SAFETY: pipe2 writes two descriptors.
    let r = unsafe { syscall3(SYS_PIPE2, p.as_mut_ptr() as usize, flags, 0) };
    if r == 0 { Some(p) } else { None }
}

impl Wl {
    /// wl_data_device_manager.get_data_device(seat)  [wl_clip_init]
    pub(crate) fn clip_init(&mut self) {
        if self.g.ddm == 0 || self.g.seat == 0 {
            return;
        }
        self.clip.ddev =
            self.w.send(self.g.ddm, &DDM_GET_DATA_DEVICE, &[A::N(&IFACE_WL_DATA_DEVICE), A::O(self.g.seat)]);
    }

    /// pixels of a drag position (the window's scale)  [dnd_px]
    fn dnd_px(&self, fixed: i32, w: i32) -> i32 {
        let r = &self.win[w as usize & (PW_MAX - 1)];
        let s = if r.flags & WLWF_USED != 0 { r.s120 } else { self.inp.s120_test };
        (s as i64 * fixed as i64 / 30720) as i32
    }

    /// destroy the current drag-and-drop offer  [dnd_drop_offer]
    fn dnd_drop_offer(&mut self) {
        let o = self.clip.dnd_offer;
        self.clip.dnd_offer = 0;
        if o != 0 && o != self.clip.sel_offer {
            self.w.send(o, &DATA_OFFER_DESTROY, &[]);
        }
    }

    /// the drop transfer is over: finish (when an action was negotiated) and destroy the
    /// offer, close the pipe, free the buffer  [dnd_end]
    fn dnd_end(&mut self, ok: bool) {
        let fd = self.clip.dnd_rfd;
        self.clip.dnd_rfd = 0;
        if fd != 0 {
            sys_close(fd);
        }
        let o = self.clip.dnd_xoffer;
        self.clip.dnd_xoffer = 0;
        if o != 0 {
            if ok && self.g.ddm_ver >= 3 && self.clip.dnd_xaction != 0 {
                self.w.send(o, &DATA_OFFER_FINISH, &[]);
            }
            if o != self.clip.sel_offer {
                self.w.send(o, &DATA_OFFER_DESTROY, &[]);
                self.w.flush();
            }
        }
        self.clip.dnd_buf = None;
    }

    /// the drop pipe is readable: read what is there; at its end the paths  [dnd_read]
    fn dnd_read(&mut self) {
        loop {
            let Some(buf) = self.clip.dnd_buf.as_mut() else { return };
            if buf.capacity() - buf.len() <= 1024 {
                let cap = buf.capacity() * 2;
                if cap > DND_MAX {
                    self.dnd_end(false); // too large for a list of paths
                    return;
                }
                buf.reserve_exact(cap - buf.len());
            }
            let len = buf.len();
            let room = buf.capacity() - len;
            // SAFETY: reads into the spare capacity of the buffer.
            let r = unsafe { syscall3(SYS_READ, self.clip.dnd_rfd as usize, buf.as_mut_ptr().add(len) as usize, room) };
            if r == -EINTR {
                continue;
            }
            if r == -EAGAIN {
                return; // more later
            }
            if r < 0 {
                self.dnd_end(false);
                return;
            }
            if r == 0 {
                crate::drop_reset();
                let n = crate::drop_uri_list(buf);
                if n != 0 {
                    self.inp.evwin = self.clip.dnd_xwin;
                    let (x, y) = (self.clip.dnd_xx, self.clip.dnd_xy);
                    self.push(E_DROP, 0, x, y, n);
                }
                self.dnd_end(true);
                return;
            }
            // SAFETY: the kernel wrote r bytes behind len.
            unsafe { buf.set_len(len + r as usize) };
        }
    }

    // ------------------------------------------------------------------ source (copy side)

    /// stop offering our data: destroy the source, end transfers  [clip_drop_source]
    fn clip_drop_source(&mut self) {
        let s = self.clip.src;
        if s != 0 {
            self.clip.src = 0;
            self.w.send(s, &DATA_SOURCE_DESTROY, &[]);
        }
        for wr in self.clip.writers.iter_mut() {
            if wr.fd != 0 {
                sys_close(wr.fd);
                wr.fd = 0;
            }
        }
        self.clip.src_data = None;
        self.clip.src_bmp = None;
        self.clip.src_rich.clear();
    }

    /// offer data on the clipboard -> ok  [wl_plat_clip_set]
    pub(crate) fn clip_set(&mut self, fmt: u32, data: &[u8]) -> bool {
        if matches!(fmt, CLIP_HTML | CLIP_RTF | CLIP_DOCX) { return self.clip_set_many(&[(fmt, data)]); }
        if self.clip.ddev == 0 {
            return false;
        }
        let (payload, bmp) = match fmt {
            CLIP_TEXT | CLIP_PNG => (data, None),
            CLIP_IMAGE => {
                let Some((png, bmp)) = crate::clipimg_split(data) else { return false };
                if png.is_empty() {
                    return false;
                }
                (png, if bmp.is_empty() { None } else { Some(bmp.to_vec()) })
            }
            _ => return false,
        };
        let copy = payload.to_vec();
        self.clip_drop_source();
        let has_bmp = bmp.is_some();
        self.clip.src_data = Some(copy);
        self.clip.src_bmp = bmp;
        self.clip.src_fmt = fmt;
        let src = self.w.send(self.g.ddm, &DDM_CREATE_DATA_SOURCE, &[A::N(&IFACE_WL_DATA_SOURCE)]);
        self.clip.src = src;
        if fmt == CLIP_TEXT {
            for m in TEXT_MIMES {
                self.w.send(src, &DATA_SOURCE_OFFER, &[A::S(Some(m))]);
            }
        } else {
            self.w.send(src, &DATA_SOURCE_OFFER, &[A::S(Some(MIME_PNG))]);
            if has_bmp {
                self.w.send(src, &DATA_SOURCE_OFFER, &[A::S(Some(MIME_BMP))]);
            }
        }
        let (dd, serial) = (self.clip.ddev, self.inp.input_serial);
        self.w.send(dd, &DATA_DEVICE_SET_SELECTION, &[A::O(src), A::U(serial)]);
        self.w.flush();
        true
    }

    // clipboard_platform: offer every representation under a single wl_data_source.
    pub(crate) fn clip_set_many(&mut self, entries: &[(u32, &[u8])]) -> bool {
        if self.clip.ddev == 0 || !crate::clip_many_valid(entries) { return false; }
        let text = entries.iter().find(|e| e.0 == CLIP_TEXT).map(|e| e.1.to_vec());
        let rich = entries.iter().filter(|e| e.0 != CLIP_TEXT).map(|e| (e.0, e.1.to_vec())).collect();
        self.clip_drop_source();
        self.clip.src_fmt = if text.is_some() { CLIP_TEXT } else { 0 };
        self.clip.src_data = text;
        self.clip.src_rich = rich;
        let src = self.w.send(self.g.ddm, &DDM_CREATE_DATA_SOURCE, &[A::N(&IFACE_WL_DATA_SOURCE)]);
        self.clip.src = src;
        for &(fmt, _) in entries {
            if fmt == CLIP_TEXT {
                for mime in TEXT_MIMES { self.w.send(src, &DATA_SOURCE_OFFER, &[A::S(Some(mime))]); }
            } else {
                for (_, mime, _) in RICH_MIMES.iter().filter(|m| m.0 == fmt) {
                    self.w.send(src, &DATA_SOURCE_OFFER, &[A::S(Some(mime))]);
                }
            }
        }
        self.w.send(self.clip.ddev, &DATA_DEVICE_SET_SELECTION, &[A::O(src), A::U(self.inp.input_serial)]);
        self.w.flush();
        true
    }

    /// write as much as possible; close when done or failed  [writer_run]
    fn writer_run(&mut self, k: usize) {
        loop {
            let wr = self.clip.writers[k];
            let data: &[u8] = if wr.rich != 0 {
                self.clip.src_rich.iter().find(|e| e.0 == wr.rich).map_or(&[], |e| e.1.as_slice())
            } else if wr.bmp {
                self.clip.src_bmp.as_deref().unwrap_or(&[])
            } else {
                self.clip.src_data.as_deref().unwrap_or(&[])
            };
            if wr.off >= data.len() {
                break;
            }
            let rest = &data[wr.off..];
            // SAFETY: writes from our data.
            let r = unsafe { syscall3(SYS_WRITE, wr.fd as usize, rest.as_ptr() as usize, rest.len()) };
            if r == -EINTR {
                continue;
            }
            if r == -EAGAIN {
                return; // continue when poll says POLLOUT
            }
            if r <= 0 {
                break; // EPIPE etc.
            }
            self.clip.writers[k].off += r as usize;
        }
        sys_close(self.clip.writers[k].fd);
        self.clip.writers[k].fd = 0;
    }

    /// pollfds of the transfers (POLLOUT for every writer, POLLIN for the drop transfer)
    /// -> entries written  [wl_clip_pollfds]
    pub(crate) fn clip_pollfds(&self, fds: &mut [PollFd]) -> usize {
        let mut n = 0;
        for wr in &self.clip.writers {
            if n >= fds.len() {
                return n;
            }
            if wr.fd != 0 {
                fds[n] = PollFd { fd: wr.fd, events: POLLOUT, revents: 0 };
                n += 1;
            }
        }
        if n < fds.len() && self.clip.dnd_rfd != 0 {
            fds[n] = PollFd { fd: self.clip.dnd_rfd, events: POLLIN, revents: 0 };
            n += 1;
        }
        n
    }

    /// continue transfers whose fd became ready  [wl_clip_service]
    pub(crate) fn clip_service(&mut self, fds: &[PollFd]) {
        for p in fds {
            if p.revents == 0 {
                continue;
            }
            if p.fd == self.clip.dnd_rfd && p.fd != 0 {
                self.dnd_read();
                continue;
            }
            if let Some(k) = self.clip.writers.iter().position(|w| w.fd == p.fd) {
                self.writer_run(k);
            }
        }
    }

    // ------------------------------------------------------------------ paste

    /// our own selection serves fmt  [own_has]
    fn own_has(&self, fmt: u32) -> bool {
        if self.clip.src_rich.iter().any(|e| e.0 == fmt) { return true; }
        let f = self.clip.src_fmt;
        if fmt == CLIP_TEXT {
            return f == CLIP_TEXT;
        }
        (f == CLIP_PNG || f == CLIP_IMAGE) && (fmt == CLIP_PNG || fmt == CLIP_IMAGE)
    }

    /// the clipboard offers fmt (K1)  [wl_plat_clip_has]
    pub(crate) fn clip_has(&self, fmt: u32) -> bool {
        if self.clip.src != 0 {
            return self.own_has(fmt);
        }
        let o = self.clip.sel_offer;
        if o == 0 {
            return false;
        }
        let Some((_, mask, _)) = self.w.obj_iface(o) else { return false };
        let want = match fmt {
            CLIP_TEXT => MB_ANYTEXT,
            CLIP_PNG => MB_PNG,
            CLIP_IMAGE => MB_PNG | MB_BMP,
            CLIP_HTML => MB_HTML | MB_HTML_UTF8,
            CLIP_RTF => MB_RTF | MB_RTF_APP,
            CLIP_DOCX => MB_DOCX,
            _ => 0,
        };
        mask & want != 0
    }

    /// read the clipboard  [wl_plat_clip_get]
    pub(crate) fn clip_get(&mut self, fmt: u32) -> Option<Vec<u8>> {
        // our own selection
        if self.clip.src != 0 {
            if !self.own_has(fmt) {
                return None;
            }
            return if matches!(fmt, CLIP_HTML | CLIP_RTF | CLIP_DOCX) {
                self.clip.src_rich.iter().find(|e| e.0 == fmt).map(|e| e.1.clone())
            } else { self.clip.src_data.clone() };
        }
        let o = self.clip.sel_offer;
        if o == 0 {
            return None;
        }
        let (_, mask, _) = self.w.obj_iface(o)?;
        let mime: Option<&[u8]> = match fmt {
            CLIP_HTML | CLIP_RTF | CLIP_DOCX => RICH_MIMES.iter().find(|m| m.0 == fmt && mask & m.2 != 0).map(|m| m.1),
            CLIP_PNG => (mask & MB_PNG != 0).then_some(MIME_PNG),
            CLIP_IMAGE => {
                if mask & MB_PNG != 0 {
                    Some(MIME_PNG)
                } else if mask & MB_BMP != 0 {
                    Some(MIME_BMP)
                } else {
                    None
                }
            }
            CLIP_TEXT => [(MB_UTF8, MIME_UTF8), (MB_UTF8S, MIME_UTF8S), (MB_TEXT, MIME_TEXT), (MB_STRING, MIME_STRING)]
                .iter()
                .find(|m| mask & m.0 != 0)
                .map(|m| m.1),
            _ => return None,
        };
        let Some(mime) = mime else {
            if self.w.debug {
                crate::write_stderr(alloc::format!("[wl] clipboard: no usable format (offer mask {mask:x})\n").as_bytes());
            }
            return None;
        };
        let p = pipe2(O_CLOEXEC)?;
        // write end: sent, then closed by flush
        self.w.send(o, &DATA_OFFER_RECEIVE, &[A::S(Some(mime)), A::H(p[1])]);
        self.w.flush_wait();
        let mut buf: Vec<u8> = vec![0; 65536];
        let mut len = 0usize;
        let rich = matches!(fmt, CLIP_HTML | CLIP_RTF | CLIP_DOCX);
        let deadline = crate::time_us() + READ_TIMEOUT;
        loop {
            let now = crate::time_us();
            if now >= deadline {
                if self.w.debug {
                    crate::write_stderr(b"[wl] clipboard: no data within 3 s\n");
                }
                sys_close(p[0]);
                return None;
            }
            let ms = ((deadline - now) / 1000) as i32 + 1;
            let mut fds = [PollFd { fd: p[0], events: POLLIN, revents: 0 }, PollFd { fd: self.w.fd, events: POLLIN,
                                                                                    revents: 0 }];
            let r = poll(&mut fds, ms);
            if r == -EINTR {
                continue;
            }
            if r < 0 {
                sys_close(p[0]);
                return None;
            }
            if fds[1].revents & (POLLIN | POLLERR | POLLHUP) != 0 {
                self.w.recv(); // keep the connection alive (pings, ...)
                self.dispatch();
            }
            if fds[0].revents & (POLLIN | POLLERR | POLLHUP) == 0 {
                continue;
            }
            if buf.len() - len <= 4096 {
                let cap = if rich { (buf.len() * 2).min(crate::CLIP_RICH_MAX + 1) } else { buf.len() * 2 };
                if !rich && cap > CLIP_MAX {
                    sys_close(p[0]); // K1: other programs' data is bounded
                    return None;
                }
                buf.resize(cap, 0);
            }
            let room = buf.len() - len - usize::from(!rich);
            // SAFETY: reads into buf[len..len + room].
            let r = unsafe { syscall3(SYS_READ, p[0] as usize, buf.as_mut_ptr().add(len) as usize, room) };
            if r == -EINTR {
                continue;
            }
            if r < 0 {
                sys_close(p[0]);
                return None;
            }
            if r == 0 {
                break;
            }
            len += r as usize;
            if rich && len > crate::CLIP_RICH_MAX {
                sys_close(p[0]);
                return None;
            }
        }
        sys_close(p[0]);
        buf.truncate(len);
        if mime == MIME_BMP {
            return crate::clip_bmp_fix(&buf); // paint.net's alpha rule (a new buffer)
        }
        Some(buf)
    }
}

// ------------------------------------------------------------------ offers (paste side)

/// wl_data_device.data_offer(new id): track the new offer  [wlh_dd_offer]
pub(crate) fn wlh_dd_offer(wl: &mut Wl, _id: u32, a: &Args, _u: u64) {
    wl.w.srv_add(a.u(0), &IFACE_WL_DATA_OFFER, 0);
}

/// wl_data_offer.offer(mime)  [wlh_offer_mime]
pub(crate) fn wlh_offer_mime(wl: &mut Wl, id: u32, a: &Args, u: u64) {
    if let Some(m) = a.s(0) {
        wl.w.set_udata(id, u | mime_bit(m));
    }
}

/// wl_data_offer.action(dnd_action) (K1): the negotiated action of the drag-and-drop offer
/// [wlh_offer_action]
pub(crate) fn wlh_offer_action(wl: &mut Wl, id: u32, a: &Args, _u: u64) {
    if id == wl.clip.dnd_offer {
        wl.clip.dnd_action = a.u(0);
    } else if id == wl.clip.dnd_xoffer {
        wl.clip.dnd_xaction = a.u(0);
    }
}

/// the clipboard now holds this offer (or nothing)  [wlh_dd_selection]
pub(crate) fn wlh_dd_selection(wl: &mut Wl, _id: u32, a: &Args, _u: u64) {
    let new = a.u(0);
    let old = wl.clip.sel_offer;
    if old != 0 && old != new && old != wl.clip.dnd_offer && old != wl.clip.dnd_xoffer {
        wl.w.send(old, &DATA_OFFER_DESTROY, &[]);
    }
    wl.clip.sel_offer = new;
}

/// drag-and-drop enter(serial, surface, x, y, offer): accept text/uri-list with the action
/// copy (K1)  [wlh_dd_enter]
pub(crate) fn wlh_dd_enter(wl: &mut Wl, _id: u32, a: &Args, _u: u64) {
    wl.dnd_drop_offer();
    let offer = a.u(4);
    wl.clip.dnd_offer = offer;
    wl.clip.dnd_action = 0;
    wl.clip.dnd_ok = false;
    let w = wl.find(a.u(1)).max(0);
    wl.clip.dnd_win = w;
    wl.clip.dnd_x = wl.dnd_px(a.i(2), w);
    wl.clip.dnd_y = wl.dnd_px(a.i(3), w);
    if offer == 0 {
        return;
    }
    let Some((_, mask, _)) = wl.w.obj_iface(offer) else { return };
    let mut mime: Option<&[u8]> = None; // accept(serial, NULL): nothing for us
    if mask & MB_URI != 0 {
        wl.clip.dnd_ok = true;
        mime = Some(MIME_URI);
    }
    wl.w.send(offer, &DATA_OFFER_ACCEPT, &[A::U(a.u(0)), A::S(mime)]);
    if wl.clip.dnd_ok && wl.g.ddm_ver >= 3 {
        wl.w.send(offer, &DATA_OFFER_SET_ACTIONS, &[A::U(DND_ACTION_COPY), A::U(DND_ACTION_COPY)]);
    }
    wl.w.flush();
}

/// motion(time, x, y) during a drag (K1)  [wlh_dd_motion]
pub(crate) fn wlh_dd_motion(wl: &mut Wl, _id: u32, a: &Args, _u: u64) {
    let w = wl.clip.dnd_win;
    wl.clip.dnd_x = wl.dnd_px(a.i(1), w);
    wl.clip.dnd_y = wl.dnd_px(a.i(2), w);
}

/// drag-and-drop leave (also after a drop)  [wlh_dd_leave]
pub(crate) fn wlh_dd_leave(wl: &mut Wl, _id: u32, _a: &Args, _u: u64) {
    wl.dnd_drop_offer();
}

/// drop: receive text/uri-list through a pipe (K1)  [wlh_dd_drop]
pub(crate) fn wlh_dd_drop(wl: &mut Wl, _id: u32, _a: &Args, _u: u64) {
    let o = wl.clip.dnd_offer;
    if o == 0 {
        return;
    }
    if !wl.clip.dnd_ok || wl.clip.dnd_rfd != 0 {
        wl.dnd_drop_offer(); // one transfer at a time
        return;
    }
    let Some(p) = pipe2(O_CLOEXEC | O_NONBLOCK) else {
        wl.dnd_drop_offer();
        return;
    };
    wl.clip.dnd_buf = Some(Vec::with_capacity(4096));
    // write end: sent, then closed by flush
    wl.w.send(o, &DATA_OFFER_RECEIVE, &[A::S(Some(MIME_URI)), A::H(p[1])]);
    wl.w.flush();
    let c = &mut wl.clip;
    c.dnd_rfd = p[0];
    c.dnd_xoffer = o;
    c.dnd_xaction = c.dnd_action;
    c.dnd_xwin = c.dnd_win;
    c.dnd_xx = c.dnd_x;
    c.dnd_xy = c.dnd_y;
    c.dnd_offer = 0; // leave must not destroy it
}

// ------------------------------------------------------------------ source events

/// send(mime, fd): write our data to fd (non-blocking)  [wlh_src_send]
pub(crate) fn wlh_src_send(wl: &mut Wl, id: u32, a: &Args, _u: u64) {
    let fd = a.u(1) as i32;
    if id != wl.clip.src || (wl.clip.src_data.is_none() && wl.clip.src_rich.is_empty()) {
        sys_close(fd);
        return;
    }
    let Some(k) = wl.clip.writers.iter().position(|w| w.fd == 0) else {
        sys_close(fd); // too many readers at once
        return;
    };
    // O_NONBLOCK so we never stall the UI
    // SAFETY: fcntl on the fd the compositor gave us.
    unsafe { syscall3(SYS_FCNTL, fd as usize, F_SETFL, O_NONBLOCK) };
    let bmp = wl.clip.src_bmp.is_some() && a.s(0) == Some(MIME_BMP); // image/bmp -> the BMP copy
    let rich = RICH_MIMES.iter().find(|m| a.s(0) == Some(m.1)).map_or(0, |m| m.0);
    if rich != 0 && !wl.clip.src_rich.iter().any(|e| e.0 == rich) { sys_close(fd); return; }
    wl.clip.writers[k] = Writer { fd, off: 0, bmp, rich };
    wl.writer_run(k);
}

/// another client owns the selection now  [wlh_src_cancelled]
pub(crate) fn wlh_src_cancelled(wl: &mut Wl, id: u32, _a: &Args, _u: u64) {
    if id == wl.clip.src {
        wl.clip_drop_source();
    } else {
        wl.w.send(id, &DATA_SOURCE_DESTROY, &[]);
    }
}

