// assoc -- the app's file types and URL schemes (apps/APP/PACKAGE filetypes= / schemes=, carried
// in the payload manifest): the HKCU\Software\Classes entries the setup writes, and the list
// install.ini keeps for the uninstaller
//
//   parse_types(text) -> [FileType]   "ext[,ext..]|mime|description[|own]; ..." -- every
//                                     extension gets the app in "Open with"; "own" types also
//                                     open with it by default (their extension is the app's)
//   parse_schemes(text) -> [scheme]   "scheme scheme ..." (';' or blanks)
//   plan(app, display, exe, types, schemes) -> Plan { set: [(subkey, name, value)], remove }
//                                     remove: "T:<subkey>" (the key with everything below) and
//                                     "V:<subkey>|<name>" (one value), ';' separated
//
// Keys: Software\Classes\<app>.<ext> (the ProgID: description, DefaultIcon, shell\open\command
// = "<exe>" "%1"), Software\Classes\.<ext>\OpenWithProgids\<ProgID>, for "own" types also the
// default value and Content Type of Software\Classes\.<ext>; a scheme: Software\Classes\<scheme>
// ("URL:<display>", URL Protocol, DefaultIcon, shell\open\command).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// one file type of the manifest
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileType {
    /// lower case, without the dot
    pub(crate) exts: Vec<String>,
    pub(crate) mime: String,
    pub(crate) desc: String,
    /// the app is the default program of these extensions
    pub(crate) own: bool,
}

/// what the setup writes and what the uninstaller removes
pub(crate) struct Plan {
    pub(crate) set: Vec<(String, String, String)>,
    pub(crate) remove: String,
}

fn name_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_+.".contains(&c))
}

/// "ext[,ext..]|mime|description[|own]; ..." -> the valid entries
pub(crate) fn parse_types(text: &str) -> Vec<FileType> {
    let mut out = Vec::new();
    for entry in text.split(';') {
        let f: Vec<&str> = entry.split('|').map(str::trim).collect();
        if f.len() < 3 || f.len() > 4 || (f.len() == 4 && f[3] != "own") {
            continue;
        }
        let exts: Vec<String> = f[0].split(',').map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|e| name_ok(e)).collect();
        if exts.is_empty() || !f[1].contains('/') || f[1].contains(['\\', '"']) || f[2].contains('"') {
            continue;
        }
        out.push(FileType { exts, mime: String::from(f[1]), desc: String::from(f[2]), own: f.len() == 4 });
    }
    out
}

/// "scheme scheme; ..." -> the valid schemes (lower case)
pub(crate) fn parse_schemes(text: &str) -> Vec<String> {
    text.split([';', ' ', ',']).map(|s| s.trim().to_ascii_lowercase()).filter(|s| name_ok(s) && s.as_bytes()[0].is_ascii_alphabetic()).collect()
}

/// the registry entries of the types and schemes for the program exe
pub(crate) fn plan(app: &str, display: &str, exe: &str, types: &[FileType], schemes: &[String]) -> Plan {
    let mut set = Vec::new();
    let mut remove: Vec<String> = Vec::new();
    let mut put = |k: &str, n: &str, v: &str| set.push((String::from(k), String::from(n), String::from(v)));
    let icon = format!("{exe},0");
    let cmd = format!("\"{exe}\" \"%1\"");
    let classes = "Software\\Classes";
    for t in types {
        let progid = format!("{app}.{}", t.exts[0]);
        let key = format!("{classes}\\{progid}");
        put(&key, "", &t.desc);
        put(&format!("{key}\\DefaultIcon"), "", &icon);
        put(&format!("{key}\\shell\\open\\command"), "", &cmd);
        remove.push(format!("T:{key}"));
        for e in &t.exts {
            let ext = format!("{classes}\\.{e}");
            put(&format!("{ext}\\OpenWithProgids"), &progid, "");
            remove.push(format!("V:{ext}\\OpenWithProgids|{progid}"));
            if t.own {
                put(&ext, "", &progid);
                put(&ext, "Content Type", &t.mime);
                remove.push(format!("V:{ext}|"));
                remove.push(format!("V:{ext}|Content Type"));
            }
        }
    }
    for s in schemes {
        let key = format!("{classes}\\{s}");
        put(&key, "", &format!("URL:{display}"));
        put(&key, "URL Protocol", "");
        put(&format!("{key}\\DefaultIcon"), "", &icon);
        put(&format!("{key}\\shell\\open\\command"), "", &cmd);
        remove.push(format!("T:{key}"));
    }
    Plan { set, remove: remove.join(";") }
}
