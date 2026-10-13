// linux_x11_clip -- the clipboard through the X11 CLIPBOARD selection, and (K1) file drops
// through XDND                                    [src/plat/linux/x11_clip.s, x11_dnd.s]
//
//   clip_set(fmt, data) -> bool      CLIP_TEXT (UTF-8) / CLIP_PNG / CLIP_IMAGE (CLIPIMG bytes,
//        clipimg_pack: targets image/png + image/bmp).  The data is copied; we own CLIPBOARD
//        (timestamp of the last event) until another client takes it (SelectionClear).
//   clip_get(fmt) -> data            our own selection directly; otherwise ConvertSelection
//        into a property of our window, wait for SelectionNotify (other events keep being
//        handled, 3 s timeout), read it (INCR transfers included).  Text: UTF8_STRING,
//        text/plain;charset=utf-8, STRING (Latin-1, converted).  Images: image/png;
//        CLIP_IMAGE: TARGETS first, then image/png, else image/bmp (through clip_bmp_fix:
//        paint.net's alpha rule).  At most CLIP_MAX bytes.
//   clip_has(fmt) -> bool            the CLIPBOARD owner offers fmt: our own data directly;
//        else GetSelectionOwner at most every 250 ms and TARGETS (300 ms timeout) when the
//        owner changed or the answer is older than 2 s
//   clip_request(ev)                 SelectionRequest: TARGETS, TIMESTAMP, UTF8_STRING, TEXT,
//        text/plain;charset=utf-8, text/plain, STRING (UTF-8 -> Latin-1), image/png,
//        image/bmp (CLIP_IMAGE); MULTIPLE and others are refused.  Data that does not fit
//        into one request (min(maximum request, request buffer) - 64 bytes: ~1 MB with
//        BIG-REQUESTS, 256 KB without) goes out with INCR in 64 KB chunks, driven by
//        PropertyNotify (Deleted) on the requestor.
//   clip_notify / clip_clear / clip_property (event handlers), clip_init (the selection)
//   dnd_window(win)                  XdndAware = 5 on every new window (the first call interns
//                                    the atoms, also image/bmp for the clipboard)
//   dnd_client(ev, index) -> consumed   XdndEnter / Position / Leave / Drop
//   dnd_notify(ev)                   SelectionNotify of XdndSelection
//   dnd_deferred()                   (deferred) read the dropped text/uri-list, queue E_DROP
//                                    (x, y = window pixels, data = files), send XdndFinished
//
// Test aids: jfgui_X11SELECTION=NAME uses that selection instead of CLIPBOARD (WSLg's Weston
// mirrors CLIPBOARD and crashes after reading any INCR selection); jfgui_X11INCR=N sends
// everything above N bytes with INCR in N / 2 chunks.
//
// XDND: Enter accepts the source when text/uri-list is among its first three types or it
// has more types (XdndTypeList is not read: the conversion decides).  Position: XdndStatus
// (accept, action copy) with an empty rectangle, so every move is reported.  Drop:
// ConvertSelection(XdndSelection, text/uri-list) into jfgui_DND on the main window with the
// drop's timestamp; the property is read after the event handlers (at most DND_MAX bytes, no
// INCR), parsed by drop_uri_list (file:// URIs, %XX), then XdndFinished.  A drop without an
// answer in 5 s is finished as failed.  All message fields are untrusted: the source window
// must match the one of XdndEnter.

use alloc::vec::Vec;

use super::*;
use crate::{CLIP_DOCX, CLIP_HTML, CLIP_RTF, CLIP_IMAGE, CLIP_PNG, CLIP_TEXT, E_DROP};

pub(super) const XF_MAX: usize = 4; // concurrent INCR transfers
const INCR_CHUNK: u32 = 65536;
const INCR_MIN: u32 = 256;
const CLIP_TIMEOUT: u64 = 3000000; // us
const CLIP_MAX: usize = 1 << 29; // largest paste we read
const HAS_POLL: u64 = 250000; // us between two GetSelectionOwner of clip_has
const HAS_FRESH: u64 = 2000000; // us an owner's TARGETS answer is kept
const HAS_TIMEOUT: u64 = 300000; // us for that TARGETS answer
const HB_TEXT: u32 = 1; // clip_has cache bits
const HB_PNG: u32 = 2;
const HB_BMP: u32 = 4;
// XDND
const XDND_VERSION: u32 = 5;
const DND_MAX: u32 = 1 << 20; // bytes of a text/uri-list
const DND_TIMEOUT: u64 = 5000000; // us
const D_AWARE: usize = 0;
const D_ENTER: usize = 1;
const D_POSITION: usize = 2;
const D_STATUS: usize = 3;
const D_LEAVE: usize = 4;
const D_DROP: usize = 5;
const D_FINISHED: usize = 6;
const D_SELECTION: usize = 7;
const D_COPY: usize = 8;
const D_URI: usize = 9;
const D_PROP: usize = 10;
const D_BMP: usize = 11;
pub(super) const D_COUNT: usize = 12;
const XD_NAMES: [&str; D_COUNT] = [
    "XdndAware",
    "XdndEnter",
    "XdndPosition",
    "XdndStatus",
    "XdndLeave",
    "XdndDrop",
    "XdndFinished",
    "XdndSelection",
    "XdndActionCopy",
    "text/uri-list",
    "jfgui_DND",
    "image/bmp",
];

/// utf8_decode of core/str.s at d[i] (bytes past the end read as NUL) -> (codepoint, next);
/// invalid bytes, overlong forms, surrogates and values above U+10FFFF decode as U+FFFD and
/// advance one byte
fn utf8_decode(d: &[u8], i: usize) -> (u32, usize) {
    let at = |k: usize| d.get(k).copied().unwrap_or(0) as u32;
    let c = at(i);
    if c < 0x80 {
        return (c, i + 1);
    }
    let (mut v, n, min) = match c {
        0xC2..=0xDF => (c & 0x1F, 1, 0x80),
        0xE0..=0xEF => (c & 0x0F, 2, 0x800),
        0xF0..=0xF4 => (c & 7, 3, 0x10000),
        _ => return (0xFFFD, i + 1),
    };
    let mut p = i + 1;
    for _ in 0..n {
        let b = at(p);
        if b & 0xC0 != 0x80 {
            return (0xFFFD, i + 1);
        }
        v = v << 6 | (b & 0x3F);
        p += 1;
    }
    if v < min || v > 0x10FFFF || v & 0xFFFFF800 == 0xD800 {
        return (0xFFFD, i + 1);
    }
    (v, p)
}

/// to_latin1(utf8): characters above U+00FF become '?'
fn to_latin1(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(d.len());
    let mut i = 0;
    while i < d.len() {
        let (c, n) = utf8_decode(d, i);
        i = n;
        out.push(if c <= 0xFF { c as u8 } else { b'?' });
    }
    out
}

/// from_latin1: Latin-1 -> UTF-8
fn from_latin1(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(d.len() * 2);
    for &b in d {
        if b < 0x80 {
            out.push(b);
        } else {
            out.push(0xC0 | b >> 6);
            out.push(0x80 | (b & 0x3F));
        }
    }
    out
}

/// own_has(cb_fmt, fmt): our own selection serves fmt
fn own_has(own: u32, fmt: u32) -> bool {
    if fmt == CLIP_TEXT {
        return own == CLIP_TEXT;
    }
    (own == CLIP_PNG || own == CLIP_IMAGE) && (fmt == CLIP_PNG || fmt == CLIP_IMAGE)
}

impl X11 {
    // ------------------------------------------------------------------ copy side

    /// x11_plat_clip_set(fmt, data) -> false on failure
    pub(super) fn clip_set(&mut self, fmt: u32, data: &[u8]) -> bool {
        if matches!(fmt, CLIP_HTML | CLIP_RTF | CLIP_DOCX) { return self.clip_set_many(&[(fmt, data)]); }
        let win0 = self.xw[0].win;
        if win0 == 0 {
            return false;
        }
        let (d, bmp) = match fmt {
            CLIP_TEXT | CLIP_PNG => (data.to_vec(), None),
            CLIP_IMAGE => match crate::clipimg_split(data) {
                Some((png, bmp)) if !png.is_empty() => (png.to_vec(), if bmp.is_empty() { None } else { Some(bmp.to_vec()) }),
                _ => return false,
            },
            _ => return false,
        };
        self.clip_drop();
        self.cb_data = Some(d);
        self.cb_bmp = bmp;
        self.cb_fmt = fmt;
        self.clip_claim()
    }

    // clipboard_platform: TEXT and rich representations share a single selection owner.
    pub(super) fn clip_set_many(&mut self, entries: &[(u32, &[u8])]) -> bool {
        if self.xw[0].win == 0 || !crate::clip_many_valid(entries) { return false; }
        if self.cb_rich_atoms.contains(&0) { return false; }
        let text = entries.iter().find(|e| e.0 == CLIP_TEXT).map(|e| e.1.to_vec());
        let rich = entries.iter().filter(|e| e.0 != CLIP_TEXT).map(|e| (e.0, e.1.to_vec())).collect();
        self.clip_drop();
        self.cb_fmt = if text.is_some() { CLIP_TEXT } else { 0 };
        self.cb_data = text;
        self.cb_rich = rich;
        self.clip_claim()
    }

    fn clip_claim(&mut self) -> bool {
        let win0 = self.xw[0].win;
        self.cb_time = self.time;
        let (sel, t) = (self.cb_sel, self.cb_time);
        let (o, _) = self.req(XR_SET_SELECTION_OWNER, 16);
        wr32(&mut self.out, o, win0);
        wr32(&mut self.out, o + 4, sel);
        wr32(&mut self.out, o + 8, t);
        let (o, seq) = self.req(XR_GET_SELECTION_OWNER, 8);
        wr32(&mut self.out, o, sel);
        match self.reply(seq) {
            Some(r) if rd32(&r, 8) == win0 => {
                self.cb_own = true;
                true
            }
            _ => {
                self.clip_drop();
                false
            }
        }
    }

    /// clip_drop: forget our selection data, end transfers to other clients
    fn clip_drop(&mut self) {
        self.cb_own = false;
        for k in 0..XF_MAX {
            if self.xfers[k].req != 0 {
                self.xfer_end(k);
            }
        }
        self.cb_data = None;
        self.cb_bmp = None;
        self.cb_rich.clear();
    }

    /// x11_clip_clear(ev): SelectionClear(time, owner, selection) -- someone else owns it
    pub(super) fn clip_clear(&mut self, b: &[u8]) {
        if rd32(b, 8) == self.xw[0].win && rd32(b, 12) == self.cb_sel {
            self.clip_drop();
        }
    }

    /// x11_clip_request(ev): SelectionRequest(time, owner, requestor, selection, target,
    /// property) -- convert our data, answer with SelectionNotify
    pub(super) fn clip_request(&mut self, b: &[u8]) {
        let req = rd32(b, 12);
        let target = rd32(b, 20);
        let mut prop = rd32(b, 24);
        if prop == 0 {
            prop = target; // obsolete clients: property = target
        }
        let mut answered = 0u32;
        if self.cb_own && rd32(b, 16) == self.cb_sel {
            let a = self.atoms;
            if target == a[A_TARGETS] {
                let mut list = Vec::from([a[A_TARGETS], a[A_TIMESTAMP]]);
                if self.cb_fmt == CLIP_TEXT {
                    list.push(a[A_UTF8_STRING]);
                    list.extend_from_slice(&[a[A_TEXT_PLAIN_UTF8], a[A_TEXT_PLAIN], XA_STRING, a[A_TEXT]]);
                } else if matches!(self.cb_fmt, CLIP_PNG | CLIP_IMAGE) {
                    list.push(a[A_IMAGE_PNG]);
                    if self.cb_bmp.is_some() && self.xd_bmp_atom != 0 {
                        list.push(self.xd_bmp_atom);
                    }
                }
                for &(fmt, _) in &self.cb_rich {
                    list.push(self.cb_rich_atoms[(fmt - CLIP_HTML) as usize]);
                    if fmt == CLIP_HTML { list.push(self.cb_rich_atoms[3]); }
                    if fmt == CLIP_RTF { list.push(self.cb_rich_atoms[4]); }
                }
                self.change_prop32(req, prop, XA_ATOM, &list);
                answered = prop;
            } else if target == a[A_TIMESTAMP] {
                let t = self.cb_time;
                self.change_prop32(req, prop, XA_INTEGER, &[t]);
                answered = prop;
            } else {
                let mut send: Option<(u32, Vec<u8>)> = None;
                if self.cb_fmt == CLIP_TEXT {
                    let d = self.cb_data.clone().unwrap_or_default();
                    if target == a[A_UTF8_STRING] || target == a[A_TEXT] {
                        send = Some((a[A_UTF8_STRING], d)); // TEXT: answered as UTF8_STRING
                    } else if target == a[A_TEXT_PLAIN_UTF8] || target == a[A_TEXT_PLAIN] {
                        send = Some((target, d));
                    } else if target == XA_STRING {
                        send = Some((XA_STRING, to_latin1(&d))); // STRING: a Latin-1 copy
                    }
                } else if target == a[A_IMAGE_PNG] && matches!(self.cb_fmt, CLIP_PNG | CLIP_IMAGE) {
                    send = Some((target, self.cb_data.clone().unwrap_or_default()));
                } else if target != 0 && target == self.xd_bmp_atom {
                    if let Some(bmp) = self.cb_bmp.clone() {
                        send = Some((target, bmp)); // image/bmp of CLIP_IMAGE
                    }
                }
                if send.is_none() {
                    if let Some((_, data)) = self.cb_rich.iter().find(|(fmt, _)| self.cb_rich_atoms[(fmt - CLIP_HTML) as usize] == target
                        || (*fmt == CLIP_HTML && target == self.cb_rich_atoms[3])
                        || (*fmt == CLIP_RTF && target == self.cb_rich_atoms[4])) {
                        send = Some((target, data.clone()));
                    }
                }
                if let Some((typ, d)) = send {
                    if self.clip_send(req, prop, typ, d) {
                        answered = prop;
                    }
                }
            }
        }
        let mut ev = [0u8; 32];
        ev[0] = XE_SELECTION_NOTIFY;
        wr32(&mut ev, 4, rd32(b, 4)); // time
        wr32(&mut ev, 8, req);
        wr32(&mut ev, 12, rd32(b, 16)); // selection
        wr32(&mut ev, 16, target);
        wr32(&mut ev, 20, answered);
        self.send_event(req, 0, &ev, false);
        self.flush();
    }

    /// clip_send(requestor, property, type, data) -> false refused: store the data in the
    /// property, or start an INCR transfer for large data
    fn clip_send(&mut self, req: u32, prop: u32, typ: u32, d: Vec<u8>) -> bool {
        if d.len() <= self.incr_limit() as usize {
            self.change_property(req, prop, typ, 8, 0, &d);
            return true;
        }
        let Some(k) = self.xfers.iter().position(|x| x.req == 0) else { return false }; // too many at once
        let len = d.len() as u32;
        self.xfers[k] = Xfer { req, prop, typ, data: d, off: 0 };
        self.select_events(req, XM_PROPERTY); // PropertyNotify of the requestor
        let incr = self.atoms[A_INCR];
        self.change_prop32(req, prop, incr, &[len]); // INCR: lower bound of the size
        true
    }

    /// incr_limit -> bytes above which data goes out with INCR: jfgui_X11INCR, else what
    /// fits into one ChangeProperty (maximum request and our request buffer)
    fn incr_limit(&mut self) -> u32 {
        if self.incr_limit != 0 {
            return self.incr_limit;
        }
        self.incr_limit = self.maxreq.min(X11_OUT_CAP as u32) - 64;
        if let Some(v) = crate::env("jfgui_X11INCR") {
            let n = parse_int(v.as_bytes()).0 as u32;
            if n >= INCR_MIN && n <= self.incr_limit {
                self.incr_limit = n;
                self.incr_chunk = n >> 1;
            }
        }
        self.incr_limit
    }

    /// incr_chunk -> bytes per INCR chunk (64 KB, or jfgui_X11INCR / 2)
    fn incr_chunk(&mut self) -> u32 {
        self.incr_limit();
        if self.incr_chunk != 0 { self.incr_chunk } else { INCR_CHUNK }
    }

    /// x11_clip_init: the selection atom -- CLIPBOARD, or jfgui_X11SELECTION (tests)
    pub(super) fn clip_init(&mut self) {
        self.cb_sel = self.atoms[A_CLIPBOARD];
        for (i, name) in [b"text/html".as_slice(), b"text/rtf", b"application/x-jfgui-docx-fragment", b"text/html;charset=utf-8", b"application/rtf"].iter().enumerate() {
            let seq = self.intern_send(name, false);
            self.cb_rich_atoms[i] = self.atom_reply(seq);
        }
        if let Some(n) = crate::env("jfgui_X11SELECTION") {
            if !n.is_empty() {
                let seq = self.intern_send(n.as_bytes(), false);
                if let Some(r) = self.reply(seq) {
                    self.cb_sel = rd32(&r, 8);
                }
            }
        }
    }

    /// select_events(window, mask): our event mask on another client's window
    fn select_events(&mut self, win: u32, mask: u32) {
        let (o, _) = self.req(XR_CHANGE_WINDOW_ATTR, 16);
        wr32(&mut self.out, o, win);
        wr32(&mut self.out, o + 4, XCW_EVENT_MASK);
        wr32(&mut self.out, o + 8, mask);
    }

    /// xfer_end(k): free a transfer record; stop watching the requestor unless another
    /// transfer goes there
    fn xfer_end(&mut self, k: usize) {
        let req = self.xfers[k].req;
        self.xfers[k].req = 0;
        self.xfers[k].data = Vec::new();
        if self.xfers.iter().any(|x| x.req == req) {
            return;
        }
        self.select_events(req, 0);
    }

    /// x11_clip_property(ev): PropertyNotify(window, atom, time, state) of our paste property
    /// (NewValue during an INCR receive) or of an INCR requestor (Deleted: send the next
    /// chunk; an empty chunk ends the transfer)
    pub(super) fn clip_property(&mut self, b: &[u8]) {
        let (win, atom, state) = (rd32(b, 4), rd32(b, 8), b[16]);
        if win == self.xw[0].win {
            if atom == self.atoms[A_jfgui_SEL] && state == 0 {
                self.cb_newval = true;
            }
            return;
        }
        if state != 1 {
            return;
        }
        let Some(k) = self.xfers.iter().position(|x| x.req == win && x.prop == atom) else { return };
        let left = self.xfers[k].data.len() - self.xfers[k].off;
        let n = left.min(self.incr_chunk() as usize);
        let (req, prop, typ, off) = (self.xfers[k].req, self.xfers[k].prop, self.xfers[k].typ, self.xfers[k].off);
        let chunk = self.xfers[k].data[off..off + n].to_vec();
        self.change_property(req, prop, typ, 8, 0, &chunk);
        self.xfers[k].off += n;
        if n == 0 {
            self.xfer_end(k); // the empty chunk went out: done
        }
        self.flush();
    }

    // ------------------------------------------------------------------ paste side

    /// x11_clip_notify(ev): SelectionNotify(time, requestor, selection, target, property)
    pub(super) fn clip_notify(&mut self, b: &[u8]) {
        if rd32(b, 8) != self.xw[0].win {
            return;
        }
        if rd32(b, 12) != self.cb_sel {
            self.dnd_notify(b); // XdndSelection
            return;
        }
        self.cb_prop = rd32(b, 20);
        self.cb_got = true;
    }

    /// x11_plat_clip_get(fmt) -> the data, None when there is none
    pub(super) fn clip_get(&mut self, fmt: u32) -> Option<Vec<u8>> {
        if self.cb_own {
            // CLIP_PNG / CLIP_IMAGE share the PNG
            return if own_has(self.cb_fmt, fmt) { self.cb_data.clone() }
                else { self.cb_rich.iter().find(|e| e.0 == fmt).map(|e| e.1.clone()) };
        }
        if self.xw[0].win == 0 {
            return None;
        }
        match fmt {
            CLIP_HTML | CLIP_RTF | CLIP_DOCX => {
                let atom = self.cb_rich_atoms[(fmt - CLIP_HTML) as usize];
                if atom == 0 { return None; }
                match self.clip_convert(atom) {
                    Ok(data) => Some(data),
                    Err(-2) => None,
                    Err(_) => {
                        let alternate = match fmt { CLIP_HTML => self.cb_rich_atoms[3], CLIP_RTF => self.cb_rich_atoms[4], _ => 0 };
                        if alternate == 0 { None } else { self.clip_convert(alternate).ok() }
                    }
                }
            }
            CLIP_IMAGE => self.get_image(),
            CLIP_PNG => self.clip_convert(self.atoms[A_IMAGE_PNG]).ok(),
            CLIP_TEXT => {
                match self.clip_convert(self.atoms[A_UTF8_STRING]) {
                    Ok(d) => return Some(d),
                    Err(-2) => return None, // timeout: do not wait again
                    Err(_) => {}
                }
                match self.clip_convert(self.atoms[A_TEXT_PLAIN_UTF8]) {
                    Ok(d) => return Some(d),
                    Err(-2) => return None,
                    Err(_) => {}
                }
                self.clip_convert(XA_STRING).ok().map(|d| from_latin1(&d)) // Latin-1 -> UTF-8
            }
            _ => None,
        }
    }

    /// clip_convert(target) -> the data; Err(-1) refused / Err(-2) timeout
    fn clip_convert(&mut self, target: u32) -> Result<Vec<u8>, i32> {
        let limit = if self.cb_rich_atoms.contains(&target) { crate::CLIP_RICH_MAX } else { CLIP_MAX };
        self.rb = Vec::new();
        let (win0, sel, prop, t) = (self.xw[0].win, self.cb_sel, self.atoms[A_jfgui_SEL], self.time);
        let (o, _) = self.req(XR_DELETE_PROPERTY, 12);
        wr32(&mut self.out, o, win0);
        wr32(&mut self.out, o + 4, prop);
        self.cb_got = false;
        self.cb_newval = false;
        let (o, _) = self.req(XR_CONVERT_SELECTION, 24);
        let b = &mut self.out;
        wr32(b, o, win0);
        wr32(b, o + 4, sel);
        wr32(b, o + 8, target);
        wr32(b, o + 12, prop);
        wr32(b, o + 16, t);
        if !self.clip_wait(|s| s.cb_got) {
            return self.convert_timeout();
        }
        if self.cb_prop == 0 {
            return Err(-1);
        }
        let typ = self.prop_read(limit);
        if typ == 0 || (typ == u32::MAX && limit == crate::CLIP_RICH_MAX) {
            self.rb = Vec::new();
            return Err(-1);
        }
        // (an error of the first read keeps what was read, as the assembly)
        if typ == self.atoms[A_INCR] {
            // INCR: the owner writes chunks after each delete; an empty one ends it
            self.rb.clear();
            loop {
                if !self.clip_wait(|s| s.cb_newval) {
                    return self.convert_timeout();
                }
                self.cb_newval = false;
                let old = self.rb.len();
                let t = self.prop_read(limit);
                if t == u32::MAX {
                    self.rb = Vec::new();
                    return Err(-1);
                }
                if old != self.rb.len() || t == 0 {
                    continue; // data came: wait for the next chunk (or the property is not there yet)
                }
                break;
            }
        }
        Ok(core::mem::take(&mut self.rb))
    }

    fn convert_timeout(&mut self) -> Result<Vec<u8>, i32> {
        if self.debug {
            eprint("x11: clipboard: no answer within 3 s\n");
        }
        self.rb = Vec::new();
        Err(-2)
    }

    /// prop_read -> property type (0 = none, u32::MAX = error): append the whole paste property
    /// to rb (reads delete it at the end)
    fn prop_read(&mut self, limit: usize) -> u32 {
        let mut off = 0u32; // offset in 4-byte units
        loop {
            let (win0, prop) = (self.xw[0].win, self.atoms[A_jfgui_SEL]);
            let Some(r) = self.get_property(win0, prop, 0, off, X11_PROP_CHUNK, true) else { return u32::MAX };
            let typ = rd32(&r, 8);
            if typ == 0 {
                return 0; // no such property
            }
            let bytes = (rd32(&r, 16) as usize).wrapping_mul((r[1] >> 3) as usize) as u32 as usize;
            let after = rd32(&r, 12);
            if self.rb.len() + bytes + usize::from(limit == CLIP_MAX) > limit {
                return u32::MAX; // other programs' data is bounded
            }
            self.rb.extend_from_slice(&r[32..32 + bytes.min(r.len() - 32)]);
            off = off.wrapping_add(bytes as u32 >> 2);
            if after == 0 {
                return typ;
            }
        }
    }

    /// clip_wait(flag) -> true when the flag became set, false after 3 s (clip_has: 300 ms);
    /// every other event is handled meanwhile
    fn clip_wait(&mut self, flag: fn(&X11) -> bool) -> bool {
        let now = crate::time_us();
        let deadline = now + if self.cb_wait != 0 { self.cb_wait } else { CLIP_TIMEOUT };
        loop {
            self.dispatch();
            if flag(self) {
                return true;
            }
            self.flush();
            let now = crate::time_us();
            if now >= deadline {
                return false;
            }
            let ms = ((deadline - now) / 1000) as i32 + 1;
            if self.poll(POLLIN, ms) & POLLIN != 0 {
                self.recv();
            }
        }
    }

    /// targets_mask(list) -> HB_* bits of an ATOM list
    fn targets_mask(&self, d: &[u8]) -> u32 {
        let mut m = 0;
        for c in d.chunks_exact(4) {
            let a = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            if a == 0 {
                continue;
            }
            if a == self.atoms[A_IMAGE_PNG] {
                m |= HB_PNG;
            } else if a == self.xd_bmp_atom {
                m |= HB_BMP;
            } else if a == self.atoms[A_UTF8_STRING]
                || a == self.atoms[A_TEXT_PLAIN_UTF8]
                || a == self.atoms[A_TEXT_PLAIN]
                || a == self.atoms[A_TEXT]
                || a == XA_STRING
            {
                m |= HB_TEXT;
            }
            for (i, &atom) in self.cb_rich_atoms.iter().enumerate() {
                if atom != 0 && a == atom { m |= match i { 3 => 8, 4 => 16, _ => 8 << i }; }
            }
        }
        m
    }

    /// get_image: CLIP_IMAGE of another client -- TARGETS, then image/png, else image/bmp
    /// (clip_bmp_fix)
    fn get_image(&mut self) -> Option<Vec<u8>> {
        let mut mask = HB_PNG | HB_BMP; // unknown: try both
        match self.clip_convert(self.atoms[A_TARGETS]) {
            Ok(list) => mask = self.targets_mask(&list),
            Err(-2) => return None, // the owner does not answer
            Err(_) => {}
        }
        if mask & HB_PNG != 0 {
            match self.clip_convert(self.atoms[A_IMAGE_PNG]) {
                Ok(d) => return Some(d),
                Err(-2) => return None,
                Err(_) => {}
            }
        }
        if mask & HB_BMP == 0 || self.xd_bmp_atom == 0 {
            return None;
        }
        let d = self.clip_convert(self.xd_bmp_atom).ok()?;
        crate::clip_bmp_fix(&d)
    }

    /// x11_plat_clip_has(fmt) -> the clipboard offers fmt
    pub(super) fn clip_has(&mut self, fmt: u32) -> bool {
        if self.cb_own {
            return own_has(self.cb_fmt, fmt) || self.cb_rich.iter().any(|e| e.0 == fmt);
        }
        if self.xw[0].win == 0 {
            return false;
        }
        let now = crate::time_us();
        if now.wrapping_sub(self.hs_time) >= HAS_POLL {
            self.hs_time = now;
            let sel = self.cb_sel;
            let (o, seq) = self.req(XR_GET_SELECTION_OWNER, 8);
            wr32(&mut self.out, o, sel);
            let owner = self.reply(seq).map_or(0, |r| rd32(&r, 8));
            if owner == 0 {
                self.hs_owner = 0;
                self.hs_mask = 0;
            } else if owner != self.hs_owner || now.wrapping_sub(self.hs_full) >= HAS_FRESH {
                self.hs_owner = owner;
                self.hs_full = now;
                self.hs_mask = 0;
                self.cb_wait = HAS_TIMEOUT;
                let r = self.clip_convert(self.atoms[A_TARGETS]);
                self.cb_wait = 0;
                match r {
                    Ok(list) => self.hs_mask = self.targets_mask(&list),
                    // a silent owner: ask again in 30 s (as the assembly: the unsigned age
                    // test above asks again at once while hs_full lies in the future)
                    Err(-2) => self.hs_full = self.hs_full.wrapping_add(30000000),
                    Err(_) => {}
                }
            }
        }
        let bits = match fmt {
            CLIP_TEXT => HB_TEXT,
            CLIP_PNG => HB_PNG,
            CLIP_IMAGE => HB_PNG | HB_BMP,
            CLIP_HTML | CLIP_RTF | CLIP_DOCX => 8 << (fmt - CLIP_HTML),
            _ => 0,
        };
        self.hs_mask & bits != 0
    }

    // ------------------------------------------------------------------ XDND (K1)

    /// x11_dnd_window(win): XdndAware on a new window (interning the atoms once)
    pub(super) fn dnd_window(&mut self, win: u32) {
        if !self.xd_init {
            self.xd_init = true;
            let mut seqs = [0u32; D_COUNT];
            for (i, n) in XD_NAMES.iter().enumerate() {
                seqs[i] = self.intern_send(n.as_bytes(), false);
            }
            for i in 0..D_COUNT {
                self.xd_atoms[i] = self.atom_reply(seqs[i]);
            }
            self.xd_bmp_atom = self.xd_atoms[D_BMP];
        }
        let aware = self.xd_atoms[D_AWARE];
        if aware != 0 {
            self.change_prop32(win, aware, XA_ATOM, &[XDND_VERSION]);
        }
    }

    /// xd_send(type, l1, l2, l4): ClientMessage `type` to the source (l0 = our window)
    fn xd_send(&mut self, typ: u32, l1: u32, l2: u32, l4: u32) {
        let mut ev = [0u8; 32];
        ev[0] = XE_CLIENT_MESSAGE;
        ev[1] = 32;
        let src = self.xd_src;
        wr32(&mut ev, 4, src); // window = the source
        wr32(&mut ev, 8, typ);
        wr32(&mut ev, 12, self.xw[(self.xd_win as u32 as usize) & (PW_MAX - 1)].win); // l0: our window
        wr32(&mut ev, 16, l1);
        wr32(&mut ev, 20, l2);
        wr32(&mut ev, 28, l4);
        self.send_event(src, 0, &ev, false);
        self.flush();
    }

    /// xd_reset: no drag in progress
    fn xd_reset(&mut self) {
        self.xd_src = 0;
        self.xd_accept = false;
        self.xd_wait = false;
        self.xd_ready = false;
    }

    /// xd_finish(ok): XdndFinished (version 5: success flag in l1, action in l2), then reset
    fn xd_finish(&mut self, ok: bool) {
        let (l1, l2) = if ok { (1, self.xd_atoms[D_COPY]) } else { (0, 0) };
        let f = self.xd_atoms[D_FINISHED];
        self.xd_send(f, l1, l2, 0);
        self.xd_reset();
    }

    /// x11_dnd_client(ev, index) -> true when it was an XDND message
    pub(super) fn dnd_client(&mut self, b: &[u8], idx: i32) -> bool {
        let typ = rd32(b, 8);
        if typ == 0 || b[1] != 32 {
            return false;
        }
        let a = self.xd_atoms;
        if typ == a[D_ENTER] {
            if self.xd_wait {
                return true; // still reading the last drop
            }
            self.xd_reset();
            if rd32(b, 16) >> 24 < 3 {
                return true; // the source's version
            }
            self.xd_src = rd32(b, 12);
            self.xd_win = idx;
            let uri = a[D_URI];
            // more than three types: try it at the drop
            self.xd_accept = rd32(b, 16) & 1 != 0 || rd32(b, 20) == uri || rd32(b, 24) == uri || rd32(b, 28) == uri;
        } else if typ == a[D_POSITION] {
            let src = rd32(b, 12);
            if src == 0 || src != self.xd_src || self.xd_wait {
                return true;
            }
            self.xd_win = idx;
            // root position -> window pixels (client origin on the root, when known)
            let r = self.xw[(idx as u32 as usize) & (PW_MAX - 1)];
            let (mut x, mut y) = (rds16(b, 22), rds16(b, 20));
            if r.posok {
                x -= r.rx;
                y -= r.ry;
            }
            self.xd_x = x;
            self.xd_y = y;
            let (l1, l4) = if self.xd_accept { (1, a[D_COPY]) } else { (0, 0) }; // l2, l3: empty rectangle
            self.xd_send(a[D_STATUS], l1, 0, l4);
        } else if typ == a[D_LEAVE] {
            if rd32(b, 12) == self.xd_src && !self.xd_wait {
                self.xd_reset();
            }
        } else if typ == a[D_DROP] {
            let src = rd32(b, 12);
            if src == 0 || src != self.xd_src || self.xd_wait {
                return true;
            }
            if !self.xd_accept {
                self.xd_finish(false);
                return true;
            }
            let win0 = self.xw[0].win;
            let (o, _) = self.req(XR_DELETE_PROPERTY, 12);
            wr32(&mut self.out, o, win0);
            wr32(&mut self.out, o + 4, a[D_PROP]);
            let (o, _) = self.req(XR_CONVERT_SELECTION, 24);
            let out = &mut self.out;
            wr32(out, o, win0);
            wr32(out, o + 4, a[D_SELECTION]);
            wr32(out, o + 8, a[D_URI]);
            wr32(out, o + 12, a[D_PROP]);
            wr32(out, o + 16, rd32(b, 20)); // the drop's timestamp
            self.flush();
            self.xd_wait = true;
            self.xd_ready = false;
            self.xd_deadline = crate::time_us() + DND_TIMEOUT;
        } else {
            return false;
        }
        true
    }

    /// x11_dnd_notify(ev): SelectionNotify(time, requestor, selection, target, property)
    fn dnd_notify(&mut self, b: &[u8]) {
        if !self.xd_wait || rd32(b, 8) != self.xw[0].win {
            return;
        }
        let sel = rd32(b, 12);
        if sel == 0 || sel != self.xd_atoms[D_SELECTION] {
            return;
        }
        self.xd_prop = rd32(b, 20);
        self.xd_ready = true;
    }

    /// x11_dnd_deferred: the drop's data arrived (or the wait timed out)
    pub(super) fn dnd_deferred(&mut self) {
        if !self.xd_wait {
            return;
        }
        if !self.xd_ready {
            if crate::time_us() >= self.xd_deadline {
                self.xd_finish(false); // no answer: failed
            }
            return;
        }
        let mut files = 0u32;
        if self.xd_prop != 0 {
            let (win0, prop) = (self.xw[0].win, self.xd_atoms[D_PROP]);
            if let Some(r) = self.get_property(win0, prop, 0, 0, DND_MAX / 4, true) {
                let n = rd32(&r, 16);
                // text/uri-list is 8-bit data (INCR is 32)
                if rd32(&r, 8) != 0 && r[1] == 8 && n <= DND_MAX {
                    crate::drop_reset();
                    files = crate::drop_uri_list(&r[32..32 + (n as usize).min(r.len() - 32)]);
                    if files != 0 {
                        self.evwin = self.xd_win;
                        let (x, y) = (self.xd_x, self.xd_y);
                        self.push(E_DROP, 0, x, y, files);
                    }
                }
            }
        }
        self.xd_finish(files != 0);
    }
}
