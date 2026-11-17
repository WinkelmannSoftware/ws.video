//! Arbeitsfläche: Vorschau des Bilds mit Auswahlrahmen, Verschieben, Skalieren und Zuschneiden.
use super::editor::{Editor, Tool};
use crate::model::*;
use gtk::prelude::*;
use gtk::{cairo, gdk, glib};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Move,
    Resize(usize),
    Crop(usize),
    Pan,
}

#[derive(Clone, Debug)]
pub struct Drag {
    pub mode: Mode,
    pub clip: u64,
    pub orig: Clip,
    /// Quellzeit des Clips beim Start (bestimmt, welche Animation bearbeitet wird)
    pub st_time: f64,
    pub start_pan: (f64, f64),
    pub moved: bool,
    pub live: bool,
}

#[derive(Clone, Copy)]
pub struct View {
    pub s: f64,
    pub ox: f64,
    pub oy: f64,
}
impl View {
    pub fn to_widget(&self, x: f64, y: f64) -> (f64, f64) { (x * self.s + self.ox, y * self.s + self.oy) }
    pub fn to_project(&self, x: f64, y: f64) -> (f64, f64) { ((x - self.ox) / self.s, (y - self.oy) / self.s) }
}

pub fn view(ed: &Editor) -> View {
    let (w, h) = (ed.canvas.width() as f64, ed.canvas.height() as f64);
    let st = ed.st.borrow();
    let (pw, ph) = (st.project.width.max(1) as f64, st.project.height.max(1) as f64);
    let fit = ((w - 40.0) / pw).min((h - 40.0) / ph).max(0.01);
    let s = if st.view_zoom > 0.0 { st.view_zoom } else { fit };
    View { s, ox: (w - pw * s) / 2.0 + st.view_pan.0, oy: (h - ph * s) / 2.0 + st.view_pan.1 }
}

/// Rechteck, das auf der Arbeitsfläche bearbeitet wird: Ziel der gerade gültigen Animation, sonst die Grundposition.
pub fn target_rect(c: &Clip, st_time: f64) -> (f64, f64, f64, f64) {
    match c.anim_index_at(st_time) {
        Some(i) => { let a = &c.anims[i]; (a.x, a.y, a.w, a.h) }
        None => (c.x, c.y, c.w, c.h),
    }
}
pub fn set_target_rect(c: &mut Clip, st_time: f64, r: (f64, f64, f64, f64)) {
    match c.anim_index_at(st_time) {
        Some(i) => { let a = &mut c.anims[i]; a.x = r.0; a.y = r.1; a.w = r.2; a.h = r.3; }
        None => { c.x = r.0; c.y = r.1; c.w = r.2; c.h = r.3; }
    }
}
pub fn offset_rect(c: &mut Clip, st_time: f64, dx: f64, dy: f64) {
    let r = target_rect(c, st_time);
    set_target_rect(c, st_time, (r.0 + dx, r.1 + dy, r.2, r.3));
}

/// Griffe im Uhrzeigersinn ab links oben: 0 LO, 1 O, 2 RO, 3 R, 4 RU, 5 U, 6 LU, 7 L
fn handle_points(r: (f64, f64, f64, f64)) -> [(f64, f64); 8] {
    let (x, y, w, h) = r;
    [(x, y), (x + w / 2.0, y), (x + w, y), (x + w, y + h / 2.0), (x + w, y + h), (x + w / 2.0, y + h), (x, y + h), (x, y + h / 2.0)]
}
/// Seitengriffe gibt es nur, wenn die Kante auf dem Bildschirm lang genug dafür ist.
fn handle_shown(i: usize, w: f64, h: f64) -> bool {
    match i { 1 | 5 => w >= 44.0, 3 | 7 => h >= 44.0, _ => true }
}
/// Griff unter dem Zeiger (Rechteck und Zeiger in Bildschirmpixeln).
/// Bei kleinen Objekten wie einem Icon bleibt die Mitte frei: Sonst lägen die Griffe über dem ganzen Objekt,
/// und jeder Versuch, es zu verschieben, würde die Größe ändern. Mindestens 24 Pixel in der Mitte gehören dem
/// Verschieben; ist das Objekt kleiner, greifen die Griffe nur noch außerhalb. Nach außen bleibt alles gleich.
fn handle_at(r: (f64, f64, f64, f64), x: f64, y: f64) -> Option<usize> {
    const OUT: f64 = 9.0;
    let (rx, ry, w, h) = r;
    let deep = |size: f64| ((size - 24.0) / 2.0).clamp(0.0, OUT);
    let (inx, iny) = (deep(w), deep(h));
    if x > rx + inx && x < rx + w - inx && y > ry + iny && y < ry + h - iny { return None; }
    for (i, (hx, hy)) in handle_points(r).iter().enumerate() {
        if handle_shown(i, w, h) && (x - hx).abs() <= OUT && (y - hy).abs() <= OUT { return Some(i); }
    }
    None
}
fn handle_cursor(i: usize) -> &'static str {
    match i { 0 | 4 => "nwse-resize", 2 | 6 => "nesw-resize", 1 | 5 => "ns-resize", _ => "ew-resize" }
}

fn clip_at(p: &Project, t: f64, x: f64, y: f64) -> Option<u64> {
    for c in p.visual_clips_at(t) {
        if p.track(c.track).map(|t| t.locked).unwrap_or(false) { continue; }
        let (cx, cy, cw, ch) = c.rect_at(t);
        // schmale Pfeile und Linien bekommen etwas Rand zum Greifen
        let m = if c.kind == Kind::Arrow { 12.0 } else { 0.0 };
        if x >= cx - m && x <= cx + cw + m && y >= cy - m && y <= cy + ch + m { return Some(c.id); }
    }
    None
}

fn full_frame(c: &Clip, r: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let fw = r.2 / (1.0 - c.crop[0] - c.crop[2]).max(0.01);
    let fh = r.3 / (1.0 - c.crop[1] - c.crop[3]).max(0.01);
    (r.0 - c.crop[0] * fw, r.1 - c.crop[1] * fh, fw, fh)
}

fn keeps_aspect(c: &Clip) -> bool { matches!(c.kind, Kind::Video | Kind::Image) }

fn draw(ed: &Editor, cr: &cairo::Context, w: i32, h: i32) {
    let (w, h) = (w as f64, h as f64);
    super::theme::set(cr, "buehne");
    let _ = cr.paint();
    let v = view(ed);
    let st = ed.st.borrow();
    let (pw, ph) = (st.project.width as f64, st.project.height as f64);
    let (x0, y0) = v.to_widget(0.0, 0.0);
    // Arbeitsfläche
    cr.rectangle(x0 - 1.0, y0 - 1.0, pw * v.s + 2.0, ph * v.s + 2.0);
    super::theme::set(cr, "rinne");
    let _ = cr.fill();
    cr.rectangle(x0, y0, pw * v.s, ph * v.s);
    cr.set_source_rgb(0.0, 0.0, 0.0);
    let _ = cr.fill();
    // aktuelles Bild
    let empty = st.project.clips.is_empty();
    if !empty {
        let eng = ed.engine.borrow();
        let serial = eng.frame_serial();
        let mut cache = ed.frame_cache.borrow_mut();
        if cache.0 != serial || cache.1.is_none() {
            cache.1 = eng.with_frame(|f| f.and_then(super::frame_surface));
            cache.0 = serial;
        }
        if let Some(s) = cache.1.as_ref() {
            let _ = cr.save();
            cr.rectangle(x0, y0, pw * v.s, ph * v.s);
            cr.clip();
            cr.translate(x0, y0);
            cr.scale(pw * v.s / s.width() as f64, ph * v.s / s.height() as f64);
            let _ = cr.set_source_surface(s, 0.0, 0.0);
            // Beim Abspielen auf einem hochauflösenden Monitor kostet das weiche Vergrößern jedes Bildes mehr Zeit,
            // als ein Bild dauert, und das im Haupt-Thread der Oberfläche. Dann genügt die schnelle Vergrößerung;
            // das Standbild wird wieder weich gezeichnet.
            let enlarge = pw * v.s / s.width() as f64 * ed.canvas.scale_factor() as f64;
            cr.source().set_filter(if eng.is_playing() && enlarge > 1.25 { cairo::Filter::Fast } else { cairo::Filter::Bilinear });
            let _ = cr.paint();
            let _ = cr.restore();
        }
    } else {
        super::draw_text(cr, "Medien hierher ziehen oder eine Aufnahme starten", x0 + 24.0, y0 + ph * v.s / 2.0 - 10.0, 15.0, (0.6, 0.61, 0.66, 1.0), false, pw * v.s - 48.0);
    }
    let t = st.playhead;
    let accent = super::theme::accent();
    // Weichzeichner-Bereiche und nicht live gezeichnete Clips andeuten
    for c in st.project.visual_clips_at(t) {
        let sel = st.sel.contains(&c.id);
        let (cx, cy, cw, ch) = c.rect_at(t);
        let (wx, wy) = v.to_widget(cx, cy);
        if c.kind == Kind::Blur && !sel {
            cr.set_dash(&[4.0, 4.0], 0.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
            cr.set_line_width(1.0);
            cr.rectangle(wx + 0.5, wy + 0.5, cw * v.s, ch * v.s);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
        }
        if let Some(d) = &st.canvas_drag {
            if d.clip == c.id && !d.live && d.moved {
                cr.set_source_rgba(accent.0, accent.1, accent.2, 0.18);
                cr.rectangle(wx, wy, cw * v.s, ch * v.s);
                let _ = cr.fill();
            }
        }
    }
    // Hilfslinien beim Einrasten
    for (vertical, pos) in &st.guides {
        cr.set_source_rgba(0.35, 0.8, 1.0, 0.9);
        cr.set_line_width(1.0);
        if *vertical {
            let (gx, _) = v.to_widget(*pos, 0.0);
            cr.move_to(gx.round() + 0.5, 0.0);
            cr.line_to(gx.round() + 0.5, h);
        } else {
            let (_, gy) = v.to_widget(0.0, *pos);
            cr.move_to(0.0, gy.round() + 0.5);
            cr.line_to(w, gy.round() + 0.5);
        }
        let _ = cr.stroke();
    }
    // Auswahlrahmen
    let single = st.sel.len() == 1;
    for id in &st.sel {
        let Some(c) = st.project.clip(*id) else { continue };
        if !c.is_visual() || t < c.start - 1e-6 || t > c.end() + 1e-6 { continue; }
        let r = c.rect_at(t);
        let (wx, wy) = v.to_widget(r.0, r.1);
        let (ww, wh) = (r.2 * v.s, r.3 * v.s);
        let crop_mode = st.tool == Tool::Crop && c.kind.is_media();
        if crop_mode {
            let f = full_frame(c, r);
            let (fx, fy) = v.to_widget(f.0, f.1);
            cr.set_dash(&[5.0, 4.0], 0.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.5);
            cr.set_line_width(1.0);
            cr.rectangle(fx + 0.5, fy + 0.5, f.2 * v.s, f.3 * v.s);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
        }
        cr.set_source_rgb(accent.0, accent.1, accent.2);
        cr.set_line_width(if crop_mode { 2.5 } else { 1.5 });
        cr.rectangle(wx + 0.5, wy + 0.5, ww, wh);
        let _ = cr.stroke();
        if single {
            for (i, (hx, hy)) in handle_points((wx, wy, ww, wh)).iter().enumerate() {
                if !handle_shown(i, ww, wh) { continue; }
                if crop_mode {
                    // Zuschneide-Griffe als Balken bzw. Winkel
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    let (bw, bh) = match i { 1 | 5 => (22.0, 6.0), 3 | 7 => (6.0, 22.0), _ => (10.0, 10.0) };
                    cr.rectangle(hx - bw / 2.0, hy - bh / 2.0, bw, bh);
                    let _ = cr.fill_preserve();
                    cr.set_source_rgb(accent.0, accent.1, accent.2);
                    cr.set_line_width(1.5);
                    let _ = cr.stroke();
                } else {
                    // kleine Objekte bekommen kleinere Griffe, damit sie darunter sichtbar bleiben
                    cr.arc(*hx, *hy, if ww.min(wh) < 44.0 { 3.5 } else { 5.5 }, 0.0, std::f64::consts::TAU);
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    let _ = cr.fill_preserve();
                    cr.set_source_rgb(accent.0, accent.1, accent.2);
                    cr.set_line_width(1.5);
                    let _ = cr.stroke();
                }
            }
        }
    }
}

/// Wendet einen Ziehvorgang auf den Clip an. (dx, dy) in Projektpixeln.
fn apply_drag(p: &mut Project, d: &Drag, dx: f64, dy: f64, free: bool, guides: &mut Vec<(bool, f64)>, snap_tol: f64) {
    let (pw, ph) = (p.width as f64, p.height as f64);
    let Some(c) = p.clip_mut(d.clip) else { return };
    let o = target_rect(&d.orig, d.st_time);
    guides.clear();
    match d.mode {
        Mode::Move => {
            let (mut nx, mut ny) = (o.0 + dx, o.1 + dy);
            // an Rändern und Mitte der Arbeitsfläche einrasten
            for (edge, target) in [(nx, 0.0), (nx + o.2 / 2.0, pw / 2.0), (nx + o.2, pw)] {
                if (edge - target).abs() < snap_tol { nx += target - edge; guides.push((true, target)); break; }
            }
            for (edge, target) in [(ny, 0.0), (ny + o.3 / 2.0, ph / 2.0), (ny + o.3, ph)] {
                if (edge - target).abs() < snap_tol { ny += target - edge; guides.push((false, target)); break; }
            }
            set_target_rect(c, d.st_time, (nx.round(), ny.round(), o.2, o.3));
        }
        Mode::Resize(hd) => {
            let left = matches!(hd, 0 | 6 | 7);
            let right = matches!(hd, 2 | 3 | 4);
            let top = matches!(hd, 0 | 1 | 2);
            let bottom = matches!(hd, 4 | 5 | 6);
            let (mut x0, mut y0, mut x1, mut y1) = (o.0, o.1, o.0 + o.2, o.1 + o.3);
            if left { x0 += dx; }
            if right { x1 += dx; }
            if top { y0 += dy; }
            if bottom { y1 += dy; }
            // an den Rändern der Arbeitsfläche einrasten
            if left && x0.abs() < snap_tol { x0 = 0.0; guides.push((true, 0.0)); }
            if right && (x1 - pw).abs() < snap_tol { x1 = pw; guides.push((true, pw)); }
            if top && y0.abs() < snap_tol { y0 = 0.0; guides.push((false, 0.0)); }
            if bottom && (y1 - ph).abs() < snap_tol { y1 = ph; guides.push((false, ph)); }
            let min = 8.0;
            let lock = keeps_aspect(&d.orig) != free;
            if lock && o.2 > 0.0 && o.3 > 0.0 {
                let kx = (x1 - x0) / o.2;
                let ky = (y1 - y0) / o.3;
                let k = match hd { 1 | 5 => ky, 3 | 7 => kx, _ => if (kx - 1.0).abs() > (ky - 1.0).abs() { kx } else { ky } };
                let k = k.max(min / o.2.min(o.3));
                let (nw, nh) = (o.2 * k, o.3 * k);
                // Ankerpunkt: gegenüberliegende Ecke bzw. Kantenmitte
                let nx = if left { o.0 + o.2 - nw } else if right { o.0 } else { o.0 + (o.2 - nw) / 2.0 };
                let ny = if top { o.1 + o.3 - nh } else if bottom { o.1 } else { o.1 + (o.3 - nh) / 2.0 };
                set_target_rect(c, d.st_time, (nx.round(), ny.round(), nw.round(), nh.round()));
            } else {
                if x1 - x0 < min { if left { x0 = x1 - min } else { x1 = x0 + min } }
                if y1 - y0 < min { if top { y0 = y1 - min } else { y1 = y0 + min } }
                set_target_rect(c, d.st_time, (x0.round(), y0.round(), (x1 - x0).round(), (y1 - y0).round()));
            }
        }
        Mode::Crop(hd) => {
            let f = full_frame(&d.orig, o);
            let left = matches!(hd, 0 | 6 | 7);
            let right = matches!(hd, 2 | 3 | 4);
            let top = matches!(hd, 0 | 1 | 2);
            let bottom = matches!(hd, 4 | 5 | 6);
            let (mut x0, mut y0, mut x1, mut y1) = (o.0, o.1, o.0 + o.2, o.1 + o.3);
            let min = 12.0;
            if left { x0 = (x0 + dx).clamp(f.0, x1 - min); }
            if right { x1 = (x1 + dx).clamp(x0 + min, f.0 + f.2); }
            if top { y0 = (y0 + dy).clamp(f.1, y1 - min); }
            if bottom { y1 = (y1 + dy).clamp(y0 + min, f.1 + f.3); }
            c.crop = [((x0 - f.0) / f.2).clamp(0.0, 0.98), ((y0 - f.1) / f.3).clamp(0.0, 0.98), ((f.0 + f.2 - x1) / f.2).clamp(0.0, 0.98), ((f.1 + f.3 - y1) / f.3).clamp(0.0, 0.98)];
            set_target_rect(c, d.st_time, (x0, y0, x1 - x0, y1 - y0));
        }
        Mode::Pan => {}
    }
}

pub fn setup(ed: &Rc<Editor>) {
    {
        let weak = Rc::downgrade(ed);
        ed.canvas.set_draw_func(move |_, cr, w, h| { if let Some(e) = weak.upgrade() { draw(&e, cr, w, h); } });
    }
    // Ziehen mit der linken Maustaste
    let drag = gtk::GestureDrag::new();
    drag.set_button(0);
    {
        let weak = Rc::downgrade(ed);
        // letzter Klick (Zeit in ms, Ort): für den Doppelklick auf Text
        let last_press: Rc<std::cell::Cell<(u32, f64, f64)>> = Rc::new(std::cell::Cell::new((0, -100.0, -100.0)));
        drag.connect_drag_begin(move |g, x, y| {
            let Some(e) = weak.upgrade() else { return };
            e.canvas.grab_focus();
            let button = g.current_button();
            let state = g.current_event_state();
            let now = g.current_event_time();
            let (lt, lx, ly) = last_press.get();
            let double = button == 1 && now.wrapping_sub(lt) < 450 && (x - lx).abs() < 6.0 && (y - ly).abs() < 6.0;
            if std::env::var_os("WS_DEBUG").is_some() { eprintln!("Druck: Taste {button} Zeit {now} zuletzt {lt} Ort {x:.0},{y:.0} doppelt {double}"); }
            last_press.set(if double { (0, -100.0, -100.0) } else { (now, x, y) });
            if double && e.selected_clip().map(|c| c.kind == Kind::Text).unwrap_or(false) {
                // erst nach der Klick-Verarbeitung: sonst holt sich die Vorschau den Fokus zurück
                let p = e.props.borrow().clone();
                if let Some(p) = p { glib::timeout_add_local_once(std::time::Duration::from_millis(60), move || p.focus_text()); }
                return;
            }
            let v = view(&e);
            let (px, py) = v.to_project(x, y);
            let (tool, t, sel, pan) = { let st = e.st.borrow(); (st.tool, st.playhead, st.sel.clone(), st.view_pan) };
            if button == 2 || tool == Tool::Hand {
                e.st.borrow_mut().canvas_drag = Some(Drag { mode: Mode::Pan, clip: 0, orig: Clip::default(), st_time: 0.0, start_pan: pan, moved: false, live: false });
                return;
            }
            if button != 1 { return; }
            // Griffe des ausgewählten Clips
            if sel.len() == 1 {
                let c = e.st.borrow().project.clip(sel[0]).cloned();
                if let Some(c) = c {
                    if c.is_visual() && t >= c.start - 1e-6 && t <= c.end() + 1e-6 {
                        let r = c.rect_at(t);
                        let (wx, wy) = v.to_widget(r.0, r.1);
                        if let Some(i) = handle_at((wx, wy, r.2 * v.s, r.3 * v.s), x, y) {
                            let crop = tool == Tool::Crop && c.kind.is_media();
                            let live = crop || !c.kind.is_annotation();
                            let st_time = c.source_time(t);
                            e.st.borrow_mut().canvas_drag = Some(Drag { mode: if crop { Mode::Crop(i) } else { Mode::Resize(i) }, clip: c.id, orig: c, st_time, start_pan: pan, moved: false, live });
                            return;
                        }
                    }
                }
            }
            let hit = { let st = e.st.borrow(); clip_at(&st.project, t, px, py) };
            match hit {
                Some(id) => {
                    e.select(Some(id), state.contains(gdk::ModifierType::CONTROL_MASK));
                    let c = e.st.borrow().project.clip(id).cloned();
                    if let Some(c) = c {
                        let st_time = c.source_time(t);
                        e.st.borrow_mut().canvas_drag = Some(Drag { mode: Mode::Move, clip: id, orig: c, st_time, start_pan: pan, moved: false, live: true });
                    }
                }
                None => e.select(None, false),
            }
        });
    }
    {
        let weak = Rc::downgrade(ed);
        drag.connect_drag_update(move |g, dx, dy| {
            let Some(e) = weak.upgrade() else { return };
            let Some(mut d) = e.st.borrow().canvas_drag.clone() else { return };
            if d.mode == Mode::Pan {
                e.st.borrow_mut().view_pan = (d.start_pan.0 + dx, d.start_pan.1 + dy);
                e.canvas.queue_draw();
                return;
            }
            if !d.moved {
                if dx.abs() < 3.0 && dy.abs() < 3.0 { return; }
                d.moved = true;
                e.begin_change();
                if let Some(sd) = e.st.borrow_mut().canvas_drag.as_mut() { sd.moved = true; }
            }
            let v = view(&e);
            let free = g.current_event_state().contains(gdk::ModifierType::SHIFT_MASK);
            let mut guides = Vec::new();
            e.modify(d.live, |p| apply_drag(p, &d, dx / v.s, dy / v.s, free, &mut guides, 8.0 / v.s));
            e.st.borrow_mut().guides = guides;
        });
    }
    {
        let weak = Rc::downgrade(ed);
        drag.connect_drag_end(move |_, _, _| {
            let Some(e) = weak.upgrade() else { return };
            let d = e.st.borrow_mut().canvas_drag.take();
            e.st.borrow_mut().guides.clear();
            if let Some(d) = d {
                if d.moved && d.mode != Mode::Pan { e.changed(); } else { e.canvas.queue_draw(); }
            }
        });
    }
    ed.canvas.add_controller(drag);


    // Mauszeiger je nach Stelle
    let motion = gtk::EventControllerMotion::new();
    {
        let weak = Rc::downgrade(ed);
        motion.connect_motion(move |_, x, y| {
            let Some(e) = weak.upgrade() else { return };
            let st = e.st.borrow();
            if st.canvas_drag.is_some() { return; }
            let mut cursor = if st.tool == Tool::Hand { "grab" } else { "default" };
            if st.tool != Tool::Hand {
                let t = st.playhead;
                drop(st);
                let v = view(&e);
                let st = e.st.borrow();
                let mut found = false;
                if st.sel.len() == 1 {
                    if let Some(c) = st.project.clip(st.sel[0]) {
                        if c.is_visual() && t >= c.start - 1e-6 && t <= c.end() + 1e-6 {
                            let r = c.rect_at(t);
                            let (wx, wy) = v.to_widget(r.0, r.1);
                            if let Some(i) = handle_at((wx, wy, r.2 * v.s, r.3 * v.s), x, y) { cursor = handle_cursor(i); found = true; }
                        }
                    }
                }
                if !found {
                    let (px, py) = v.to_project(x, y);
                    if clip_at(&st.project, t, px, py).is_some() { cursor = "move"; }
                }
            }
            e.canvas.set_cursor_from_name(Some(cursor));
        });
    }
    ed.canvas.add_controller(motion);

    // Mausrad: mit Strg zoomen, sonst Ansicht verschieben
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    {
        let weak = Rc::downgrade(ed);
        let pos = Rc::new(std::cell::Cell::new((0.0f64, 0.0f64)));
        let m2 = gtk::EventControllerMotion::new();
        let p2 = pos.clone();
        m2.connect_motion(move |_, x, y| p2.set((x, y)));
        ed.canvas.add_controller(m2);
        scroll.connect_scroll(move |c, dx, dy| {
            let Some(e) = weak.upgrade() else { return glib::Propagation::Proceed };
            let ctrl = c.current_event_state().contains(gdk::ModifierType::CONTROL_MASK);
            let v = view(&e);
            let mut st = e.st.borrow_mut();
            if ctrl {
                let (mx, my) = pos.get();
                let (px, py) = v.to_project(mx, my);
                let ns = (v.s * if dy < 0.0 { 1.15 } else { 1.0 / 1.15 }).clamp(0.05, 8.0);
                st.view_zoom = ns;
                // Punkt unter dem Mauszeiger festhalten
                let (w, h) = (e.canvas.width() as f64, e.canvas.height() as f64);
                let (pw, ph) = (st.project.width as f64, st.project.height as f64);
                st.view_pan = (mx - px * ns - (w - pw * ns) / 2.0, my - py * ns - (h - ph * ns) / 2.0);
            } else if st.view_zoom > 0.0 {
                st.view_pan = (st.view_pan.0 - dx * 40.0, st.view_pan.1 - dy * 40.0);
            } else {
                return glib::Propagation::Proceed;
            }
            drop(st);
            e.canvas.queue_draw();
            glib::Propagation::Stop
        });
    }
    ed.canvas.add_controller(scroll);

    // Dateien oder Medien aus der Liste auf die Arbeitsfläche ziehen
    let drop = gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
    drop.set_types(&[gdk::FileList::static_type(), glib::Type::STRING]);
    {
        let weak = Rc::downgrade(ed);
        drop.connect_drop(move |_, value, x, y| {
            let Some(e) = weak.upgrade() else { return false };
            let v = view(&e);
            let pos = v.to_project(x, y);
            let t = e.st.borrow().playhead;
            if let Ok(list) = value.get::<gdk::FileList>() {
                let paths: Vec<String> = list.files().iter().filter_map(|f| f.path()).map(|p| p.to_string_lossy().to_string()).collect();
                for i in e.import_files(&paths, false) { e.place_media(i, t, None, Some(pos)); }
                return true;
            }
            if let Ok(s) = value.get::<String>() {
                if let Some(i) = s.strip_prefix("wsmedia:").and_then(|n| n.parse::<usize>().ok()) {
                    e.place_media(i, t, None, Some(pos));
                    return true;
                }
            }
            false
        });
    }
    ed.canvas.add_controller(drop);
}
