// theme -- colour tokens, accent colour and type ramp of the linked style  [src/ui/ui_theme.s]
//
//   theme_set(mode)            0 light, 1 dark, 2 follow the system (plat::theme)
//   theme_dark() -> bool       the dark theme is active
//   accent_set(rgb)            0xRRGGBB accent, 0 = the style's default
//   color(tok) -> argb         0xAARRGGBB of a T_* token (byte offset)
//   ts_font(ts)                select font and pixel size of a TS_* text style
//   ts_line(ts) -> px          line height of a text style
//   ts_base(ts) -> px          baseline offset from the line top
//   color_mix(a, b, t)         per-channel a + (b - a) * t / 256 (t 0..256)
//
// The values come from the style (Style::tokens, accent, ts_size / ts_line / ts_font).

use super::consts::*;
use super::Ui;

/// g_theme: the T_COUNT tokens padded to a power of two (256 bytes)
pub(crate) const THEME_N: usize = 64;

/// color_mix(a, b, t): per-channel a + (b - a) * t / 256 (t 0..256), every channel
/// including alpha
pub fn color_mix(a: u32, b: u32, t: i32) -> u32 {
    let mut r = 0u32;
    for sh in (0..32).step_by(8) {
        let x = ((a >> sh) & 255) as i32;
        let y = ((b >> sh) & 255) as i32;
        let v = x + (((y - x).wrapping_mul(t)) >> 8);
        r |= (v as u32) << sh;
    }
    r
}

impl Ui {
    /// ui_theme_set(mode): 0 light, 1 dark, 2 the system setting (unknown: dark)
    pub fn theme_set(&mut self, mode: u32) {
        let mut m = mode;
        if m == 2 {
            let t = plat::theme();
            m = if t < 0 { 1 } else { t as u32 };
        }
        let changed = self.theme_dark != (m & 1 != 0);
        self.theme_dark = m & 1 != 0;
        self.theme_build();
        if changed { self.theme_invalidate(); }
    }

    /// ui_theme_dark: the dark theme is active
    pub fn theme_dark(&self) -> bool {
        self.theme_dark
    }

    /// ui_accent_set(0xRRGGBB), 0 = the style's default accent
    pub fn accent_set(&mut self, rgb: u32) {
        self.accent = rgb & 0xFF_FFFF;
        self.theme_build();
    }

    /// ui_theme_build: copy the table for the current mode and fill in the accent tokens
    pub(crate) fn theme_build(&mut self) {
        let st = self.style;
        let mut t = st.tokens(self.theme_dark);
        st.accent(&mut t, self.theme_dark, self.accent); // the style derives the accent tokens
        let changed = self.theme[..T_COUNT] != t;
        self.theme[..T_COUNT].copy_from_slice(&t);
        if changed { self.theme_invalidate(); }
    }

    // agent_core: style icon colours may change without changing Cmd::Icon's
    // foreground. Invalidate the live window and every saved window together.
    fn theme_invalidate(&mut self) {
        self.dirty_force_full();
        for window in &mut self.m.win.recs {
            if let Some(parts) = &mut window.save { parts.dirty.full = true; }
        }
        self.need = true;
        self.need_render = true;
    }

    /// ui_color(tok): 0xAARRGGBB of a token (byte offset; any value reads inside the table)
    pub fn color(&self, tok: u32) -> u32 {
        self.theme[((tok & 252) / 4) as usize]
    }

    /// ui_ts_font(ts): select the font and pixel size (size * dpi / 96, 24.8) of a style
    pub fn ts_font(&mut self, ts: u32) {
        let ts = if (ts & 7) < TS_COUNT { ts & 7 } else { TS_BODY } as usize;
        let size = (self.style.ts_size_fx()[ts] as u32).wrapping_mul(self.wp.rt.dpi as u32) / 96;
        self.text.set_font(self.style.ts_font()[ts] as u32, size as i32);
    }

    /// ui_ts_line(ts): line height of a text style in px
    pub fn ts_line(&self, ts: u32) -> i32 {
        let ts = if (ts & 7) < TS_COUNT { ts & 7 } else { TS_BODY } as usize;
        self.dp(self.style.ts_line()[ts] as i32)
    }

    /// ui_ts_base(ts): baseline offset from the top of the line box (px; selects the font)
    pub fn ts_base(&mut self, ts: u32) -> i32 {
        self.ts_font(ts);
        let line = self.ts_line(ts);
        let asc = self.text.ascent();
        let desc = self.text.descent();
        ((line - (asc + desc)) >> 1) + asc
    }
}
