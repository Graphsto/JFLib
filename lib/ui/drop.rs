// drop -- (K1) dropped files for apps, the clipboard format probe, the headless image
// clipboard                                                           [src/ui/ui_drop.s]
//
//   drop_count() -> n               files of the last drop (UIC_DROP, arg = window id)
//   drop_path(i) -> Option<&str>    path i; valid until the next drop arrives
//   drop_list() -> &[String]        every path of the last drop
//   drop_pos() -> (x, y)            drop position in DIP of the window it happened in
//   drop_window() -> id             that window's app id (0 = main window)
//   clip_has(fmt) -> bool           the clipboard offers CLIP_TEXT / CLIP_PNG / CLIP_IMAGE
//                                   (headless runs: the in-process clipboard)
// Internal:
//   drop_accept(ev)                 E_DROP: take the platform's pending list (runtime.rs)
//   drop_inject(list)               script "drop": the list of the next E_DROP
//   hclip_img_set(fmt, data)        headless CLIP_PNG (bytes) / CLIP_IMAGE (a CLIPIMG
//                                   record: png then bmp, see clipimg.rs): PNG + BMP copies
//   hclip_img_get(fmt)              CLIP_PNG: the PNG; CLIP_IMAGE: the PNG, else the BMP
//   hclip_img_clear()   hclip_img_has(fmt)

use alloc::string::String;
use alloc::vec::Vec;

use super::consts::*;
use super::Ui;
use plat::Event;

/// drop and headless image clipboard state
#[derive(Default)]
pub(crate) struct S {
    pub(crate) list: Vec<String>,
    pub(crate) inj: Option<Vec<String>>,
    pub(crate) hi_png: Option<Vec<u8>>,
    pub(crate) hi_bmp: Option<Vec<u8>>,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) win: u32,
}

impl Ui {
    /// ui_drop_count: files of the last drop
    pub fn drop_count(&self) -> u32 {
        self.dr.list.len() as u32
    }

    /// ui_drop_path(i): path i of the last drop (None when out of range)
    pub fn drop_path(&self, i: u32) -> Option<&str> {
        self.dr.list.get(i as usize).map(|s| s.as_str())
    }

    /// ui_drop_list: every path of the last drop
    pub fn drop_list(&self) -> &[String] {
        &self.dr.list
    }

    /// ui_drop_pos: drop position in DIP of the window it happened in
    pub fn drop_pos(&self) -> (i32, i32) {
        (self.dr.x, self.dr.y)
    }

    /// ui_drop_window: the app id of the window of the last drop (0 = main window)
    pub fn drop_window(&self) -> u32 {
        self.dr.win
    }

    // drop_install(list, win, x_px, y_px): the new current list
    fn drop_install(&mut self, list: Vec<String>, win: u32, x: i32, y: i32) {
        self.dr.list = list;
        self.dr.win = self.uiw_rec_id(win);
        self.uiw_use(win); // DIP in that window
        self.dr.x = self.px2dip(x);
        self.dr.y = self.px2dip(y);
        self.uiw_use(0);
    }

    /// ui_drop_accept(ev): E_DROP of window ev.win (UI index): the platform list becomes current
    pub(crate) fn drop_accept(&mut self, ev: &Event) {
        let list = if self.headless {
            self.dr.inj.take().unwrap_or_default() // headless: the list of the script's "drop"
        } else {
            plat::drop_take()
        };
        super::perm::grant_all(&list); // the user dropped them (fs.user)
        self.drop_install(list, ev.win as u32, ev.x, ev.y);
    }

    /// ui_drop_inject(list): (script) the next E_DROP gets list
    pub(crate) fn drop_inject(&mut self, list: Vec<String>) {
        self.dr.inj = Some(list);
    }

    /// ui_clip_has(fmt): the clipboard offers fmt (cheap: no data is transferred)
    pub fn clip_has(&self, fmt: u32) -> bool {
        if !self.headless {
            return plat::clip_has(fmt);
        }
        if fmt == CLIP_TEXT {
            return self.hclip_text_has();
        }
        if matches!(fmt, CLIP_HTML | CLIP_RTF | CLIP_DOCX) {
            return self.m.edit.hclip_rich.iter().any(|e| e.0 == fmt);
        }
        self.hclip_img_has(fmt)
    }

    /// hclip_img_clear
    pub(crate) fn hclip_img_clear(&mut self) {
        self.dr.hi_png = None;
        self.dr.hi_bmp = None;
    }

    /// hclip_img_has(fmt): a PNG is stored (CLIP_IMAGE: or a BMP)
    pub(crate) fn hclip_img_has(&self, fmt: u32) -> bool {
        self.dr.hi_png.is_some() || (fmt == CLIP_IMAGE && self.dr.hi_bmp.is_some())
    }

    /// hclip_img_set(fmt, data) -> 0 / -1: CLIP_PNG bytes, or CLIP_IMAGE: a CLIPIMG record
    /// (clipimg::clipimg_record: u64 PNG length, PNG, u64 BMP length, BMP)
    pub(crate) fn hclip_img_set(&mut self, fmt: u32, data: &[u8]) -> i32 {
        match fmt {
            CLIP_PNG => {
                self.hclip_img_set_parts(Some(data), None);
                0
            }
            CLIP_IMAGE => match super::clipimg::clipimg_parse(data) {
                Some((png, bmp)) => {
                    self.hclip_img_set_parts(png, bmp);
                    0
                }
                None => {
                    self.hclip_text_clear();
                    self.hclip_img_clear();
                    -1
                }
            },
            _ => {
                self.hclip_text_clear();
                self.hclip_img_clear();
                -1
            }
        }
    }

    /// the stored image as PNG and / or BMP copies (one content at a time)
    pub(crate) fn hclip_img_set_parts(&mut self, png: Option<&[u8]>, bmp: Option<&[u8]>) {
        self.hclip_text_clear();
        self.hclip_img_clear();
        self.dr.hi_bmp = bmp.map(|b| b.to_vec());
        self.dr.hi_png = png.map(|p| p.to_vec());
    }

    /// hclip_img_get(fmt): CLIP_PNG: the PNG; CLIP_IMAGE: the PNG, else the BMP (copies)
    pub(crate) fn hclip_img_get(&self, fmt: u32) -> Option<Vec<u8>> {
        if fmt != CLIP_PNG && fmt != CLIP_IMAGE {
            return None;
        }
        if let Some(p) = &self.dr.hi_png {
            return Some(p.clone());
        }
        if fmt == CLIP_IMAGE {
            return self.dr.hi_bmp.clone();
        }
        None
    }
}
