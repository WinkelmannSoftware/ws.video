//! Wo das Programm liegt (auch als AppImage) und der Eintrag im Startmenü für die AppImage-Fassung.
use std::path::PathBuf;
use std::process::Command;

const ICON: &str = include_str!("../paket/ws-video.svg");
const DESKTOP: &str = "app.wsvideo.WSVideo.desktop";
const MIME: &str = "app.wsvideo.WSVideo.xml";

pub fn home() -> PathBuf { PathBuf::from(std::env::var_os("HOME").unwrap_or_default()) }

fn data() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".local/share"))
}

/// Pfad der AppImage-Datei, wenn das Programm aus einem AppImage läuft.
pub fn appimage() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file())
}

/// Aufruf, der dieses Programm von außen startet (für Tastenkürzel und Startmenü).
pub fn exe() -> String {
    let installed = home().join(".local/bin/ws-video");
    if let Some(a) = appimage() {
        // der Befehl `ws-video` aus dem Startmenü-Eintrag zeigt auf dieses AppImage: der bleibt auch nach einem Umzug der Datei gültig
        if std::fs::canonicalize(&installed).ok() == std::fs::canonicalize(&a).ok() { return installed.to_string_lossy().to_string(); }
        return a.to_string_lossy().to_string();
    }
    if installed.exists() { return installed.to_string_lossy().to_string(); }
    std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| "ws-video".into())
}

fn desktop_file() -> PathBuf { data().join("applications").join(DESKTOP) }
fn icon_file() -> PathBuf { data().join("icons/hicolor/scalable/apps/ws-video.svg") }
fn mime_file() -> PathBuf { data().join("mime/packages").join(MIME) }
fn link_file() -> PathBuf { home().join(".local/bin/ws-video") }

/// Schreibweise für die Zeile `Exec=` (Pfade mit Leerzeichen und Sonderzeichen).
fn quoted(path: &str) -> String {
    let mut s = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') { s.push('\\'); }
        s.push(c);
    }
    s.push('"');
    s
}

/// Zeigt der Eintrag im Startmenü schon auf dieses AppImage?
pub fn integrated() -> bool {
    let Some(a) = appimage() else { return true };
    std::fs::read_to_string(desktop_file()).map(|t| t.contains(&format!("Exec={} ", quoted(&a.to_string_lossy())))).unwrap_or(false)
}

fn quiet(cmd: &str, args: &[&str]) { let _ = Command::new(cmd).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status(); }

/// Trägt das AppImage ins Startmenü ein, ordnet ihm Projektdateien zu und legt den Befehl `ws-video` an.
pub fn integrate() -> Result<String, String> {
    let a = appimage().ok_or("Das ist nur für die AppImage-Fassung nötig. Dieses Programm läuft nicht aus einem AppImage.")?;
    let a = a.to_string_lossy().to_string();
    let err = |e: std::io::Error| format!("Der Eintrag konnte nicht geschrieben werden: {e}");
    for f in [desktop_file(), icon_file(), mime_file(), link_file()] {
        if let Some(d) = f.parent() { std::fs::create_dir_all(d).map_err(err)?; }
    }
    std::fs::write(icon_file(), ICON).map_err(err)?;
    std::fs::write(desktop_file(), format!("[Desktop Entry]\nType=Application\nName=WS.Video\nComment=Bildschirm aufnehmen und Videos bearbeiten\nExec={} %F\nIcon=ws-video\nTerminal=false\nCategories=AudioVideo;Video;Recorder;\nMimeType=application/x-wsvideo;\nStartupWMClass=app.wsvideo.WSVideo\n", quoted(&a))).map_err(err)?;
    std::fs::write(mime_file(), "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n  <mime-type type=\"application/x-wsvideo\">\n    <comment>WS.Video-Projekt</comment>\n    <glob pattern=\"*.wsvideo\"/>\n    <icon name=\"ws-video\"/>\n  </mime-type>\n</mime-info>\n").map_err(err)?;
    let d = data();
    quiet("update-mime-database", &[&d.join("mime").to_string_lossy()]);
    quiet("update-desktop-database", &[&d.join("applications").to_string_lossy()]);
    quiet("xdg-mime", &["default", DESKTOP, "application/x-wsvideo"]);
    quiet("gtk-update-icon-cache", &["-q", &d.join("icons/hicolor").to_string_lossy()]);
    // Befehl `ws-video` für das Terminal: nur anlegen, wenn dort kein fest installiertes Programm liegt
    let link = link_file();
    let mut command = String::new();
    let is_link = std::fs::symlink_metadata(&link).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    if is_link || !link.exists() {
        let _ = std::fs::remove_file(&link);
        if std::os::unix::fs::symlink(&a, &link).is_ok() {
            command = format!("\nIm Terminal startet es mit „{}“.", link.display());
        }
    }
    Ok(format!("WS.Video steht jetzt im Startmenü und öffnet Projektdateien per Doppelklick.{command}\nDie Datei {a} muss dafür an ihrem Platz bleiben."))
}

/// Nimmt den Eintrag wieder heraus.
pub fn disintegrate() -> String {
    for f in [desktop_file(), icon_file(), mime_file()] { let _ = std::fs::remove_file(f); }
    let link = link_file();
    if std::fs::symlink_metadata(&link).map(|m| m.file_type().is_symlink()).unwrap_or(false) { let _ = std::fs::remove_file(&link); }
    let d = data();
    quiet("update-mime-database", &[&d.join("mime").to_string_lossy()]);
    quiet("update-desktop-database", &[&d.join("applications").to_string_lossy()]);
    "Der Eintrag im Startmenü, die Zuordnung der Projektdateien und der Befehl „ws-video“ wurden entfernt. Projekte, Aufnahmen und Einstellungen bleiben erhalten.".into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn exec_quoting() {
        assert_eq!(super::quoted("/home/a b/WS.Video.AppImage"), "\"/home/a b/WS.Video.AppImage\"");
        assert_eq!(super::quoted("/x/$a\"b"), "\"/x/\\$a\\\"b\"");
    }
}
