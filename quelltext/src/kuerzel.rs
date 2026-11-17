//! Tastenkürzel für die Aufnahme in Hyprland eintragen, anzeigen und wieder entfernen.
//! Geändert wird nur auf ausdrücklichen Aufruf (`ws-video --kuerzel-einrichten`), vorher gibt es eine Sicherungskopie.
//!
//! Zwei Schreibweisen: die ältere Textform (`bindings.conf`, `hyprland.conf`) und die Lua-Dateien neuerer
//! Hyprland-Versionen, wie Omarchy sie anlegt (`hyprland.lua` lädt `bindings.lua`, Tasten über `o.bind`).
use crate::recorder::hypr;
use std::path::{Path, PathBuf};

const TAG: &str = "WS.Video: Aufnahme steuern";
const SUPER_ALT: u64 = 64 | 8;
const PAIRS: [(&str, &str); 3] = [("R", "P"), ("F9", "F10"), ("F11", "F12")];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Style { Conf, Lua }

struct Target { file: PathBuf, style: Style, dir: PathBuf }

/// Aufruf für die Tastenzeile. Pfade mit Leerzeichen stehen in einfachen Anführungszeichen, weil Hyprland die Zeile einer Shell übergibt.
fn exe() -> String {
    let e = crate::selbst::exe();
    if e.contains(char::is_whitespace) { format!("'{e}'") } else { e }
}

fn hypr_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")).join("hypr")
}

/// Datei, in die die Tasten gehören. Gibt es `hyprland.lua`, liest Hyprland nur noch die Lua-Dateien.
fn target() -> Result<Target, String> {
    let dir = hypr_dir();
    let main = dir.join("hyprland.lua");
    if main.is_file() {
        let text = std::fs::read_to_string(&main).unwrap_or_default();
        // `o.bind` stellt Omarchy bereit. Ohne Omarchy ist die Schreibweise eine andere, die ich nicht rate.
        if !text.to_lowercase().contains("omarchy") {
            return Err(format!("Deine Hyprland-Einstellungen sind Lua-Dateien ohne Omarchy ({}). Für diese Schreibweise kann ich die Tasten nicht selbst eintragen. Es wurde nichts geändert.", main.display()));
        }
        let own = dir.join("bindings.lua");
        let file = if own.is_file() && text.contains("hypr.bindings") { own } else { main };
        return Ok(Target { file, style: Style::Lua, dir });
    }
    match ["bindings.conf", "hyprland.conf"].iter().map(|f| dir.join(f)).find(|p| p.is_file()) {
        Some(file) => Ok(Target { file, style: Style::Conf, dir }),
        None => Err(format!("In {} finde ich weder hyprland.lua noch bindings.conf oder hyprland.conf. Es wurde nichts geändert.", dir.display())),
    }
}

/// Tastenfolge vereinheitlichen: "SUPER + ALT + R" und "ALT+super+r" werden gleich.
pub fn norm(combo: &str) -> String {
    let mut parts: Vec<String> = combo.split('+').map(|p| p.trim().to_uppercase()).filter(|p| !p.is_empty()).collect();
    parts.sort();
    parts.join("+")
}

/// Tastenfolgen, die in einem Lua-Text mit `bind("…")` belegt werden (Kommentarzeilen zählen nicht).
pub fn lua_bound(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for l in text.lines() {
        let l = l.trim_start();
        if l.starts_with("--") { continue; }
        let mut rest = l;
        while let Some(i) = rest.find("bind(") {
            rest = &rest[i + 5..];
            let t = rest.trim_start();
            if let Some(t) = t.strip_prefix('"') { if let Some(end) = t.find('"') { out.push(norm(&t[..end])); } }
        }
    }
    out
}

fn lua_files(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() { if depth > 0 { lua_files(&p, depth - 1, out); } }
        else if p.extension().map(|x| x == "lua").unwrap_or(false) { out.push(p); }
    }
}

/// Belegte Tastenfolgen aus den Lua-Dateien von Omarchy und aus den eigenen.
fn named_bindings(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    lua_files(dir, 0, &mut files);
    let omarchy = std::env::var("OMARCHY_PATH").unwrap_or_else(|_| "/usr/share/omarchy".into());
    lua_files(&PathBuf::from(omarchy).join("default/hypr"), 3, &mut files);
    files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).flat_map(|t| lua_bound(&t)).collect()
}

/// Erstes Tastenpaar (Start und Stopp, Pause), das mit SUPER + ALT noch frei ist.
/// `taken` kommt von Hyprland selbst, `named` aus den Lua-Dateien.
pub fn free_keys(taken: &[(u64, String)], named: &[String]) -> Option<(&'static str, &'static str)> {
    let free = |k: &str| !taken.iter().any(|(m, key)| *m == SUPER_ALT && key.eq_ignore_ascii_case(k)) && !named.contains(&norm(&format!("SUPER+ALT+{k}")));
    PAIRS.into_iter().find(|(a, b)| free(a) && free(b))
}

fn quote(s: &str) -> String { s.replace('\\', "\\\\").replace('"', "\\\"") }

pub fn lines(style: Style, start: &str, pause: &str, exe: &str) -> String {
    match style {
        Style::Conf => format!("# {TAG} (eingetragen von „ws-video --kuerzel-einrichten“)\nbindd = SUPER ALT, {start}, WS.Video Aufnahme starten oder beenden, exec, {exe} --toggle\nbindd = SUPER ALT, {pause}, WS.Video Aufnahme pausieren oder fortsetzen, exec, {exe} --pause\n"),
        Style::Lua => { let e = quote(exe); format!("-- {TAG} (eingetragen von „ws-video --kuerzel-einrichten“)\no.bind(\"SUPER + ALT + {start}\", \"WS.Video Aufnahme starten oder beenden\", \"{e} --toggle\")\no.bind(\"SUPER + ALT + {pause}\", \"WS.Video Aufnahme pausieren oder fortsetzen\", \"{e} --pause\")\n") }
    }
}

/// Text ohne den Block, den `lines` geschrieben hat (beide Schreibweisen).
pub fn without_block(text: &str) -> (String, bool) {
    let mut out = Vec::new();
    let mut found = false;
    let mut skip = 0;
    for l in text.lines() {
        let t = l.trim_start();
        if (t.starts_with('#') || t.starts_with("--")) && t.contains(TAG) { found = true; skip = 2; continue; }
        let low = t.to_lowercase();
        if skip > 0 && (t.starts_with("bind") || t.starts_with("o.bind")) && (low.contains("ws-video") || low.contains("ws.video")) { skip -= 1; continue; }
        skip = 0;
        out.push(l);
    }
    while found && out.last().map(|l| l.trim().is_empty()).unwrap_or(false) { out.pop(); }
    let mut s = out.join("\n");
    if !s.is_empty() { s.push('\n'); }
    (s, found)
}

fn ours(text: &str) -> Vec<&str> {
    text.lines().filter(|l| { let t = l.trim_start(); (t.starts_with("bind") || t.starts_with("o.bind")) && t.contains("ws-video --") }).collect()
}

/// Unser Block in Textdateien, die Hyprland bei Lua-Einstellungen gar nicht mehr liest (von einer früheren
/// Version dieses Befehls): wieder herausnehmen. Liefert die bereinigten Dateien.
fn clean_stale(dir: &Path) -> Vec<PathBuf> {
    let mut done = Vec::new();
    for f in ["bindings.conf", "hyprland.conf"].iter().map(|f| dir.join(f)) {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let (new, found) = without_block(&text);
        if found && std::fs::write(&f, new).is_ok() { done.push(f); }
    }
    done
}

/// Nur zeigen, was eingetragen würde.
pub fn show() {
    let (style, file, named) = match target() {
        Ok(t) => { let n = if t.style == Style::Lua { named_bindings(&t.dir) } else { vec![] }; (t.style, t.file.display().to_string(), n) }
        Err(_) => (Style::Conf, "~/.config/hypr/bindings.conf".to_string(), vec![]),
    };
    let (a, b) = free_keys(&hypr::binds(), &named).unwrap_or(PAIRS[0]);
    println!("Diese Zeilen gehören in {file}:\n");
    print!("{}", lines(style, a, b, &exe()));
    println!("\nSUPER + ALT + {a} startet die Aufnahme mit den letzten Einstellungen und beendet sie wieder.\nSUPER + ALT + {b} pausiert sie und setzt sie fort.\n„ws-video --kuerzel-einrichten“ trägt die Zeilen für dich ein.");
}

pub fn setup() -> i32 {
    if !hypr::available() {
        println!("Hyprland ist von hier aus nicht erreichbar. Bitte den Befehl in einem Terminal auf deinem Rechner ausführen, während Hyprland läuft.\n");
        show();
        return 1;
    }
    let t = match target() { Ok(t) => t, Err(e) => { println!("{e}\n"); return 1; } };
    let file = &t.file;
    let text = match std::fs::read_to_string(file) { Ok(x) => x, Err(e) => { println!("{} lässt sich nicht lesen: {e}", file.display()); return 1; } };
    let stale = if t.style == Style::Lua { clean_stale(&t.dir) } else { vec![] };
    for f in &stale { println!("Aufgeräumt: Den früheren Eintrag in {} habe ich entfernt. Diese Datei liest dein Hyprland nicht mehr.\n", f.display()); }
    if text.contains(TAG) || !ours(&text).is_empty() {
        println!("Die Tastenkürzel stehen schon in {}. Es wurde nichts geändert.\n", file.display());
        for l in ours(&text) { println!("  {l}"); }
        println!("\nMit „ws-video --kuerzel-entfernen“ nimmst du sie wieder heraus.");
        return 0;
    }
    let named = if t.style == Style::Lua { named_bindings(&t.dir) } else { vec![] };
    let Some((a, b)) = free_keys(&hypr::binds(), &named) else {
        println!("SUPER + ALT + R, P, F9 bis F12 sind bei dir alle schon belegt. Es wurde nichts geändert.\nTrage die Zeilen bitte mit eigenen Tasten von Hand in {} ein:\n", file.display());
        print!("{}", lines(t.style, "R", "P", &exe()));
        return 1;
    };
    let backup = PathBuf::from(format!("{}.vor-ws-video", file.display()));
    if let Err(e) = std::fs::copy(file, &backup) { println!("Sicherungskopie {} ließ sich nicht anlegen ({e}). Es wurde nichts geändert.", backup.display()); return 1; }
    let mut new = text.clone();
    if !new.is_empty() && !new.ends_with('\n') { new.push('\n'); }
    new.push('\n');
    new.push_str(&lines(t.style, a, b, &exe()));
    if let Err(e) = std::fs::write(file, new) { println!("{} ließ sich nicht schreiben: {e}", file.display()); return 1; }
    let _ = hypr::request("reload");
    std::thread::sleep(std::time::Duration::from_millis(800));
    let now = hypr::binds();
    let ok = [a, b].iter().all(|k| now.iter().any(|(m, key)| *m == SUPER_ALT && key.eq_ignore_ascii_case(k)));
    println!("Eingetragen in {}\n(Sicherungskopie der alten Datei: {})\n", file.display(), backup.display());
    println!("SUPER + ALT + {a}   Aufnahme starten mit den letzten Einstellungen, noch einmal drücken beendet sie");
    println!("SUPER + ALT + {b}   Aufnahme pausieren und fortsetzen\n");
    if ok { println!("Hyprland kennt die beiden Tasten jetzt. Sie gelten auf jeder Arbeitsumgebung und auf jedem Monitor."); }
    else { println!("Hyprland meldet die Tasten noch nicht. Probier sie bitte einmal aus. Tut sich nichts oder zeigt Hyprland oben am Bildschirm einen Fehler, nimmt „ws-video --kuerzel-entfernen“ die Zeilen wieder heraus."); }
    0
}

pub fn remove() -> i32 {
    let dir = hypr_dir();
    let mut files: Vec<PathBuf> = ["bindings.lua", "hyprland.lua", "bindings.conf", "hyprland.conf"].iter().map(|f| dir.join(f)).collect();
    files.retain(|f| f.is_file());
    if files.is_empty() { println!("Keine Hyprland-Einstellungen gefunden. Es wurde nichts geändert."); return 1; }
    let mut any = false;
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let (new, found) = without_block(&text);
        if !found { continue; }
        if let Err(e) = std::fs::write(&file, new) { println!("{} ließ sich nicht schreiben: {e}", file.display()); return 1; }
        println!("Die Tastenkürzel von WS.Video sind aus {} entfernt.", file.display());
        any = true;
    }
    if !any { println!("In deinen Hyprland-Einstellungen stehen keine Tastenkürzel von WS.Video. Es wurde nichts geändert."); return 0; }
    let _ = hypr::request("reload");
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_and_block() {
        assert_eq!(free_keys(&[], &[]), Some(("R", "P")));
        assert_eq!(free_keys(&[(72, "P".into())], &[]), Some(("F9", "F10")));
        assert_eq!(free_keys(&[(64, "R".into())], &[]), Some(("R", "P")));
        assert_eq!(free_keys(&[], &[norm("ALT + SUPER + r")]), Some(("F9", "F10")));
        let before = "bind = SUPER, Q, killactive\n";
        let with = format!("{before}\n{}", lines(Style::Conf, "R", "P", "/home/a/.local/bin/ws-video"));
        assert!(with.contains("exec, /home/a/.local/bin/ws-video --toggle"));
        let (back, found) = without_block(&with);
        assert!(found);
        assert_eq!(back, before);
        assert!(!without_block(before).1);
    }

    #[test]
    fn lua_style() {
        let before = "-- eigene Tasten\n-- o.bind(\"SUPER + SHIFT + R\", \"SSH\", \"alacritty\")\no.bind(\"SUPER + D\", \"OCR\", \"/home/a/.local/bin/ocr\")\nhl.unbind(\"SUPER + L\")\no.bind( \"SUPER + L\", \"Sperren\", \"lock\")\n";
        assert_eq!(lua_bound(before), vec![norm("SUPER+D"), norm("SUPER+L"), norm("SUPER+L")]);
        let block = lines(Style::Lua, "R", "P", "/home/a/.local/bin/ws-video");
        assert!(block.contains("o.bind(\"SUPER + ALT + R\", \"WS.Video Aufnahme starten oder beenden\", \"/home/a/.local/bin/ws-video --toggle\")"));
        let with = format!("{before}\n{block}");
        assert_eq!(ours(&with).len(), 2);
        let (back, found) = without_block(&with);
        assert!(found);
        assert_eq!(back, before);
        assert_eq!(lua_bound(&with).iter().filter(|k| **k == norm("SUPER + ALT + R")).count(), 1);
    }
}
