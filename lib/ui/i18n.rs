// i18n -- the language of the library's own texts and translation catalogues for apps
//
//   set_language(LANG_*) / language()   the UI language (process wide; default German)
//   tr(de) -> text                      a text of the library itself (file dialog, command
//                                       bar, tree, toasts, shortcuts) in the current language;
//                                       the German source text is the key
//   Catalog::from_tsv(text)             an app's catalogue: a TSV file whose first line names
//                                       the languages ("de\ten\tfr"), then one row per text,
//                                       the source text first; catalog.tr(s) -> the text in
//                                       the current language (the source when missing)
//   month_name(m) / month_short(m) / weekday_short(d)   calendar names (m 1..12, d 0 = Mon)
//   set_catalog(catalog) / t(s)         the app's catalogue for the whole process: t(s) is the
//                                       text in the current language (German source when missing)
//   fill(text, args)                    "{}" placeholders replaced in order
//
// The language index is the catalogue column: LANG_DE = 0 (the source column), LANG_EN = 1.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

pub const LANG_DE: u32 = 0;
pub const LANG_EN: u32 = 1;

static LANG: AtomicU32 = AtomicU32::new(LANG_DE);

/// set the UI language of the library texts (LANG_DE, LANG_EN)
pub fn set_language(lang: u32) {
    LANG.store(lang, Ordering::Relaxed);
}

/// the current UI language (LANG_*)
pub fn language() -> u32 {
    LANG.load(Ordering::Relaxed)
}

// the library's own texts: German source -> English
static LIB_EN: &[(&str, &str)] = &[
    ("Öffnen", "Open"),
    ("Speichern", "Save"),
    ("Speichern unter", "Save as"),
    ("Abbrechen", "Cancel"),
    ("Schließen", "Close"),
    ("Dateiname:", "File name:"),
    ("Dateityp:", "File type:"),
    ("Ordner auswählen", "Select folder"),
    ("Neuer Ordner", "New folder"),
    ("Aufwärts", "Up"),
    ("Zurück", "Back"),
    ("Weiter", "Next"),
    ("Fertigstellen", "Finish"),
    ("Weitere Befehle", "More commands"),
    ("Vorherige Register", "Previous tabs"),
    ("Weitere Register", "More tabs"),
    ("Aufklappen", "Expand"),
    ("Zuklappen", "Collapse"),
    ("Navigation umschalten", "Toggle navigation"),
    ("Heute", "Today"),
    ("Vorheriger Monat", "Previous month"),
    ("Nächster Monat", "Next month"),
    ("Strg", "Ctrl"),
    ("Umschalt", "Shift"),
    ("Alt", "Alt"),
    ("Entf", "Del"),
    ("Einfg", "Ins"),
    ("Pos1", "Home"),
    ("Ende", "End"),
    ("Bild auf", "PgUp"),
    ("Bild ab", "PgDn"),
    ("Leertaste", "Space"),
    ("Eingabe", "Enter"),
    ("Rücktaste", "Backspace"),
];

/// a library text in the current language (the German source when there is no translation)
pub fn tr(de: &'static str) -> &'static str {
    if language() == LANG_DE {
        return de;
    }
    LIB_EN.iter().find(|(k, _)| *k == de).map_or(de, |(_, v)| v)
}

static MONTHS: [[&str; 12]; 2] = [
    ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
];
static MONTHS_SHORT: [[&str; 12]; 2] = [
    ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"],
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
];
static WEEKDAYS_SHORT: [[&str; 7]; 2] = [["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"], ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]];
static WEEKDAYS_ABBR: [[&str; 7]; 2] = [["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"], ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]];

fn li() -> usize {
    if language() == LANG_EN { 1 } else { 0 }
}

/// the name of month m (1..12) in the current language
pub fn month_name(m: u32) -> &'static str {
    MONTHS[li()][((m.max(1) - 1) % 12) as usize]
}

/// the short name of month m (1..12)
pub fn month_short(m: u32) -> &'static str {
    MONTHS_SHORT[li()][((m.max(1) - 1) % 12) as usize]
}

/// the two-letter name of weekday d (0 = Monday .. 6 = Sunday)
pub fn weekday_short(d: u32) -> &'static str {
    WEEKDAYS_SHORT[li()][(d % 7) as usize]
}

/// the usual abbreviation of weekday d (0 = Monday) in running text: "Sa" / "Sat"
pub fn weekday_abbr(d: u32) -> &'static str {
    WEEKDAYS_ABBR[li()][(d % 7) as usize]
}

/// a translation catalogue of an app (see the module comment)
pub struct Catalog {
    langs: Vec<String>,
    rows: BTreeMap<&'static str, Vec<&'static str>>,
}

impl Catalog {
    /// parse a TSV catalogue (first line: the language codes); "\n" and "\t" escapes in the
    /// cells are not interpreted
    pub fn from_tsv(text: &'static str) -> Catalog {
        let mut lines = text.lines();
        let langs: Vec<String> = lines.next().map(|h| h.split('\t').map(|s| String::from(s.trim())).collect()).unwrap_or_default();
        let mut rows = BTreeMap::new();
        for line in lines {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cols = line.split('\t');
            let Some(key) = cols.next() else { continue };
            let rest: Vec<&'static str> = cols.collect();
            rows.insert(key, rest);
        }
        Catalog { langs, rows }
    }

    /// the language codes of the columns ("de", "en", ...)
    pub fn languages(&self) -> &[String] {
        &self.langs
    }

    /// the text in the current language (the source text when the cell is missing or empty)
    pub fn tr<'a>(&'a self, s: &'a str) -> &'a str {
        let l = language() as usize;
        if l == 0 {
            return s;
        }
        match self.rows.get(s).and_then(|r| r.get(l - 1)) {
            Some(t) if !t.is_empty() => t,
            _ => s,
        }
    }

    /// whether the catalogue has a row for this source text
    pub fn has(&self, s: &str) -> bool {
        self.rows.contains_key(s)
    }
}

static APP: rt::sync::SpinLock<Option<&'static Catalog>> = rt::sync::SpinLock::new(None);

/// install the app's catalogue for the whole process (t() reads it)
pub fn set_catalog(c: Catalog) {
    let r: &'static Catalog = Box::leak(Box::new(c));
    *APP.lock() = Some(r);
}

/// the app's text in the current language: the installed catalogue, then the library's own
/// texts, then the German source
pub fn t(s: &'static str) -> &'static str {
    if language() == LANG_DE {
        return s;
    }
    let cat = *APP.lock();
    match cat.and_then(|c| c.get_static(s)) {
        Some(x) => x,
        None => tr(s),
    }
}

/// replace the "{}" placeholders of a text in order
pub fn fill(text: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    let mut i = 0;
    while let Some(p) = rest.find("{}") {
        out.push_str(&rest[..p]);
        out.push_str(args.get(i).copied().unwrap_or(""));
        i += 1;
        rest = &rest[p + 2..];
    }
    out.push_str(rest);
    out
}

impl Catalog {
    /// the translation of a source text in the current language, when the catalogue has one
    pub fn get_static(&self, s: &str) -> Option<&'static str> {
        let l = language() as usize;
        if l == 0 {
            return None;
        }
        self.rows.get(s).and_then(|r| r.get(l - 1)).copied().filter(|t| !t.is_empty())
    }
}
