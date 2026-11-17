//! Zeitleiste: Spuren, Clips, Abspielkopf, Trimmen, Verschieben, Bereichsmarkierung.
use super::editor::Editor;
use super::icons;
use crate::model::*;
use crate::ops::{self, snap_frame};
use gtk::prelude::*;
use gtk::{cairo, gdk, glib};
use std::rc::Rc;

pub const HEADER_W: f64 = 150.0;
pub const RULER_H: f64 = 28.0;
pub const TRACK_H: f64 = 56.0;
pub const GAP: f64 = 4.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TMode {
    Scrub, Range, Move, TrimL, TrimR,
    /// Schere: Klick schneidet, Ziehen markiert einen Bereich. `Drag::clip` trägt die Spur (0 = alle).
    Cut,
    /// Griff oben am Clip: Dauer des Ein- bzw. Ausblendens (oder des Effekts am Anfang/Ende)
    FadeIn, FadeOut,
    /// Balken einer Bewegung (Zoom, Schwenk): ganzen Balken verschieben bzw. am Ende die Dauer ziehen
    AnimMove(usize), AnimEnd(usize),
}
impl TMode {
    pub fn is_effect(self) -> bool { matches!(self, TMode::FadeIn | TMode::FadeOut | TMode::AnimMove(_) | TMode::AnimEnd(_)) }
}

/// Hat der Clip Griffe zum Ein- und Ausblenden?
fn can_fade(c: &Clip) -> bool { c.kind != Kind::Blur }
/// x-Lage der beiden Griffe (Ein, Aus). Sie sitzen etwas innerhalb der Kante, damit Trimmen daneben möglich bleibt.
fn fade_handles(g: &Geo, c: &Clip) -> Option<(f64, f64)> {
    let (x0, x1) = (g.x(c.start), g.x(c.end()));
    if !can_fade(c) || x1 - x0 < 44.0 { return None; }
    let a = (x0 + c.fade_in * g.pps).clamp(x0 + 9.0, x1 - 9.0);
    let b = (x1 - c.fade_out * g.pps).clamp(x0 + 9.0, x1 - 9.0);
    Some((a, b))
}
const HANDLE_Y: f64 = 9.0;
/// Farbe der Schnittlinie
const CUT: (f64, f64, f64) = (1.0, 0.36, 0.36);
/// Schnittzeit unter der Maus: auf ganze Bilder, in der Nähe des Abspielkopfs genau auf ihn
fn cut_time(g: &Geo, x: f64, fps: u32, playhead: f64) -> f64 {
    let t = g.t(x);
    if (t - playhead).abs() * g.pps <= 6.0 { playhead } else { snap_frame(t, fps) }
}
/// Welche Spur trifft die Schere? (Spur-ID oder 0 für alle, und ob alle gemeint sind)
fn cut_target(ed: &Editor, g: &Geo, y: f64, shift: bool) -> (u64, bool) {
    let st = ed.st.borrow();
    if y < RULER_H || (st.cut_all != shift) { return (0, true); }
    let id = g.track_at(y).and_then(|i| st.project.tracks.get(i)).filter(|t| !t.locked).map(|t| t.id).unwrap_or(u64::MAX);
    (id, false)
}
/// Balken einer Bewegung: (x, Breite)
fn anim_bar(g: &Geo, c: &Clip, a: &Anim) -> (f64, f64) {
    (g.x(c.start) + (a.t - c.inpoint) * g.pps, (a.dur * g.pps).max(10.0))
}
/// Text für das Hinweisfeld beim Ziehen
fn effect_text(c: &Clip, mode: TMode, fps: u32) -> String {
    let sec = |v: f64| format!("{:.2} s", v).replace('.', ",");
    let name = |k: FadeKind, enter: bool| { let e = FadeKind::ALL[k.index() as usize]; if enter { e.1 } else { e.2 } };
    match mode {
        TMode::FadeIn if !c.is_visual() => format!("Ton einblenden · {}", sec(c.fade_in)),
        TMode::FadeOut if !c.is_visual() => format!("Ton ausblenden · {}", sec(c.fade_out)),
        TMode::FadeIn => format!("Anfang · {} · {}", name(c.fade_in_kind, true), sec(c.fade_in)),
        TMode::FadeOut => format!("Ende · {} · {}", name(c.fade_out_kind, false), sec(c.fade_out)),
        TMode::AnimMove(i) | TMode::AnimEnd(i) => match c.anims.get(i) {
            Some(a) => format!("Bewegung · Beginn {} · Dauer {}", format_time((c.start + a.t - c.inpoint).max(0.0), fps), sec(a.dur)),
            None => String::new(),
        },
        _ => String::new(),
    }
}

#[derive(Clone, Debug)]
pub struct Drag {
    pub mode: TMode,
    pub clip: u64,
    pub origs: Vec<Clip>,
    pub start: (f64, f64),
    pub start_time: f64,
    pub moved: bool,
    /// Clip soll beim Loslassen auf eine neue Spur
    pub new_track: bool,
}

struct Geo {
    pps: f64,
    scroll: f64,
    vscroll: f64,
    n: usize,
}
impl Geo {
    fn of(ed: &Editor) -> Geo {
        let st = ed.st.borrow();
        Geo { pps: st.tl_pps, scroll: st.tl_scroll, vscroll: st.tl_vscroll, n: st.project.tracks.len() }
    }
    fn x(&self, t: f64) -> f64 { HEADER_W + (t - self.scroll) * self.pps }
    fn t(&self, x: f64) -> f64 { ((x - HEADER_W) / self.pps + self.scroll).max(0.0) }
    /// y der Spur mit Index i (0 = unterste); i == n ist die leere Zeile „neue Spur“ ganz oben
    fn y(&self, i: usize) -> f64 { RULER_H + GAP + (self.n - i) as f64 * (TRACK_H + GAP) - self.vscroll }
    fn track_at(&self, y: f64) -> Option<usize> {
        if y < RULER_H { return None; }
        let row = ((y - RULER_H - GAP + self.vscroll) / (TRACK_H + GAP)).floor() as i64;
        if row < 0 || row > self.n as i64 { return None; }
        Some(self.n - row as usize)
    }
    fn content_h(&self) -> f64 { RULER_H + GAP + (self.n + 1) as f64 * (TRACK_H + GAP) }
}

fn kind_color(k: Kind) -> (f64, f64, f64) {
    match k {
        Kind::Video => (0.20, 0.42, 0.72),
        Kind::Audio => (0.17, 0.55, 0.42),
        Kind::Image => (0.50, 0.36, 0.74),
        Kind::Text | Kind::Shape | Kind::Arrow => (0.76, 0.47, 0.18),
        Kind::Blur => (0.42, 0.45, 0.52),
    }
}

fn ruler_step(pps: f64) -> f64 {
    for s in [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0] {
        if s * pps >= 75.0 { return s; }
    }
    3600.0
}

fn draw(ed: &Editor, cr: &cairo::Context, w: i32, h: i32) {
    let (w, h) = (w as f64, h as f64);
    let g = Geo::of(ed);
    let st = ed.st.borrow();
    let p = &st.project;
    super::theme::set(cr, "grund");
    let _ = cr.paint();
    let offline = ed.engine.borrow().builder.offline.clone();
    let accent = super::theme::accent();

    // Spurzeilen
    for i in 0..=g.n {
        let y = g.y(i);
        if y + TRACK_H < RULER_H || y > h { continue; }
        cr.rectangle(HEADER_W, y, w - HEADER_W, TRACK_H);
        if i == g.n { cr.set_source_rgba(1.0, 1.0, 1.0, 0.015); } else { super::theme::set(cr, "flaeche"); }
        let _ = cr.fill();
    }

    // Clips
    let _ = cr.save();
    cr.rectangle(HEADER_W, RULER_H, w - HEADER_W, h - RULER_H);
    cr.clip();
    let drag = st.tl_drag.as_ref();
    for (ti, tr) in p.tracks.iter().enumerate() {
        let clips = p.clips_on(tr.id);
        for (ci, c) in clips.iter().enumerate() {
            let mut y = g.y(ti);
            if let Some(d) = drag { if d.new_track && d.moved && d.clip == c.id { y = g.y(g.n); } }
            let (x0, x1) = (g.x(c.start), g.x(c.end()));
            if x1 < HEADER_W || x0 > w || y + TRACK_H < RULER_H || y > h { continue; }
            let sel = st.sel.contains(&c.id);
            let (r, gg, b) = kind_color(c.kind);
            let dim = if tr.hidden || (c.kind == Kind::Audio && tr.muted) { 0.55 } else { 1.0 };
            let cw = (x1 - x0).max(2.0);
            let _ = cr.save();
            super::rounded_rect(cr, x0 + 0.5, y + 1.5, cw - 1.0, TRACK_H - 3.0, 6.0);
            cr.set_source_rgba(r * dim, gg * dim, b * dim, if sel { 1.0 } else { 0.86 });
            let _ = cr.fill_preserve();
            cr.clip();
            let mut text_x = x0 + 8.0;
            // Vorschaubild
            if matches!(c.kind, Kind::Video | Kind::Image) && c.has_video && cw > 60.0 {
                ed.request_thumb(&c.path);
                if let Some(Some(s)) = ed.thumbs.borrow().get(&c.path) {
                    let th = TRACK_H - 3.0;
                    let tw = (th * s.width() as f64 / s.height().max(1) as f64).min(cw * 0.5);
                    let tx = x0.max(HEADER_W - tw + 30.0).min(x1 - tw);
                    super::draw_surface(cr, s, tx, y + 1.5, tw, th, true);
                    text_x = tx + tw + 8.0;
                }
            }
            // Wellenform
            if c.carries_audio() {
                ed.request_wave(&c.path);
                if let Some(wave) = ed.waves.borrow().get(&c.path) {
                    let top = if c.is_visual() { y + TRACK_H * 0.42 } else { y + 6.0 };
                    let hh = y + TRACK_H - 5.0 - top;
                    let mid = top + hh / 2.0;
                    cr.set_source_rgba(1.0, 1.0, 1.0, if c.muted || tr.muted { 0.18 } else { 0.55 });
                    let (from, to) = (x0.max(HEADER_W).floor(), x1.min(w).ceil());
                    let mut px = from;
                    while px < to {
                        let ts = c.inpoint + (px - x0) / g.pps;
                        let i0 = (ts * 100.0).floor().max(0.0) as usize;
                        let i1 = (((ts + 1.0 / g.pps) * 100.0).ceil() as usize).max(i0 + 1);
                        let mut peak: f32 = 0.0;
                        for i in i0..i1.min(wave.len()) { peak = peak.max(wave[i]); }
                        let amp = (peak as f64 * c.volume.min(2.0)).min(1.0).sqrt() * hh / 2.0;
                        cr.rectangle(px, mid - amp.max(0.5), 1.0, (amp * 2.0).max(1.0));
                        px += 1.0;
                    }
                    let _ = cr.fill();
                }
            }
            // Ein-/Ausblenden
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.38);
            if c.fade_in > 0.0 {
                let fw = (c.fade_in * g.pps).min(cw);
                cr.move_to(x0, y);
                cr.line_to(x0 + fw, y);
                cr.line_to(x0, y + TRACK_H);
                cr.close_path();
                let _ = cr.fill();
            }
            if c.fade_out > 0.0 {
                let fw = (c.fade_out * g.pps).min(cw);
                cr.move_to(x1, y);
                cr.line_to(x1 - fw, y);
                cr.line_to(x1, y + TRACK_H);
                cr.close_path();
                let _ = cr.fill();
            }
            // helle Kante der Blende, damit man sieht, wie weit sie reicht
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.55);
            cr.set_line_width(1.0);
            if c.fade_in > 0.0 {
                cr.move_to(x0, y + TRACK_H);
                cr.line_to(x0 + (c.fade_in * g.pps).min(cw), y + HANDLE_Y);
                let _ = cr.stroke();
            }
            if c.fade_out > 0.0 {
                cr.move_to(x1, y + TRACK_H);
                cr.line_to(x1 - (c.fade_out * g.pps).min(cw), y + HANDLE_Y);
                let _ = cr.stroke();
            }
            // Überblendung mit dem nächsten Clip
            if let Some(next) = clips.get(ci + 1) {
                if next.start < c.end() - 1e-6 {
                    let (ox0, ox1) = (g.x(next.start), x1);
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
                    cr.set_line_width(1.0);
                    cr.move_to(ox0, y + 2.0);
                    cr.line_to(ox1, y + TRACK_H - 2.0);
                    cr.move_to(ox0, y + TRACK_H - 2.0);
                    cr.line_to(ox1, y + 2.0);
                    let _ = cr.stroke();
                }
            }
            // Animationen (Zoom & Schwenk)
            for a in &c.anims {
                let ax = x0 + (a.t - c.inpoint) * g.pps;
                let aw = (a.dur * g.pps).max(10.0);
                if ax + aw < x0 || ax > x1 { continue; }
                cr.set_source_rgba(1.0, 0.82, 0.24, 0.95);
                super::rounded_rect(cr, ax, y + TRACK_H - 13.0, aw, 9.0, 3.0);
                let _ = cr.fill();
                cr.move_to(ax + aw, y + TRACK_H - 15.0);
                cr.line_to(ax + aw + 7.0, y + TRACK_H - 8.5);
                cr.line_to(ax + aw, y + TRACK_H - 2.0);
                cr.close_path();
                let _ = cr.fill();
                // Griffkerbe am Ende
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
                cr.rectangle(ax + aw - 3.0, y + TRACK_H - 11.0, 1.5, 5.0);
                let _ = cr.fill();
            }
            // Griffe für Ein- und Ausblenden
            if let Some((ha, hb)) = fade_handles(&g, c) {
                for (hx, on) in [(ha, c.fade_in > 0.0), (hb, c.fade_out > 0.0)] {
                    cr.arc(hx, y + HANDLE_Y, 4.5, 0.0, std::f64::consts::TAU);
                    cr.set_source_rgba(1.0, 1.0, 1.0, if sel { 1.0 } else if on { 0.85 } else { 0.5 });
                    let _ = cr.fill_preserve();
                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
                    cr.set_line_width(1.0);
                    let _ = cr.stroke();
                }
            }
            if offline.contains(&c.id) {
                cr.set_source_rgba(0.8, 0.1, 0.1, 0.55);
                let _ = cr.paint();
            }
            // Name
            let name = if offline.contains(&c.id) {
                // ehrlich bleiben: Die Datei kann auch da sein und sich nur nicht laden lassen
                if c.kind.is_media() && !std::path::Path::new(&c.path).exists() { format!("Datei fehlt: {}", c.name) } else { format!("Nicht ladbar: {}", c.name) }
            } else if c.kind == Kind::Text { c.text.replace('\n', " ") } else { c.name.clone() };
            let tx = text_x.max(HEADER_W + 6.0).min(x1 - 20.0);
            if x1 - tx > 24.0 {
                super::draw_text(cr, &name, tx, y + 5.0, 11.0, (1.0, 1.0, 1.0, 0.95), true, x1 - tx - 6.0);
            }
            let _ = cr.restore();
            if sel {
                super::rounded_rect(cr, x0 + 1.0, y + 2.0, cw - 2.0, TRACK_H - 4.0, 6.0);
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.set_line_width(2.0);
                let _ = cr.stroke();
            }
        }
    }
    // markierter Bereich
    if let Some((a, b)) = st.range {
        let (xa, xb) = (g.x(a.min(b)), g.x(a.max(b)));
        if st.range_tracks.is_empty() {
            cr.rectangle(xa, RULER_H, xb - xa, h - RULER_H);
            cr.set_source_rgba(accent.0, accent.1, accent.2, 0.18);
            let _ = cr.fill();
        } else {
            // Bereich gilt nur für einzelne Spuren
            for (i, tr) in p.tracks.iter().enumerate() {
                if !st.range_tracks.contains(&tr.id) { continue; }
                cr.rectangle(xa, g.y(i), xb - xa, TRACK_H);
                cr.set_source_rgba(accent.0, accent.1, accent.2, 0.3);
                let _ = cr.fill_preserve();
                cr.set_source_rgba(accent.0, accent.1, accent.2, 0.9);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
        }
    }
    // Hinweisfeld: zeigt beim Ziehen eines Effekts, was gerade eingestellt wird
    if let Some(d) = drag.filter(|d| d.mode.is_effect()) {
        if let (Some(c), Some(ti)) = (p.clip(d.clip), p.clip(d.clip).and_then(|c| p.track_index(c.track))) {
            let text = effect_text(c, d.mode, p.fps);
            let ax = match d.mode {
                TMode::FadeIn => g.x(c.start) + c.fade_in * g.pps,
                TMode::FadeOut => g.x(c.end()) - c.fade_out * g.pps,
                TMode::AnimMove(i) | TMode::AnimEnd(i) => c.anims.get(i).map(|a| { let (x, bw) = anim_bar(&g, c, a); x + bw }).unwrap_or(0.0),
                _ => 0.0,
            };
            let bw = text.chars().count() as f64 * 6.3 + 18.0;
            let bx = (ax - bw / 2.0).clamp(HEADER_W + 4.0, (w - bw - 4.0).max(HEADER_W + 4.0));
            let top = g.y(ti);
            let by = if top - 26.0 > RULER_H { top - 25.0 } else { top + TRACK_H + 3.0 };
            super::rounded_rect(cr, bx, by, bw, 22.0, 6.0);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.88);
            let _ = cr.fill_preserve();
            cr.set_source_rgb(accent.0, accent.1, accent.2);
            cr.set_line_width(1.0);
            let _ = cr.stroke();
            super::draw_text(cr, &text, bx + 9.0, by + 4.0, 11.0, (1.0, 1.0, 1.0, 1.0), true, 0.0);
            // Hilfslinie an der Stelle, die gezogen wird
            cr.set_source_rgba(accent.0, accent.1, accent.2, 0.8);
            cr.move_to(ax.round() + 0.5, top);
            cr.line_to(ax.round() + 0.5, top + TRACK_H);
            let _ = cr.stroke();
        }
    }
    let _ = cr.restore();

    // Zeitlineal
    cr.rectangle(0.0, 0.0, w, RULER_H);
    super::theme::set(cr, "flaeche");
    let _ = cr.fill();
    let _ = cr.save();
    cr.rectangle(HEADER_W, 0.0, w - HEADER_W, RULER_H);
    cr.clip();
    if let Some((a, b)) = st.range {
        let (xa, xb) = (g.x(a.min(b)), g.x(a.max(b)));
        cr.rectangle(xa, 0.0, xb - xa, RULER_H);
        cr.set_source_rgba(accent.0, accent.1, accent.2, 0.5);
        let _ = cr.fill();
    }
    let step = ruler_step(g.pps);
    let mut t = (g.scroll / step).floor() * step;
    while g.x(t) < w {
        let x = g.x(t).round() + 0.5;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
        cr.set_line_width(1.0);
        cr.move_to(x, RULER_H - 9.0);
        cr.line_to(x, RULER_H);
        let _ = cr.stroke();
        for k in 1..5 {
            let xm = g.x(t + step * k as f64 / 5.0).round() + 0.5;
            cr.move_to(xm, RULER_H - 4.0);
            cr.line_to(xm, RULER_H);
        }
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.18);
        let _ = cr.stroke();
        let label = if step < 1.0 { format!("{}:{:04.1}", (t / 60.0).floor() as i64, t % 60.0) } else { format_dur(t) };
        super::draw_text(cr, &label, x + 4.0, 5.0, 10.5, (0.75, 0.76, 0.8, 1.0), false, 0.0);
        t += step;
    }
    let _ = cr.restore();

    // Spurköpfe
    cr.rectangle(0.0, RULER_H, HEADER_W, h - RULER_H);
    super::theme::set(cr, "grund");
    let _ = cr.fill();
    let _ = cr.save();
    cr.rectangle(0.0, RULER_H, HEADER_W, h - RULER_H);
    cr.clip();
    for i in 0..=g.n {
        let y = g.y(i);
        if y + TRACK_H < RULER_H || y > h { continue; }
        if i == g.n {
            super::draw_text(cr, "Neue Spur: Clip hierher ziehen", 10.0, y + TRACK_H / 2.0 - 7.0, 10.5, (0.5, 0.51, 0.56, 1.0), false, HEADER_W - 16.0);
            continue;
        }
        let tr = &p.tracks[i];
        cr.rectangle(0.0, y, HEADER_W - 1.0, TRACK_H);
        super::theme::set(cr, "flaeche");
        let _ = cr.fill();
        super::draw_text(cr, &tr.name, 10.0, y + 8.0, 12.0, (0.92, 0.92, 0.95, 1.0), true, HEADER_W - 18.0);
        let icons_y = y + TRACK_H - 24.0;
        let items: [(&str, &str, bool); 3] = [("eye", "eye-off", tr.hidden), ("speaker", "mute", tr.muted), ("lock", "lock", tr.locked)];
        for (k, (on, off, active)) in items.iter().enumerate() {
            let ix = 10.0 + k as f64 * 26.0;
            let _ = cr.save();
            cr.translate(ix, icons_y);
            if *active { icons::draw(cr, off, 16.0, (accent.0, accent.1, accent.2, 1.0)); } else { icons::draw(cr, on, 16.0, (1.0, 1.0, 1.0, 0.4)); }
            let _ = cr.restore();
        }
    }
    let _ = cr.restore();
    cr.rectangle(0.0, 0.0, HEADER_W, RULER_H);
    super::theme::set(cr, "grund");
    let _ = cr.fill();
    super::draw_text(cr, "Spuren", 10.0, 7.0, 10.5, (0.55, 0.56, 0.61, 1.0), true, 0.0);

    // Schnittlinie der Schere: dünn über alles, kräftig auf den Ebenen, die geschnitten werden
    if let Some((t, track, all)) = st.cut_hover.filter(|_| st.tl_cut && st.tl_drag.is_none()) {
        let x = g.x(t).round() + 0.5;
        if x >= HEADER_W {
            let _ = cr.save();
            cr.rectangle(HEADER_W, 0.0, w - HEADER_W, h);
            cr.clip();
            cr.set_source_rgba(CUT.0, CUT.1, CUT.2, 0.5);
            cr.set_line_width(1.0);
            cr.set_dash(&[4.0, 3.0], 0.0);
            cr.move_to(x, 0.0);
            cr.line_to(x, h);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
            let _ = cr.save();
            cr.rectangle(HEADER_W, RULER_H, w - HEADER_W, h - RULER_H);
            cr.clip();
            cr.set_source_rgb(CUT.0, CUT.1, CUT.2);
            cr.set_line_width(2.5);
            for (i, tr) in p.tracks.iter().enumerate() {
                if tr.locked || !(all || tr.id == track) { continue; }
                cr.move_to(x, g.y(i));
                cr.line_to(x, g.y(i) + TRACK_H);
            }
            let _ = cr.stroke();
            let _ = cr.restore();
            // Schere und Zeit im Lineal
            let label = format_time(t, p.fps);
            let bw = label.chars().count() as f64 * 6.3 + 30.0;
            let bx = if x + 6.0 + bw < w { x + 6.0 } else { x - 6.0 - bw };
            super::rounded_rect(cr, bx, 3.0, bw, RULER_H - 6.0, 5.0);
            cr.set_source_rgb(CUT.0, CUT.1, CUT.2);
            let _ = cr.fill();
            let _ = cr.save();
            cr.translate(bx + 4.0, 5.0);
            icons::draw(cr, "scissors", 16.0, (0.1, 0.02, 0.02, 1.0));
            let _ = cr.restore();
            super::draw_text(cr, &label, bx + 24.0, 6.0, 11.0, (0.1, 0.02, 0.02, 1.0), true, 0.0);
            let _ = cr.restore();
        }
    }

    // Abspielkopf
    let px = g.x(st.playhead).round() + 0.5;
    if px >= HEADER_W {
        cr.set_source_rgb(accent.0, accent.1, accent.2);
        cr.set_line_width(1.5);
        cr.move_to(px, RULER_H - 2.0);
        cr.line_to(px, h);
        let _ = cr.stroke();
        cr.move_to(px - 7.0, 4.0);
        cr.line_to(px + 7.0, 4.0);
        cr.line_to(px + 7.0, RULER_H - 12.0);
        cr.line_to(px, RULER_H - 2.0);
        cr.line_to(px - 7.0, RULER_H - 12.0);
        cr.close_path();
        let _ = cr.fill();
    }
}

fn clip_at(ed: &Editor, g: &Geo, x: f64, y: f64) -> Option<(u64, TMode)> {
    let st = ed.st.borrow();
    let ti = g.track_at(y)?;
    let tr = st.project.tracks.get(ti)?;
    if tr.locked { return None; }
    let ty = g.y(ti);
    if y < ty || y > ty + TRACK_H { return None; }
    let mut found: Option<&Clip> = None;
    for c in st.project.clips_on(tr.id) {
        let (x0, x1) = (g.x(c.start), g.x(c.end()));
        if x >= x0 - 2.0 && x <= x1 + 2.0 {
            // bei Überblendungen den ausgewählten bzw. späteren Clip bevorzugen
            match found {
                Some(f) if st.sel.contains(&f.id) && !st.sel.contains(&c.id) => {}
                _ => found = Some(c),
            }
        }
    }
    let c = found?;
    let (x0, x1) = (g.x(c.start), g.x(c.end()));
    if let Some((ha, hb)) = fade_handles(g, c) {
        if y - ty <= HANDLE_Y + 9.0 {
            let (da, db) = ((x - ha).abs(), (x - hb).abs());
            if da <= 8.0 && da <= db { return Some((c.id, TMode::FadeIn)); }
            if db <= 8.0 { return Some((c.id, TMode::FadeOut)); }
        }
    }
    if y >= ty + TRACK_H - 17.0 {
        for (i, a) in c.anims.iter().enumerate().rev() {
            let (ax, aw) = anim_bar(g, c, a);
            if x >= ax - 2.0 && x <= ax + aw + 8.0 {
                return Some((c.id, if x >= ax + aw - 5.0 { TMode::AnimEnd(i) } else { TMode::AnimMove(i) }));
            }
        }
    }
    let mode = if x1 - x0 > 18.0 && x - x0 < 7.0 { TMode::TrimL } else if x1 - x0 > 18.0 && x1 - x < 7.0 { TMode::TrimR } else { TMode::Move };
    Some((c.id, mode))
}

/// Einrastpunkte: Anfang, Abspielkopf, Bereich und Kanten der übrigen Clips.
fn snap_points(st: &super::editor::State, exclude: &[u64]) -> Vec<f64> {
    let mut v = vec![0.0, st.playhead];
    if let Some((a, b)) = st.range { v.push(a); v.push(b); }
    for c in &st.project.clips {
        if exclude.contains(&c.id) { continue; }
        v.push(c.start);
        v.push(c.end());
    }
    v
}
fn snap_to(points: &[f64], t: f64, tol: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for p in points {
        if (p - t).abs() <= tol && best.map(|b| (p - t).abs() < (b - t).abs()).unwrap_or(true) { best = Some(*p); }
    }
    best
}

fn drag_update(ed: &Editor, dx: f64, dy: f64) {
    let Some(d) = ed.st.borrow().tl_drag.clone() else { return };
    let g = Geo::of(ed);
    let x = d.start.0 + dx;
    let y = d.start.1 + dy;
    match d.mode {
        TMode::Scrub => { ed.seek(g.t(x)); return; }
        TMode::Cut => {
            if dx.abs() > 4.0 {
                let (fps, ph) = { let st = ed.st.borrow(); (st.project.fps, st.playhead) };
                let t = cut_time(&g, x, fps, ph);
                let mut st = ed.st.borrow_mut();
                st.range = Some((d.start_time.min(t), d.start_time.max(t)));
                st.range_tracks = if d.clip == 0 { vec![] } else { vec![d.clip] };
                st.cut_hover = None;
                if let Some(sd) = st.tl_drag.as_mut() { sd.moved = true; }
            }
            ed.timeline.queue_draw();
            return;
        }
        TMode::Range => {
            let fps = ed.st.borrow().project.fps;
            let t = snap_frame(g.t(x), fps);
            ed.st.borrow_mut().range_tracks.clear();
            ed.st.borrow_mut().range = if (t - d.start_time).abs() * g.pps > 3.0 { Some((d.start_time.min(t), d.start_time.max(t))) } else { None };
            ed.timeline.queue_draw();
            return;
        }
        _ => {}
    }
    if !d.moved {
        if dx.abs() < 3.0 && dy.abs() < 3.0 { return; }
        ed.begin_change();
        if let Some(sd) = ed.st.borrow_mut().tl_drag.as_mut() { sd.moved = true; }
    }
    if d.mode.is_effect() {
        let fps = ed.st.borrow().project.fps;
        let Some(main) = d.origs.iter().find(|c| c.id == d.clip).cloned() else { return };
        let delta = dx / g.pps;
        let frame = 1.0 / fps.max(1) as f64;
        ed.modify(true, |p| {
            let Some(c) = p.clip_mut(main.id) else { return };
            match d.mode {
                TMode::FadeIn => {
                    let v = snap_frame((main.fade_in + delta).clamp(0.0, (main.duration - main.fade_out).max(0.0)), fps);
                    c.fade_in = if v < frame * 1.5 { 0.0 } else { v };
                }
                TMode::FadeOut => {
                    let v = snap_frame((main.fade_out - delta).clamp(0.0, (main.duration - main.fade_in).max(0.0)), fps);
                    c.fade_out = if v < frame * 1.5 { 0.0 } else { v };
                }
                TMode::AnimMove(i) => {
                    if let (Some(a), Some(o)) = (c.anims.get_mut(i), main.anims.get(i)) {
                        let hi = (main.inpoint + main.duration - o.dur).max(main.inpoint);
                        a.t = main.inpoint + snap_frame((o.t + delta).clamp(main.inpoint, hi) - main.inpoint, fps);
                    }
                }
                TMode::AnimEnd(i) => {
                    if let (Some(a), Some(o)) = (c.anims.get_mut(i), main.anims.get(i)) {
                        let hi = (main.inpoint + main.duration - o.t).max(0.0);
                        a.dur = snap_frame((o.dur + delta).clamp(0.0, hi), fps);
                    }
                }
                _ => {}
            }
        });
        ed.refresh_props();
        return;
    }
    let ids: Vec<u64> = d.origs.iter().map(|c| c.id).collect();
    let (points, fps) = { let st = ed.st.borrow(); (snap_points(&st, &ids), st.project.fps) };
    let tol = 8.0 / g.pps;
    let delta = dx / g.pps;
    let Some(main) = d.origs.iter().find(|c| c.id == d.clip).cloned() else { return };
    let min_dur = 1.0 / fps.max(1) as f64;
    let mut new_track_flag = false;
    ed.modify(false, |p| {
        // vorherigen Zustand merken, falls die neue Lage ungültig ist
        let before: Vec<Clip> = ids.iter().filter_map(|id| p.clip(*id).cloned()).collect();
        match d.mode {
            TMode::Move => {
                let mut dl = delta;
                let s = main.start + dl;
                let cand_a = snap_to(&points, s, tol).map(|v| v - s);
                let cand_b = snap_to(&points, s + main.duration, tol).map(|v| v - (s + main.duration));
                match (cand_a, cand_b) {
                    (Some(a), Some(b)) => dl += if a.abs() <= b.abs() { a } else { b },
                    (Some(a), None) => dl += a,
                    (None, Some(b)) => dl += b,
                    _ => {}
                }
                let min_start = d.origs.iter().map(|c| c.start).fold(f64::INFINITY, f64::min);
                if min_start + dl < 0.0 { dl = -min_start; }
                // Spurwechsel nur bei einem einzelnen Clip
                let mut target: Option<u64> = None;
                if d.origs.len() == 1 {
                    match g.track_at(y) {
                        Some(i) if i == g.n => new_track_flag = true,
                        Some(i) => { if let Some(t) = p.tracks.get(i) { if !t.locked { target = Some(t.id); } } }
                        None => {}
                    }
                }
                for o in &d.origs {
                    if let Some(c) = p.clip_mut(o.id) {
                        c.start = snap_frame(o.start + dl, fps);
                        if let Some(t) = target { c.track = t; }
                    }
                }
                let ok = |p: &Project| ids.iter().all(|id| p.clip(*id).map(|c| p.placement_ok(c)).unwrap_or(true));
                if !ok(p) {
                    // ohne Spurwechsel versuchen
                    for o in &d.origs { if let Some(c) = p.clip_mut(o.id) { c.track = before.iter().find(|b| b.id == o.id).map(|b| b.track).unwrap_or(o.track); } }
                    if !ok(p) {
                        for b in &before { if let Some(c) = p.clip_mut(b.id) { c.start = b.start; c.track = b.track; } }
                    }
                }
            }
            TMode::TrimL => {
                let mut s = main.start + delta;
                if let Some(v) = snap_to(&points, s, tol) { s = v; }
                let time_based = matches!(main.kind, Kind::Video | Kind::Audio);
                let lo = if time_based { (main.start - main.inpoint).max(0.0) } else { 0.0 };
                s = snap_frame(s.clamp(lo, main.end() - min_dur), fps);
                if let Some(c) = p.clip_mut(main.id) {
                    c.start = s;
                    c.duration = main.end() - s;
                    c.inpoint = (main.inpoint + (s - main.start)).max(0.0);
                }
                let valid = p.clip(main.id).map(|c| p.placement_ok(c)).unwrap_or(true);
                if !valid { if let (Some(c), Some(b)) = (p.clip_mut(main.id), before.first()) { c.start = b.start; c.duration = b.duration; c.inpoint = b.inpoint; } }
            }
            TMode::TrimR => {
                let mut e = main.end() + delta;
                if let Some(v) = snap_to(&points, e, tol) { e = v; }
                let time_based = matches!(main.kind, Kind::Video | Kind::Audio) && main.media_duration > 0.0;
                let hi = if time_based { main.start + (main.media_duration - main.inpoint) } else { f64::INFINITY };
                e = snap_frame(e.clamp(main.start + min_dur, hi), fps);
                if let Some(c) = p.clip_mut(main.id) { c.duration = e - main.start; }
                let valid = p.clip(main.id).map(|c| p.placement_ok(c)).unwrap_or(true);
                if !valid { if let (Some(c), Some(b)) = (p.clip_mut(main.id), before.first()) { c.duration = b.duration; } }
            }
            _ => {}
        }
    });
    if let Some(sd) = ed.st.borrow_mut().tl_drag.as_mut() { sd.new_track = new_track_flag; }
}

pub fn zoom(ed: &Editor, factor: f64, anchor_x: Option<f64>) {
    {
        let g = Geo::of(ed);
        let mut st = ed.st.borrow_mut();
        let ax = anchor_x.unwrap_or_else(|| g.x(st.playhead).clamp(HEADER_W, ed.timeline.width() as f64));
        let t = g.t(ax);
        st.tl_pps = (st.tl_pps * factor).clamp(1.0, 2000.0);
        st.tl_scroll = (t - (ax - HEADER_W) / st.tl_pps).max(0.0);
    }
    update_scrollbar(ed);
    ed.timeline.queue_draw();
}

pub fn zoom_fit(ed: &Editor) {
    {
        let mut st = ed.st.borrow_mut();
        let w = (ed.timeline.width() as f64 - HEADER_W - 40.0).max(200.0);
        let d = (st.project.duration() * 1.15).max(5.0);
        st.tl_pps = (w / d).clamp(1.0, 2000.0);
        st.tl_scroll = 0.0;
    }
    update_scrollbar(ed);
    ed.timeline.queue_draw();
}

pub fn follow_playhead(ed: &Editor) {
    let w = ed.timeline.width() as f64;
    let mut st = ed.st.borrow_mut();
    let x = HEADER_W + (st.playhead - st.tl_scroll) * st.tl_pps;
    let visible = (w - HEADER_W) / st.tl_pps;
    if x > w - 20.0 {
        st.tl_scroll = st.playhead - visible * 0.1;
    } else if x < HEADER_W {
        st.tl_scroll = (st.playhead - visible * 0.1).max(0.0);
    } else {
        return;
    }
    drop(st);
    update_scrollbar(ed);
}

pub fn update_scrollbar(ed: &Editor) {
    let st = ed.st.borrow();
    let visible = ((ed.timeline.width() as f64 - HEADER_W) / st.tl_pps).max(1.0);
    let upper = (st.project.duration() * 1.1 + 5.0).max(st.tl_scroll + visible);
    let a = ed.tl_scrollbar.adjustment();
    ed.updating_scroll_set(true);
    a.configure(st.tl_scroll, 0.0, upper, visible / 10.0, visible * 0.8, visible);
    ed.updating_scroll_set(false);
}

pub fn setup(ed: &Rc<Editor>) {
    {
        let weak = Rc::downgrade(ed);
        ed.timeline.set_draw_func(move |_, cr, w, h| { if let Some(e) = weak.upgrade() { draw(&e, cr, w, h); } });
    }
    let drag = gtk::GestureDrag::new();
    {
        let weak = Rc::downgrade(ed);
        drag.connect_drag_begin(move |gst, x, y| {
            let Some(e) = weak.upgrade() else { return };
            e.timeline.grab_focus();
            let g = Geo::of(&e);
            let state = gst.current_event_state();
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let fps = e.st.borrow().project.fps;
            let t = snap_frame(g.t(x), fps);
            let mk = |mode: TMode, clip: u64, origs: Vec<Clip>| Drag { mode, clip, origs, start: (x, y), start_time: t, moved: false, new_track: false };
            if x >= HEADER_W && e.st.borrow().tl_cut {
                // Schere: Klick schneidet, Ziehen markiert einen Bereich
                let ph = e.st.borrow().playhead;
                let (track, _) = cut_target(&e, &g, y, shift);
                let tc = cut_time(&g, x, fps, ph);
                if e.engine.borrow().is_playing() { e.pause(); }
                e.st.borrow_mut().tl_drag = Some(Drag { mode: TMode::Cut, clip: track, origs: vec![], start: (x, y), start_time: tc, moved: false, new_track: false });
                return;
            }
            if y < RULER_H {
                if x < HEADER_W { return; }
                if shift {
                    e.st.borrow_mut().tl_drag = Some(mk(TMode::Range, 0, vec![]));
                } else {
                    e.st.borrow_mut().tl_drag = Some(mk(TMode::Scrub, 0, vec![]));
                    if e.engine.borrow().is_playing() { e.pause(); }
                    e.seek(t);
                }
                return;
            }
            if x < HEADER_W {
                // Schalter im Spurkopf
                if let Some(i) = g.track_at(y) {
                    if i < g.n {
                        let ty = g.y(i);
                        if y >= ty + TRACK_H - 28.0 && y <= ty + TRACK_H {
                            let k = ((x - 6.0) / 26.0).floor() as i64;
                            if (0..3).contains(&k) {
                                e.edit(|p| {
                                    if let Some(tr) = p.tracks.get_mut(i) {
                                        match k { 0 => tr.hidden = !tr.hidden, 1 => tr.muted = !tr.muted, _ => tr.locked = !tr.locked }
                                    }
                                });
                            }
                        }
                    }
                }
                return;
            }
            match clip_at(&e, &g, x, y) {
                Some((id, mode)) => {
                    e.select(Some(id), ctrl);
                    let origs: Vec<Clip> = { let st = e.st.borrow(); st.project.clips.iter().filter(|c| st.sel.contains(&c.id)).cloned().collect() };
                    if origs.iter().any(|c| c.id == id) {
                        e.st.borrow_mut().tl_drag = Some(mk(mode, id, origs));
                        if mode.is_effect() {
                            // rechts die passenden Einstellungen zeigen
                            // erst nach dem Neuaufbau der Leiste rollen, sonst stimmen die Maße noch nicht
                            let pr = e.props.borrow().clone();
                            let anim = matches!(mode, TMode::AnimMove(_) | TMode::AnimEnd(_));
                            glib::timeout_add_local_once(std::time::Duration::from_millis(60), move || { if let Some(pr) = pr { pr.reveal(anim); } });
                            e.timeline.queue_draw();
                        }
                    }
                }
                None => {
                    e.select(None, false);
                    if e.engine.borrow().is_playing() { e.pause(); }
                    e.seek(t);
                    e.st.borrow_mut().tl_drag = Some(mk(TMode::Scrub, 0, vec![]));
                }
            }
        });
    }
    {
        let weak = Rc::downgrade(ed);
        drag.connect_drag_update(move |_, dx, dy| { if let Some(e) = weak.upgrade() { drag_update(&e, dx, dy); } });
    }
    {
        let weak = Rc::downgrade(ed);
        drag.connect_drag_end(move |_, _, _| {
            let Some(e) = weak.upgrade() else { return };
            let d = e.st.borrow_mut().tl_drag.take();
            let Some(d) = d else { return };
            if d.mode == TMode::Cut {
                if d.moved {
                    let frame = 1.0 / e.st.borrow().project.fps.max(1) as f64;
                    let small = e.st.borrow().range.map(|(a, b)| (b - a).abs() < frame).unwrap_or(true);
                    if small {
                        let mut st = e.st.borrow_mut();
                        st.range = None;
                        st.range_tracks.clear();
                    } else {
                        e.select(None, false);
                        e.show_note("Bereich markiert. „Lücke schließen“ nimmt ihn heraus und fügt zusammen, Entf nimmt ihn nur heraus.");
                    }
                } else if d.clip == u64::MAX {
                    e.show_note("Auf dieser Zeile liegt keine Ebene, die geschnitten werden kann.");
                } else {
                    let tracks: Vec<u64> = if d.clip == 0 { vec![] } else { vec![d.clip] };
                    e.cut_at(d.start_time, &tracks);
                }
                e.timeline.queue_draw();
                return;
            }
            if d.mode.is_effect() {
                if d.moved {
                    if matches!(d.mode, TMode::AnimMove(_)) {
                        if let Some(c) = e.st.borrow_mut().project.clip_mut(d.clip) { c.anims.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap()); }
                    }
                    e.changed();
                } else if let TMode::AnimMove(i) | TMode::AnimEnd(i) = d.mode {
                    // Klick auf einen Balken: ans Ende der Bewegung springen, dort lässt sich ihr Ziel in der Vorschau einstellen
                    let t = { let st = e.st.borrow(); st.project.clip(d.clip).and_then(|c| c.anims.get(i).map(|a| c.start + a.t - c.inpoint + a.dur)) };
                    if let Some(t) = t { if e.engine.borrow().is_playing() { e.pause(); } e.seek(t); }
                    e.timeline.queue_draw();
                } else {
                    e.timeline.queue_draw();
                }
                return;
            }
            if d.moved && matches!(d.mode, TMode::Move | TMode::TrimL | TMode::TrimR) {
                if d.new_track {
                    let mut st = e.st.borrow_mut();
                    let n = st.project.tracks.len() + 1;
                    let tid = st.project.add_track(&format!("Spur {n}"));
                    if let Some(c) = st.project.clip_mut(d.clip) { c.track = tid; }
                }
                ops::prune_tracks(&mut e.st.borrow_mut().project);
                e.changed();
            } else {
                e.timeline.queue_draw();
            }
        });
    }
    ed.timeline.add_controller(drag);

    let motion = gtk::EventControllerMotion::new();
    let pos = Rc::new(std::cell::Cell::new((0.0f64, 0.0f64)));
    {
        let weak = Rc::downgrade(ed);
        let pos = pos.clone();
        motion.connect_motion(move |ctl, x, y| {
            pos.set((x, y));
            let Some(e) = weak.upgrade() else { return };
            if e.st.borrow().tl_drag.is_some() { return; }
            let g = Geo::of(&e);
            if e.st.borrow().tl_cut {
                // Schnittlinie folgt der Maus
                let shift = ctl.current_event_state().contains(gdk::ModifierType::SHIFT_MASK);
                let hover = if x >= HEADER_W {
                    let (fps, ph) = { let st = e.st.borrow(); (st.project.fps, st.playhead) };
                    let (track, all) = cut_target(&e, &g, y, shift);
                    Some((cut_time(&g, x, fps, ph), track, all))
                } else { None };
                e.st.borrow_mut().cut_hover = hover;
                e.timeline.set_cursor_from_name(Some(if hover.is_some() { "crosshair" } else { "default" }));
                e.timeline.queue_draw();
                return;
            }
            let hit = if y < RULER_H || x < HEADER_W { None } else { clip_at(&e, &g, x, y) };
            let cursor = match hit {
                Some((_, TMode::TrimL)) | Some((_, TMode::TrimR)) | Some((_, TMode::AnimEnd(_))) => "ew-resize",
                Some((_, TMode::FadeIn)) | Some((_, TMode::FadeOut)) => "col-resize",
                Some(_) => "grab",
                None => "default",
            };
            e.timeline.set_cursor_from_name(Some(cursor));
        });
    }
    {
        let weak = Rc::downgrade(ed);
        motion.connect_leave(move |_| {
            let Some(e) = weak.upgrade() else { return };
            if e.st.borrow().cut_hover.is_some() { e.st.borrow_mut().cut_hover = None; e.timeline.queue_draw(); }
        });
    }
    ed.timeline.add_controller(motion);
    // Hinweis an Griffen und Balken; wird bei jeder Anzeige frisch bestimmt
    ed.timeline.set_has_tooltip(true);
    {
        let weak = Rc::downgrade(ed);
        ed.timeline.connect_query_tooltip(move |_, x, y, _, tip| {
            let Some(e) = weak.upgrade() else { return false };
            let (x, y) = (x as f64, y as f64);
            if e.st.borrow().tl_drag.is_some() || e.st.borrow().tl_cut || y < RULER_H || x < HEADER_W { return false; }
            let g = Geo::of(&e);
            let text = match clip_at(&e, &g, x, y) {
                Some((_, TMode::FadeIn)) => "Anfang: ziehen stellt die Dauer ein",
                Some((_, TMode::FadeOut)) => "Ende: ziehen stellt die Dauer ein",
                Some((_, TMode::AnimMove(_))) => "Bewegung: ziehen verschiebt sie, Klick springt an ihr Ende",
                Some((_, TMode::AnimEnd(_))) => "Ende der Bewegung: ziehen macht sie schneller oder langsamer",
                _ => return false,
            };
            tip.set_text(Some(text));
            true
        });
    }

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    {
        let weak = Rc::downgrade(ed);
        let pos = pos.clone();
        scroll.connect_scroll(move |c, dx, dy| {
            let Some(e) = weak.upgrade() else { return glib::Propagation::Proceed };
            let state = c.current_event_state();
            if state.contains(gdk::ModifierType::CONTROL_MASK) {
                zoom(&e, if dy < 0.0 { 1.25 } else { 0.8 }, Some(pos.get().0.max(HEADER_W)));
                return glib::Propagation::Stop;
            }
            let g = Geo::of(&e);
            let h = e.timeline.height() as f64;
            let overflow = g.content_h() > h + 1.0;
            let mut st = e.st.borrow_mut();
            if dx.abs() > 0.0 || state.contains(gdk::ModifierType::SHIFT_MASK) || !overflow {
                let d = if dx.abs() > 0.0 { dx } else { dy };
                st.tl_scroll = (st.tl_scroll + d * 60.0 / st.tl_pps).max(0.0);
            } else {
                st.tl_vscroll = (st.tl_vscroll + dy * 30.0).clamp(0.0, (g.content_h() - h).max(0.0));
            }
            drop(st);
            update_scrollbar(&e);
            e.timeline.queue_draw();
            glib::Propagation::Stop
        });
    }
    ed.timeline.add_controller(scroll);

    // Dateien oder Medien aus der Liste in die Zeitleiste ziehen
    let drop = gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
    drop.set_types(&[gdk::FileList::static_type(), glib::Type::STRING]);
    {
        let weak = Rc::downgrade(ed);
        drop.connect_drop(move |_, value, x, y| {
            let Some(e) = weak.upgrade() else { return false };
            let g = Geo::of(&e);
            let fps = e.st.borrow().project.fps;
            let t = if x > HEADER_W { snap_frame(g.t(x), fps) } else { e.st.borrow().playhead };
            let track = g.track_at(y).and_then(|i| e.st.borrow().project.tracks.get(i).map(|t| t.id));
            if let Ok(list) = value.get::<gdk::FileList>() {
                let paths: Vec<String> = list.files().iter().filter_map(|f| f.path()).map(|p| p.to_string_lossy().to_string()).collect();
                for i in e.import_files(&paths, false) { e.place_media(i, t, track, None); }
                return true;
            }
            if let Ok(s) = value.get::<String>() {
                if let Some(i) = s.strip_prefix("wsmedia:").and_then(|n| n.parse::<usize>().ok()) {
                    e.place_media(i, t, track, None);
                    return true;
                }
                // Kachel aus Übergänge, Animationen oder Effekte auf einen Clip gezogen
                if let Some(action) = s.strip_prefix("wsfx:") {
                    if action == "pixelate" { e.quick_action(action); return true; }
                    match clip_at(&e, &g, x, y) {
                        Some((id, _)) => {
                            e.select(Some(id), false);
                            // Zoom und Bewegung beginnen an der Stelle, an der losgelassen wurde
                            if matches!(action, "zoomin" | "zoomout" | "animcustom") {
                                if e.engine.borrow().is_playing() { e.pause(); }
                                e.seek(t);
                            }
                            e.quick_action(action);
                        }
                        None => e.show_note("Zieh die Kachel auf einen Clip in der Zeitleiste."),
                    }
                    return true;
                }
            }
            false
        });
    }
    ed.timeline.add_controller(drop);
}
