//! Freie Schriften von Google Fonts nach Namen laden und für den Benutzer installieren.
use std::path::PathBuf;
use std::process::Command;

extern "C" {
    fn FcInitReinitialize() -> libc::c_int;
    fn FcConfigAppFontAddDir(config: *mut libc::c_void, dir: *const libc::c_char) -> libc::c_int;
    fn pango_cairo_font_map_get_default() -> *mut libc::c_void;
    fn pango_fc_font_map_config_changed(fontmap: *mut libc::c_void);
}

fn font_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/fonts/ws-video")
}

fn curl(url: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("curl").args(["-sS", "-f", "-L", "-m", "40", "-A", "curl", url]).output().map_err(|e| format!("curl fehlt oder startet nicht: {e}"))?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).trim().to_string()); }
    Ok(out.stdout)
}

/// Lädt die Schriftfamilie (normal, fett, kursiv soweit vorhanden). Läuft blockierend, also in einem eigenen Thread aufrufen.
/// Liefert die Zahl der installierten Dateien.
pub fn install(family: &str) -> Result<usize, String> {
    let family = family.trim();
    if family.is_empty() || !family.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '-') {
        return Err("Bitte nur den Namen der Schrift eingeben, zum Beispiel „Poppins“ oder „Open Sans“.".into());
    }
    let q = family.replace(' ', "+");
    let variants = [format!("{q}:ital,wght@0,400;0,700;1,400;1,700"), format!("{q}:wght@400;700"), q.clone()];
    let mut css = None;
    for v in &variants {
        if let Ok(b) = curl(&format!("https://fonts.googleapis.com/css2?family={v}")) { css = Some(String::from_utf8_lossy(&b).to_string()); break; }
    }
    let css = css.ok_or_else(|| format!("„{family}“ wurde bei Google Fonts nicht gefunden (Schreibweise prüfen) oder es besteht keine Internetverbindung."))?;
    let mut urls: Vec<String> = Vec::new();
    for part in css.split("url(").skip(1) {
        if let Some(end) = part.find(')') {
            let u = part[..end].trim_matches(|c| c == '\'' || c == '"').to_string();
            if u.starts_with("https://fonts.gstatic.com/") && !urls.contains(&u) { urls.push(u); }
        }
    }
    if urls.is_empty() { return Err("Google Fonts hat keine Schriftdateien geliefert.".into()); }
    let dir = font_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = family.replace(' ', "");
    let mut n = 0;
    for (i, u) in urls.iter().enumerate() {
        let ext = if u.ends_with(".otf") { "otf" } else { "ttf" };
        let data = curl(u)?;
        if data.len() < 1000 { continue; }
        std::fs::write(dir.join(format!("{base}-{}.{ext}", i + 1)), data).map_err(|e| e.to_string())?;
        n += 1;
    }
    let _ = Command::new("fc-cache").arg("-f").arg(&dir).status();
    if n == 0 { Err("Die Schriftdateien konnten nicht geladen werden.".into()) } else { Ok(n) }
}

/// Schriftliste des laufenden Programms neu einlesen (im Hauptthread aufrufen).
pub fn reload() {
    unsafe {
        FcInitReinitialize();
        let fm = pango_cairo_font_map_get_default();
        if !fm.is_null() { pango_fc_font_map_config_changed(fm); }
    }
}

/// Mitgelieferte Schriften (AppImage) für dieses Programm anmelden. Vor dem ersten Text aufrufen.
pub fn add_bundled() {
    let Some(dir) = std::env::var_os("WS_FONT_DIR") else { return };
    if !std::path::Path::new(&dir).is_dir() { return; }
    if let Ok(c) = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(dir.as_os_str())) {
        unsafe { FcConfigAppFontAddDir(std::ptr::null_mut(), c.as_ptr()); }
    }
}
