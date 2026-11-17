//! Fehlerbericht: Angaben zu Programm, System, Hardware und Aufnahme-Umgebung als Text.
//! Der Bericht wird nur auf ausdrücklichen Wunsch geschrieben, bleibt auf dem Rechner und nennt weder Benutzer- noch Rechnernamen.
use std::path::PathBuf;
use std::process::Command;

pub const NAME: &str = "ws-video-fehlerbericht.txt";

pub const NOTICE: &str = "Dieser Bericht enthält technische Angaben zu WS.Video, zum Betriebssystem und zur Hardware (Prozessor, Arbeitsspeicher, Grafik, Monitore, Namen von Kameras), die Einstellungen von WS.Video und die Programmprotokolle. Benutzername und Rechnername sind ersetzt. Inhalte von Aufnahmen sind nicht enthalten, Dateinamen aus den Protokollen können vorkommen. Die Datei liegt nur auf diesem Rechner und wird nicht automatisch versendet. Wer sie weitergibt, zum Beispiel an den Support von WS.Video, ist damit einverstanden, dass die Angaben zur Fehlersuche verarbeitet werden.";

fn sh(cmd: &str) -> String {
    Command::new("sh").arg("-c").arg(cmd).output().map(|o| String::from_utf8_lossy(&o.stdout).trim_end().to_string()).unwrap_or_default()
}

fn read(path: &str) -> String { std::fs::read_to_string(path).unwrap_or_default() }

fn env(name: &str) -> String { std::env::var(name).unwrap_or_default() }

fn or_unknown(s: String) -> String { if s.trim().is_empty() { "unbekannt".into() } else { s } }

fn yes(b: bool) -> &'static str { if b { "ja" } else { "nein" } }

/// Ordner „Downloads“ des Benutzers (auch wenn er in der Landessprache anders heißt), sonst der Persönliche Ordner.
pub fn downloads() -> PathBuf {
    let home = crate::selbst::home();
    let x = PathBuf::from(sh("xdg-user-dir DOWNLOAD 2>/dev/null"));
    [x, home.join("Downloads")].into_iter().find(|p| p.is_absolute() && p.is_dir() && *p != home).unwrap_or(home)
}

fn os_release(key: &str) -> String {
    read("/etc/os-release").lines().find_map(|l| l.strip_prefix(&format!("{key}=")).map(|v| v.trim_matches('"').to_string())).unwrap_or_default()
}

fn glibc() -> String {
    extern "C" { fn gnu_get_libc_version() -> *const libc::c_char; }
    unsafe { std::ffi::CStr::from_ptr(gnu_get_libc_version()).to_string_lossy().to_string() }
}

fn meminfo(key: &str) -> f64 {
    read("/proc/meminfo").lines().find_map(|l| l.strip_prefix(&format!("{key}:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok())).unwrap_or(0.0) / 1048576.0
}

fn graphics() -> String {
    let l = sh("lspci 2>/dev/null | grep -i -E 'vga|3d|display' | sed 's/^[0-9a-f:.]* //'");
    let mut out = if l.is_empty() { String::new() } else { format!("{l}\n") };
    // Treiber je Grafikkarte, auch ohne lspci
    if let Ok(rd) = std::fs::read_dir("/sys/class/drm") {
        let mut cards: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("card") && !n.contains('-')).collect();
        cards.sort();
        for c in cards {
            let ue = read(&format!("/sys/class/drm/{c}/device/uevent"));
            let get = |k: &str| ue.lines().find_map(|l| l.strip_prefix(&format!("{k}=")).map(|s| s.to_string())).unwrap_or_default();
            out.push_str(&format!("{c}: Treiber {}, Kennung {}\n", or_unknown(get("DRIVER")), or_unknown(get("PCI_ID"))));
        }
    }
    or_unknown(out.trim_end().to_string())
}

fn monitors() -> String {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/drm") {
        let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("card") && n.contains('-')).collect();
        names.sort();
        for n in names {
            if read(&format!("/sys/class/drm/{n}/status")).trim() != "connected" { continue; }
            let mode = read(&format!("/sys/class/drm/{n}/modes")).lines().next().unwrap_or("").to_string();
            out.push(format!("{}: {}", n.split_once('-').map(|x| x.1).unwrap_or(&n), or_unknown(mode)));
        }
    }
    if out.is_empty() { "unbekannt".into() } else { out.join("\n") }
}

fn cameras() -> String {
    let mut out: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/video4linux") {
        for e in rd.flatten() {
            let n = read(&e.path().join("name").to_string_lossy()).trim().to_string();
            if !n.is_empty() && !out.contains(&n) { out.push(n); }
        }
    }
    if out.is_empty() { "keine gefunden".into() } else { out.join("\n") }
}

fn elements() -> String {
    let _ = gst::init();
    let has = |n: &str| gst::ElementFactory::find(n).is_some();
    let list = |names: &[&str]| -> String {
        let (ok, missing): (Vec<&str>, Vec<&str>) = names.iter().partition(|n| has(n));
        format!("vorhanden: {}\n  fehlt: {}", if ok.is_empty() { "–".into() } else { ok.join(", ") }, if missing.is_empty() { "nichts".into() } else { missing.join(", ") })
    };
    format!(
        "Grundbausteine {}\nAufnahme {}\nBild kodieren {}\nTon kodieren {}\nDateiformate {}",
        list(&["uridecodebin", "videoconvert", "videoscale", "audioconvert", "audioresample", "compositor", "audiomixer", "nlecomposition", "volume", "level", "alpha", "capssetter", "circle"]),
        list(&["pipewiresrc", "pulsesrc", "v4l2src", "ximagesrc"]),
        list(&["x264enc", "openh264enc", "vah264enc", "vaapih264enc", "nvh264enc", "qsvh264enc", "x265enc", "svtav1enc", "av1enc"]),
        list(&["fdkaacenc", "avenc_aac", "voaacenc", "opusenc"]),
        list(&["matroskamux", "matroskademux", "mp4mux", "qtdemux", "h264parse", "avdec_h264", "avdec_aac"]),
    )
}

fn settings() -> String {
    let p = gtk::glib::user_config_dir().join("ws-video/einstellungen.json");
    let Ok(v) = std::fs::read_to_string(&p).map_err(|_| ()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|_| ())) else { return "noch keine gespeichert".into() };
    let Some(map) = v.as_object() else { return "nicht lesbar".into() };
    // Zugangsmarken des Portals und Dateilisten gehören nicht in den Bericht
    map.iter().filter(|(k, _)| !k.contains("token") && k.as_str() != "recent").map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n")
}

/// Alles über Programm, System, Hardware und Aufnahme-Umgebung.
pub fn system() -> String {
    let appimage = crate::selbst::appimage();
    let packaging = match &appimage {
        Some(a) => format!("AppImage ({})", a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        None => format!("eigenständiges Programm ({})", std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()),
    };
    let cpu = read("/proc/cpuinfo");
    let model = cpu.lines().find_map(|l| l.strip_prefix("model name").map(|v| v.trim_start_matches([' ', '\t', ':']).to_string())).unwrap_or_default();
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    let groups = sh("id -nG 2>/dev/null");
    let mut s = String::new();
    s.push_str("\n######## Programm\n");
    s.push_str(&format!("Version: {}\nPaket: {}\nGTK: {}.{}.{}\nGStreamer: {}\n", crate::VERSION, packaging, gtk::major_version(), gtk::minor_version(), gtk::micro_version(), { let _ = gst::init(); gst::version_string() }));
    s.push_str("\n######## System\n");
    s.push_str(&format!("Distribution: {} (Kennung {}, Version {}, verwandt mit {})\n", or_unknown(os_release("PRETTY_NAME")), or_unknown(os_release("ID")), or_unknown(os_release("VERSION_ID")), or_unknown(os_release("ID_LIKE"))));
    s.push_str(&format!("Kern: {}\nglibc: {}\n", or_unknown(sh("uname -r -m")), glibc()));
    s.push_str(&format!("Desktop: {}\nSitzung: {}\nWayland: {}\nX11: {}\nHyprland: {}\nSprache: {}\n", or_unknown(env("XDG_CURRENT_DESKTOP")), or_unknown(env("XDG_SESSION_TYPE")), yes(!env("WAYLAND_DISPLAY").is_empty()), yes(!env("DISPLAY").is_empty()), yes(crate::recorder::hypr::available()), or_unknown(env("LANG"))));
    s.push_str("\n######## Hardware\n");
    s.push_str(&format!("Prozessor: {} ({} Kerne nutzbar)\n", or_unknown(model), cores));
    s.push_str(&format!("Arbeitsspeicher: {:.1} GB, davon verfügbar {:.1} GB\n", meminfo("MemTotal"), meminfo("MemAvailable")));
    s.push_str(&format!("Grafik:\n{}\nMonitore:\n{}\nKameras:\n{}\n", graphics(), monitors(), cameras()));
    s.push_str(&format!("Freier Platz im Persönlichen Ordner: {}\n", or_unknown(sh("df -h --output=avail \"$HOME\" 2>/dev/null | tail -n 1 | tr -d ' '"))));
    s.push_str("\n######## Aufnahme-Umgebung\n");
    let portal = sh("timeout 5 busctl --user get-property org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop org.freedesktop.portal.ScreenCast version AvailableCursorModes AvailableSourceTypes 2>&1 | tr '\\n' ' '");
    s.push_str(&format!("Bildschirmportal (Version, Zeigerarten, Quellenarten): {}\n", or_unknown(portal)));
    s.push_str(&format!("Portal-Dienste: {}\n", or_unknown(sh("ls /usr/libexec/xdg-desktop-portal* /usr/lib/xdg-desktop-portal* /usr/lib/*/xdg-desktop-portal* 2>/dev/null | xargs -r -n1 basename | sort -u | tr '\\n' ' '"))));
    s.push_str(&format!("PipeWire: {}\n", or_unknown(sh("pipewire --version 2>/dev/null | tail -n 1"))));
    s.push_str(&format!("Tonserver: {}\n", or_unknown(sh("LC_ALL=C pactl info 2>/dev/null | grep -E '^Server (Name|Version)' | cut -d: -f2 | tr '\\n' ' '"))));
    s.push_str(&format!("Gruppe „input“ (für Klickdaten): {}\n", yes(groups.split_whitespace().any(|g| g == "input"))));
    s.push_str(&format!("Videobeschleunigung: {}\n", or_unknown(sh("ls /dev/dri 2>/dev/null | tr '\\n' ' '"))));
    s.push_str("\n######## Bausteine von GStreamer\n");
    s.push_str(&elements());
    s.push_str("\n\n######## Einstellungen von WS.Video\n");
    s.push_str(&settings());
    s.push('\n');
    s
}

/// Benutzer- und Rechnernamen aus dem Text nehmen.
pub fn anonymize(text: &str) -> String {
    let mut t = text.to_string();
    let home = crate::selbst::home().to_string_lossy().to_string();
    if home.len() > 1 { t = t.replace(&home, "~"); }
    let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    let host = read("/proc/sys/kernel/hostname").trim().to_string();
    for (name, with) in [(user, "<benutzer>"), (host, "<rechner>")] {
        if name.len() >= 3 && name != "root" { t = t.replace(&name, with); }
    }
    t
}
