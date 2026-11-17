//! Hyprland-Anbindung über den IPC-Socket: Monitore und Zeigerposition.
use crate::cursordata::Monitor;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

fn socket_path() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    if let Ok(rt) = std::env::var("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(rt).join("hypr").join(&sig).join(".socket.sock");
        if p.exists() { return Some(p); }
    }
    let old = PathBuf::from("/tmp/hypr").join(&sig).join(".socket.sock");
    if old.exists() { Some(old) } else { None }
}

pub fn available() -> bool { socket_path().is_some() }

pub fn request(cmd: &str) -> Option<String> {
    let mut s = UnixStream::connect(socket_path()?).ok()?;
    let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(300)));
    let _ = s.set_write_timeout(Some(std::time::Duration::from_millis(300)));
    s.write_all(cmd.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

pub fn cursor_pos() -> Option<(f64, f64)> {
    let s = request("cursorpos")?;
    let mut it = s.trim().split(',');
    let x: f64 = it.next()?.trim().parse().ok()?;
    let y: f64 = it.next()?.trim().parse().ok()?;
    Some((x, y))
}

/// Je Monitor: Name, Nummer und die Arbeitsumgebung, die er gerade zeigt.
pub fn monitor_workspaces() -> Vec<(String, i64, i64)> {
    let Some(s) = request("j/monitors") else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return vec![] };
    v.as_array().cloned().unwrap_or_default().iter().map(|m| (
        m.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        m.get("id").and_then(|x| x.as_i64()).unwrap_or(-1),
        m.get("activeWorkspace").and_then(|w| w.get("id")).and_then(|x| x.as_i64()).unwrap_or(-1),
    )).collect()
}

/// Fenster mit genau diesem Titel: (schwebend, angeheftet, Nummer des Monitors)
pub fn client(title: &str) -> Option<(bool, bool, i64)> {
    let s = request("j/clients")?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    let c = v.as_array()?.iter().find(|c| c.get("title").and_then(|t| t.as_str()) == Some(title))?.clone();
    Some((
        c.get("floating").and_then(|x| x.as_bool()).unwrap_or(false),
        c.get("pinned").and_then(|x| x.as_bool()).unwrap_or(false),
        c.get("monitor").and_then(|x| x.as_i64()).unwrap_or(-1),
    ))
}

/// Belegte Tasten: (Modifikator-Maske, Taste in Großbuchstaben)
pub fn binds() -> Vec<(u64, String)> {
    let Some(s) = request("j/binds") else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return vec![] };
    v.as_array().cloned().unwrap_or_default().iter().map(|b| (
        b.get("modmask").and_then(|x| x.as_u64()).unwrap_or(0),
        b.get("key").and_then(|x| x.as_str()).unwrap_or("").to_uppercase(),
    )).collect()
}

pub fn monitors() -> Vec<Monitor> {
    let Some(s) = request("j/monitors") else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return vec![] };
    let mut out = Vec::new();
    for m in v.as_array().cloned().unwrap_or_default() {
        let f = |k: &str| m.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
        let scale = if f("scale") > 0.0 { f("scale") } else { 1.0 };
        let (mut pw, mut ph) = (f("width"), f("height"));
        let transform = f("transform") as i32;
        if transform % 2 == 1 { std::mem::swap(&mut pw, &mut ph); }
        out.push(Monitor {
            name: m.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            x: f("x"),
            y: f("y"),
            w: pw / scale,
            h: ph / scale,
            pw: pw as u32,
            ph: ph as u32,
            scale,
        });
    }
    out
}
