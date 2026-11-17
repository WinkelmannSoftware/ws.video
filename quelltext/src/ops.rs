//! Bearbeitungsschritte auf dem Projektmodell (ohne Oberfläche).
use crate::engine;
use crate::model::*;
use crate::recorder::session::RecResult;
use std::path::Path;

pub fn snap_frame(t: f64, fps: u32) -> f64 {
    let f = fps.max(1) as f64;
    (t * f).round() / f
}

pub fn media_item(path: &str) -> anyhow::Result<MediaItem> {
    let info = engine::probe(path)?;
    let name = Path::new(path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
    let kind = if info.is_image { Kind::Image } else if info.has_video { Kind::Video } else { Kind::Audio };
    Ok(MediaItem { path: path.to_string(), name, kind, duration: info.duration, width: info.width, height: info.height, has_video: info.has_video, has_audio: info.has_audio, cursor: String::new() })
}

/// Medium in die Medienliste aufnehmen (falls noch nicht vorhanden) und Index liefern.
pub fn add_media(p: &mut Project, m: MediaItem) -> usize {
    if let Some(i) = p.media.iter().position(|x| x.path == m.path) { return i; }
    p.media.push(m);
    p.media.len() - 1
}

/// Rechteck, das ein Bild der Größe (w, h) in die Arbeitsfläche einpasst (kleinere Bilder bleiben in Originalgröße).
pub fn fit_rect(p: &Project, w: u32, h: u32) -> (f64, f64, f64, f64) {
    let (cw, ch) = (p.width as f64, p.height as f64);
    let (w, h) = (w.max(1) as f64, h.max(1) as f64);
    let k = (cw / w).min(ch / h).min(1.0);
    let (dw, dh) = (w * k, h * k);
    ((cw - dw) / 2.0, (ch - dh) / 2.0, dw, dh)
}

/// Clip aus einem Medium anlegen. `track` = gewünschte Spur (sonst erste freie).
pub fn clip_from_media(p: &mut Project, m: &MediaItem, start: f64, track: Option<u64>) -> u64 {
    let dur = if m.kind == Kind::Image || m.duration <= 0.0 { 5.0 } else { m.duration };
    let start = snap_frame(start.max(0.0), p.fps);
    let track = p.free_track(start, dur, track);
    let id = p.new_id();
    let (x, y, w, h) = if m.has_video { fit_rect(p, m.width, m.height) } else { (0.0, 0.0, 0.0, 0.0) };
    let mut c = Clip {
        id, track, kind: m.kind, name: m.name.clone(), path: m.path.clone(), start, duration: dur, inpoint: 0.0,
        media_duration: if m.kind == Kind::Image { 0.0 } else { m.duration },
        has_video: m.has_video, has_audio: m.has_audio, src_w: m.width, src_h: m.height, x, y, w, h, ..Default::default()
    };
    if !m.cursor.is_empty() { c.cursor = Some(CursorFx { data: m.cursor.clone(), ..Default::default() }); }
    p.clips.push(c);
    id
}

/// Projekt aus einer Aufnahme: jede Quelle auf einer eigenen Spur, Kamera als Kreis unten rechts.
pub fn project_from_recording(r: &RecResult, fps: u32, cam_bg: (BgMode, String)) -> Project {
    let mut p = Project { tracks: vec![], next_id: 1, fps, ..Default::default() };
    let screen = r.screen.as_ref().and_then(|f| media_item(&f.path).ok().map(|m| (m, f.offset)));
    let camera = r.camera.as_ref().and_then(|f| media_item(&f.path).ok().map(|m| (m, f.offset)));
    let mic = r.mic.as_ref().and_then(|f| media_item(&f.path).ok().map(|m| (m, f.offset)));
    let system = r.system.as_ref().and_then(|f| media_item(&f.path).ok().map(|m| (m, f.offset)));
    if let Some((m, _)) = &screen {
        // Projektgröße = Aufnahmegröße (gerade Zahlen)
        p.width = (m.width / 2 * 2).max(320);
        p.height = (m.height / 2 * 2).max(240);
    } else if let Some((m, _)) = &camera {
        p.width = (m.width / 2 * 2).max(320);
        p.height = (m.height / 2 * 2).max(240);
    }
    // Das Projekt beginnt mit dem ersten Bild: Ton, der etwas früher einsetzt, wird vorn entsprechend gekürzt.
    let t0 = [&screen, &camera].iter().filter_map(|s| s.as_ref().map(|x| x.1)).fold(f64::INFINITY, f64::min);
    let t0 = if t0.is_finite() { t0 } else { 0.0 };
    fn place(p: &mut Project, m: &MediaItem, off: f64, t0: f64, track: u64) -> u64 {
        let id = clip_from_media(p, m, (off - t0).max(0.0), Some(track));
        if off < t0 {
            let cut = t0 - off;
            if let Some(c) = p.clip_mut(id) {
                if c.duration > cut + 0.1 {
                    c.inpoint = cut;
                    c.duration -= cut;
                }
            }
        }
        id
    }
    if let Some((mut m, off)) = system {
        m.name = "Systemton".into();
        let t = p.add_track("Systemton");
        add_media(&mut p, m.clone());
        place(&mut p, &m, off, t0, t);
    }
    if let Some((mut m, off)) = mic {
        m.name = "Mikrofon".into();
        let t = p.add_track("Mikrofon");
        add_media(&mut p, m.clone());
        place(&mut p, &m, off, t0, t);
    }
    let has_screen = screen.is_some();
    if let Some((mut m, off)) = screen {
        m.name = "Bildschirm".into();
        if let Some(c) = &r.cursor { m.cursor = c.clone(); }
        let t = p.add_track("Bildschirm");
        add_media(&mut p, m.clone());
        place(&mut p, &m, off, t0, t);
    }
    if let Some((mut m, off)) = camera {
        m.name = "Kamera".into();
        let t = p.add_track("Kamera");
        add_media(&mut p, m.clone());
        let id = place(&mut p, &m, off, t0, t);
        if let Some(c) = p.clip_mut(id) {
            c.bg_mode = cam_bg.0;
            c.bg_image = cam_bg.1.clone();
        }
        if has_screen {
            let (cw, ch) = (p.width as f64, p.height as f64);
            if let Some(c) = p.clip_mut(id) {
                // mittiges Quadrat als Kreis unten rechts
                let side = (ch * 0.26).round();
                let margin = (ch * 0.04).round();
                if c.src_w > c.src_h {
                    let k = (1.0 - c.src_h as f64 / c.src_w as f64) / 2.0;
                    c.crop = [k, 0.0, k, 0.0];
                }
                c.shape = Shape::Ellipse;
                c.w = side;
                c.h = side;
                c.x = cw - side - margin;
                c.y = ch - side - margin;
                c.border = (ch * 0.005).round().max(3.0);
                c.shadow = true;
            }
        }
    }
    while p.tracks.len() < 2 {
        let n = p.tracks.len() + 1;
        p.add_track(&format!("Spur {n}"));
    }
    p
}

/// Clip an der Zeit `t` teilen; liefert die ID des rechten Teils.
pub fn split_clip(p: &mut Project, id: u64, t: f64) -> Option<u64> {
    let t = snap_frame(t, p.fps);
    let c = p.clip(id)?.clone();
    let min = 1.0 / p.fps.max(1) as f64;
    if t <= c.start + min - 1e-9 || t >= c.end() - min + 1e-9 { return None; }
    let new_id = p.new_id();
    let mut right = c.clone();
    right.id = new_id;
    right.start = t;
    right.duration = c.end() - t;
    right.inpoint = c.inpoint + (t - c.start);
    right.fade_in = 0.0;
    if let Some(l) = p.clip_mut(id) {
        l.duration = t - c.start;
        l.fade_out = 0.0;
    }
    p.clips.push(right);
    Some(new_id)
}

/// Bereich [a, b] aus allen nicht gesperrten Spuren herausschneiden und die Lücke schließen.
pub fn ripple_cut(p: &mut Project, a: f64, b: f64) { cut_range(p, a, b, &[], true); }

/// Bereich [a, b] herausnehmen. `only` nennt die Spuren (leer = alle), gesperrte bleiben immer unberührt.
/// Mit `close` rückt alles Folgende auf diesen Spuren nach, sonst bleibt die Lücke.
pub fn cut_range(p: &mut Project, a: f64, b: f64, only: &[u64], close: bool) {
    let (a, b) = (snap_frame(a.min(b), p.fps), snap_frame(a.max(b), p.fps));
    let len = b - a;
    if len <= 0.0 { return; }
    // „gesperrt“ heißt hier: wird nicht angefasst (gesperrte Spur oder nicht ausgewählt)
    let locked: Vec<u64> = p.tracks.iter().filter(|t| t.locked || (!only.is_empty() && !only.contains(&t.id))).map(|t| t.id).collect();
    let ids: Vec<u64> = p.clips.iter().filter(|c| !locked.contains(&c.track)).map(|c| c.id).collect();
    for id in &ids {
        if let Some(c) = p.clip(*id) { if c.start < a && c.end() > a { split_clip(p, *id, a); } }
    }
    let ids: Vec<u64> = p.clips.iter().filter(|c| !locked.contains(&c.track)).map(|c| c.id).collect();
    for id in &ids {
        if let Some(c) = p.clip(*id) { if c.start < b && c.end() > b { split_clip(p, *id, b); } }
    }
    p.clips.retain(|c| locked.contains(&c.track) || !(c.start >= a - 1e-9 && c.end() <= b + 1e-9));
    if !close { return; }
    for c in p.clips.iter_mut() {
        if !locked.contains(&c.track) && c.start >= b - 1e-9 { c.start = (c.start - len).max(0.0); }
    }
}

/// Clips löschen; mit `ripple` rücken die folgenden Clips derselben Spur nach.
pub fn delete_clips(p: &mut Project, ids: &[u64], ripple: bool) {
    let mut removed: Vec<Clip> = p.clips.iter().filter(|c| ids.contains(&c.id)).cloned().collect();
    p.clips.retain(|c| !ids.contains(&c.id));
    if ripple {
        removed.sort_by(|a, b| b.start.partial_cmp(&a.start).unwrap());
        for r in removed {
            for c in p.clips.iter_mut() {
                if c.track == r.track && c.start >= r.end() - 1e-6 { c.start = (c.start - r.duration).max(0.0); }
            }
        }
    }
}

/// Neue Anmerkung an der Zeit `t` mittig einfügen.
pub fn add_annotation(p: &mut Project, kind: Kind, shape: Shape, t: f64) -> u64 {
    let dur = 4.0;
    let t = snap_frame(t.max(0.0), p.fps);
    // Anmerkungen gehören nach oben: höchste Spur, wenn sie frei ist, sonst neue Spur
    // die oberste Spur ohne Medien, die an dieser Stelle frei ist; sonst eine neue Spur ganz oben
    let free = p.tracks.iter().rev().map(|t| t.id).find(|tid| {
        let mut on = p.clips.iter().filter(|c| c.track == *tid).peekable();
        on.peek().is_some() && p.clips.iter().filter(|c| c.track == *tid).all(|c| !c.kind.is_media() && (c.end() <= t + 1e-6 || c.start >= t + dur - 1e-6))
    });
    let track = match free {
        Some(tid) => tid,
        None => {
            let n = p.tracks.len() + 1;
            p.add_track(&format!("Spur {n}"))
        }
    };
    let id = p.new_id();
    let (cw, ch) = (p.width as f64, p.height as f64);
    let k = ch / 1080.0;
    let mut c = Clip { id, track, kind, start: t, duration: dur, shape, ..Default::default() };
    match kind {
        Kind::Text => {
            c.name = "Text".into();
            c.text = "Text".into();
            c.font_size = (72.0 * k).round();
            c.w = (cw * 0.5).round();
            c.h = (140.0 * k).round();
            c.fill = "#00000000".into();
            c.shadow = true;
        }
        Kind::Shape => {
            c.name = "Form".into();
            c.w = (cw * 0.25).round();
            c.h = (ch * 0.25).round();
            c.fill = "#ff8a3d55".into();
            c.stroke = "#ff8a3d".into();
            c.stroke_w = (6.0 * k).round();
            c.radius = 0.2;
        }
        Kind::Arrow => {
            c.name = "Pfeil".into();
            c.w = (cw * 0.2).round();
            c.h = (ch * 0.15).round();
            c.fill = "#ff3d3d".into();
            c.stroke_w = (14.0 * k).round();
            c.shadow = true;
        }
        Kind::Blur => {
            c.name = "Weichzeichner".into();
            c.w = (cw * 0.25).round();
            c.h = (ch * 0.15).round();
            c.blur = 0.6;
        }
        _ => {}
    }
    c.x = ((cw - c.w) / 2.0).round();
    c.y = ((ch - c.h) / 2.0).round();
    p.clips.push(c);
    id
}

pub fn duplicate_clip(p: &mut Project, id: u64) -> Option<u64> {
    let mut c = p.clip(id)?.clone();
    c.id = p.new_id();
    c.start = c.end();
    c.track = p.free_track(c.start, c.duration, Some(c.track));
    let nid = c.id;
    p.clips.push(c);
    Some(nid)
}

/// Leere Spuren am oberen Ende entfernen (mindestens zwei bleiben).
pub fn prune_tracks(p: &mut Project) {
    while p.tracks.len() > 2 {
        let last = p.tracks.last().unwrap().id;
        if p.clips.iter().any(|c| c.track == last) { break; }
        p.tracks.pop();
    }
}

/// Projektgröße ändern und alle Clips passend mitskalieren.
pub fn resize_project(p: &mut Project, w: u32, h: u32) {
    let (ow, oh) = (p.width.max(1) as f64, p.height.max(1) as f64);
    let (nw, nh) = (w as f64, h as f64);
    let k = (nw / ow).min(nh / oh);
    let (dx, dy) = ((nw - ow * k) / 2.0, (nh - oh * k) / 2.0);
    for c in p.clips.iter_mut() {
        c.x = c.x * k + dx;
        c.y = c.y * k + dy;
        c.w *= k;
        c.h *= k;
        for a in c.anims.iter_mut() {
            a.x = a.x * k + dx;
            a.y = a.y * k + dy;
            a.w *= k;
            a.h *= k;
        }
        c.font_size *= k;
        c.stroke_w *= k;
        c.border *= k;
    }
    p.width = w;
    p.height = h;
}
