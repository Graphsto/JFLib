// ui -- the immediate-mode runtime: one Ui context instead of ASMLIB's g_* globals  [src/ui]
//
//   run(desc, app, style) -> exit code   command line, headless (--shot / --script) or window
//   App: init(ui) / frame(ui) builds the UI every pass / command(ui, id, arg) -> handled
//   Style: how widgets look (style.rs, the contract of ASMLIB/inc/style.inc); the app
//   passes the style crate it was linked with (build_rs.py --style)
//   main!(expr): the app's entry point (jfgui_main, called by plat's process entry)
//
// Modules (one per ASMLIB file): ctx (ui_ctx.s: input, hit testing, behaviour, slots,
// timers, commands, semantics, clip, layers, passes), layout (ui_layout.s), cmd (ui_cmd.s:
// draw command lists and their replay), dirty (ui_dirty.s), theme (ui_theme.s), basic
// (ui_basic.s), auto (ui_auto.s: scripts and the widget dump), runtime (ui_main.s), cursor
// (ui_cursor.s), drop (ui_drop.s), clipimg (ui_clipimg.s), style (style.inc); the widget
// families popup, edit, list, value, color, win, chrome, filedlg (R5 agents).
//
// State: ASMLIB keeps every window's UI state in per-module blocks (`*_wb0` .. `*_wb1`)
// that ui_win.s swaps when it builds another window.  Here that state is `WinParts` (the
// live window's part of every module: `ui.wp.ctx`, `ui.wp.lay`, `ui.wp.cmd`, `ui.wp.dirty`,
// `ui.wp.rt`, `ui.wp.popup`, ...); win.rs swaps it with a window's saved WinParts
// (core::mem::swap).  State shared by all windows lives in `Ui` itself (core) and in
// `ui.m` (the widget families' shared state).
//
// For the widget families (R5): the asm globals map to fields -- g_mx / g_my / g_focus /
// g_hot / g_active / g_last_rect / g_keyq ... are `self.wp.ctx.*`, g_boxes / g_next_* are
// `self.wp.lay.*`, g_dcol / the command lists `self.wp.cmd.*`, g_fb / g_ui_dpi
// `self.wp.rt.fb` / `.dpi`; g_frame / g_now_ms / g_repass / g_mods / g_sems / slots / timers /
// g_uiw_cur are fields of Ui.  A family's per-window state goes into its `W` (in WinParts),
// its shared state into its `S` (in Mods).  Style calls: `let st = self.style; let mut sp =
// Sp { .. }; let (w, h) = st.xxx_size(self, &mut sp); self.place(w, h); let ev =
// self.behave(id, hb); self.sp_placed(&mut sp, ev); st.xxx(self, &sp);
// self.focus_ring_draw(ev); self.sem(K_*, id, label, value)` (basic.rs shows every pattern).
// The core calls into the families through these functions (all ported, R5): win.rs uiw_*,
// chrome.rs titlebar_begin / titlebar_end / edge_grips (ui-win), popup.rs accel_dispatch /
// tooltip / popup_count (ui-popup), edit.rs text_key_filter, clip_set / clip_get /
// hclip_text_* / open_folder / open_url (ui-edit).
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod consts;
pub mod keys;
mod auto;
mod agent;
mod basic;
mod chart;
mod chrome;
mod clipimg;
mod cmd;
mod color;
mod ctx;
mod cursor;
mod data;
mod datepick;
mod dirty;
mod drop;
mod edit;
mod filedlg;
pub mod i18n;
mod layout;
mod list;
mod notify;
mod office_gallery;
mod popup;
mod ribbon;
mod shell;
mod slider;
mod runtime;
mod stock;
mod style;
pub mod theme;
mod value;
mod vicon;
mod win;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use gfx::Dc;
use text::Text;

pub use clipimg::{clip_bmp_from_sf, clipimg_parse, clipimg_record};
pub use cmd::{Cmd, Draw, DrawRef, Gfx, IconHook};
pub use consts::*;
pub use ctx::{Sem, Slot};
pub use cursor::CursorDraw;
pub use edit::TextboxPresentation;
pub use edit::TextSurfaceLine;
pub use keys::*;
pub use layout::{dp_at, px2dip_at};
pub use popup::{Command, Mi}; // ui-popup: the command table and menu records
pub use ribbon::{RibbonGroup, RibbonGroupMode, RibbonLayout, RibbonMetrics, RibbonSample};
pub use runtime::{Runtime, launch, run};
pub use style::{Sp, Style, StyleBasic, StyleChrome, StyleData, StyleEdit, StyleList, StyleMisc, StylePopup, StyleShell, StyleValue};
pub use data::{GridCol, GridEv, GridState, TreeEv, TreeRow, TreeState};
pub use chart::{BarSeries, DonutSlice};
pub use shell::{CmdItem, NavState};
pub use stock::{card_opaque, kind_colors, Stock};
pub use vicon::{paint_svg_icon, paint_svg_icon2};
pub use theme::color_mix;
pub use win::Shared; // ui-win: the app's open flags / WINRECTs the library keeps
// ui-list: the value / colour / list API beside the Ui methods
pub use color::{hsv_to_rgb, rgb_to_hsv};
pub use value::{cvtd, deg_atan2, deg_sincos, grad_at, part_id, px_mix, RangeF, Track, NO_DEFAULT};

/// the app's entry point: `ui::main!(ui::run(&DESC, &mut MyApp::default(), &style::STYLE));`
/// (expands to the `jfgui_main` symbol the platform entry calls; the app crate denies
/// unsafe code, this one item is the exception)
#[macro_export]
macro_rules! main {
    ($body:expr) => {
        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        fn jfgui_main() -> i32 {
            $body
        }
    };
}

/// the app descriptor (APP_DESC): title, AF_* flags, sizes in DIP, accent 0xRRGGBB.  Write
/// `AppDesc { title: "..", w: .., ..AppDesc::NONE }` so later fields keep their defaults.
#[derive(Clone, Copy)]
pub struct AppDesc {
    pub title: &'static str,
    pub flags: u32,
    pub w: u16,
    pub h: u16,
    pub min_w: u16,
    pub min_h: u16,
    pub accent: u32,
    /// (ui-popup) the command table (AD_CMDS: ids, labels, icons, accelerators, CF_* flags)
    pub cmds: &'static [Command],
    /// (ui-popup) the menu bar of menubar(None) (AD_MENUBAR: Mi::Menu records)
    pub menubar: &'static [Mi],
}

impl AppDesc {
    /// every field empty / zero
    pub const NONE: AppDesc =
        AppDesc { title: "", flags: 0, w: 0, h: 0, min_w: 0, min_h: 0, accent: 0, cmds: &[], menubar: &[] };
}

/// the app's callbacks (APP_DESC on_init, on_frame, on_command)
pub trait App {
    /// Optional domain actions, in addition to the universal UI action catalogue.
    fn actions(&self) -> &'static [agent_core::ActionSpec] { &[] }
    /// Domain revision; None uses the UI runtime's monotonically increasing frame revision.
    fn action_revision(&self) -> Option<u64> { None }
    /// Dynamic domain constraints such as read-only files or an absent selection.
    fn action_available(&self, _spec: &agent_core::ActionSpec) -> Result<(), agent_core::ActionError> { Ok(()) }
    /// Domain action handler; adapters should call the same editing functions as the GUI.
    fn action(&mut self, _ui: &mut Ui, _spec: &agent_core::ActionSpec, _input: &agent_core::Value,
        _context: &agent_core::ActionContext) -> Result<agent_core::ActionResult, agent_core::ActionError> {
        Err(agent_core::ActionError::new(agent_core::ErrorCode::NotFound, "No domain action with this name"))
    }
    /// Filtered domain state; the default universal inspection includes the widget tree.
    fn action_inspect(&self, _query: &agent_core::Value) -> Result<agent_core::Value, agent_core::ActionError> {
        Ok(agent_core::Value::object([]))
    }
    /// Describe an application-owned binary resource returned by one of its actions.
    fn action_resource_info(&self, _uri: &str) -> Result<agent_core::Resource, agent_core::ActionError> {
        Err(agent_core::ActionError::new(agent_core::ErrorCode::NotFound, "Resource not found"))
    }
    /// Read a bounded byte range of an application-owned resource, never an arbitrary path.
    fn action_resource_read(&self, _uri: &str, _offset: u64, _limit: usize) -> Result<Vec<u8>, agent_core::ActionError> {
        Err(agent_core::ActionError::new(agent_core::ErrorCode::NotFound, "Resource not found"))
    }
    /// called before the first frame; false = initialisation failed (exit code 2)
    fn init(&mut self, _ui: &mut Ui) -> bool {
        true
    }
    /// builds the UI (every pass; no side effects except through widgets)
    fn frame(&mut self, ui: &mut Ui);
    /// commands queued with ui.cmd_run and the built-in UIC_* ids; true = handled
    fn command(&mut self, _ui: &mut Ui, _id: u32, _arg: u32) -> bool {
        false
    }
}

/// the per-window state of the live window: the core's per-window blocks (ui_ctx_wb0,
/// ui_layout_wb0, ui_cmd_wb0, ui_dirty_wb0, ui_main_wb0) and the families' (ui_popup,
/// ui_list, ui_chrome).  win.rs keeps one of these per window and swaps it with `Ui::wp`
/// when another window is built (core::mem::swap); a new window starts from `WinParts::new`.
pub struct WinParts {
    pub(crate) ctx: ctx::W,
    pub(crate) lay: layout::W,
    pub(crate) cmd: cmd::W,
    pub(crate) dirty: dirty::W,
    pub(crate) rt: runtime::W,
    #[allow(dead_code)] // R5: filled by the widget-family ports
    pub(crate) popup: popup::W,
    #[allow(dead_code)]
    pub(crate) list: list::W,
    #[allow(dead_code)]
    pub(crate) chrome: chrome::W,
}

impl WinParts {
    /// the state of a window before its first pass (the main window: ui_init)
    pub(crate) fn new() -> WinParts {
        WinParts {
            ctx: ctx::W::new(),
            lay: layout::W::default(),
            cmd: cmd::W::new(&cmd::CAPS_MAIN),
            dirty: dirty::W::default(),
            rt: runtime::W::new(),
            popup: Default::default(),
            list: Default::default(),
            chrome: Default::default(),
        }
    }
}

/// the shared state of the widget-family modules
#[derive(Default)]
#[allow(dead_code)] // R5: filled by the widget-family ports
pub(crate) struct Mods {
    pub(crate) popup: popup::S,
    pub(crate) edit: edit::S,
    pub(crate) list: list::S,
    pub(crate) value: value::S,
    pub(crate) color: color::S,
    pub(crate) win: win::S,
    pub(crate) chrome: chrome::S,
    pub(crate) filedlg: filedlg::S,
    pub(crate) chart: chart::S,
    pub(crate) shell: shell::S,
    pub(crate) notify: notify::S,
}

/// the UI context: everything ASMLIB keeps in g_* variables
pub struct Ui {
    pub(crate) agent: agent::State,
    pub desc: AppDesc,
    pub style: &'static dyn Style,
    /// fonts, glyph cache, text runs (shared: the asm's font module)
    pub text: Text,
    /// the gfx draw context (the asm's g_dc; render time only)
    pub dc: Dc,
    /// the live window's per-window state (see WinParts)
    pub(crate) wp: WinParts,
    /// the widget families' shared state
    pub(crate) m: Mods,

    // ---- ctx: shared by all windows (ui_ctx.s after ui_ctx_wb1)
    pub(crate) sems: Vec<Sem>,
    pub(crate) semtext_len: usize,
    pub(crate) sem_dummy: Sem,
    pub(crate) slots: BTreeMap<u32, Slot>,
    pub(crate) keydown: [u8; 256],
    pub(crate) mpts: Vec<[i32; 2]>,
    pub(crate) mprs: Vec<u32>,
    pub(crate) mtilt: [i32; 2],
    pub(crate) mpress: u32,
    pub(crate) mptype: u32,
    pub(crate) mpts_win: u32,
    pub(crate) cmdq: Vec<(u32, u32)>,
    pub(crate) timers: [(u32, u32); ctx::TIMERS_MAX],
    pub(crate) mods: u32,
    pub(crate) frame: u32,
    pub(crate) now_ms: u32,
    pub(crate) repass: bool,
    pub(crate) animating: bool,
    pub(crate) anim_off: bool,
    pub(crate) warns: u32,
    pub(crate) ui_debug: bool,
    /// scripts and jfgui_DEBUG: warn about an id two widgets of one pass share
    pub(crate) dup_check: bool,
    pub(crate) dup_warned: alloc::collections::BTreeSet<u32>,
    pub(crate) focus_req: u32,
    pub(crate) focus_req_win: u32,
    pub(crate) focus_req_old: u32,
    pub(crate) focus_req_frame: u32,
    /// window being built / live (g_uiw_cur of ui_win.s; 0 = the main window)
    pub(crate) uiw_cur: u32,

    // ---- theme (ui_theme.s)
    pub(crate) theme: [u32; theme::THEME_N],
    pub(crate) theme_dark: bool,
    pub(crate) accent: u32,

    // ---- runtime (ui_main.s, shared part)
    pub(crate) title: String,
    pub(crate) t0_us: u64,
    pub(crate) headless: bool,
    pub(crate) need: bool,
    pub(crate) need_render: bool,
    pub(crate) quit: bool,
    pub(crate) log_events: bool,
    pub(crate) autoclose_ms: u32,
    pub(crate) accent_raw: u32,
    pub(crate) settle_warned: bool,
    pub(crate) verify_dirty: bool,
    pub(crate) frames_rendered: u32,
    pub(crate) close_at: u32,

    // ---- dirty regions (ui_dirty.s, shared part)
    pub(crate) drects: Vec<[i32; 4]>,
    pub(crate) dmg: Vec<dirty::Dmg>,
    pub(crate) cvs: [dirty::Cvs; dirty::CVS_MAX],

    // ---- commands (ui_cmd.s, shared part)
    pub(crate) icon_hook: Option<IconHook>,

    // ---- basic widgets (ui_basic.s)
    pub(crate) uiinput: UiInput,

    // ---- scripts (ui_auto.s), cursors (ui_cursor.s), drops and the headless image
    // clipboard (ui_drop.s)
    pub(crate) sc: auto::S,
    pub(crate) ucur: cursor::S,
    pub(crate) dr: drop::S,
}
