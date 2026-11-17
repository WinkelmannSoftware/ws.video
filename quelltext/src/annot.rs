//! Anmerkungen (Text, Formen, Pfeile) und Schatten werden mit Cairo/Pango als PNG gezeichnet
//! und in der Zeitleiste wie Bilder behandelt.
use crate::model::{parse_color, Clip, Kind, Shape};
use gtk::cairo;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Zähler für neu installierte Schriften: Danach müssen Texte neu gezeichnet werden.
static FONT_GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub fn clear_cache() { FONT_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }

pub fn cache_dir() -> PathBuf {
    let d = gtk::glib::user_cache_dir().join("ws-video").join("bilder");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

pub fn shape_path(cr: &cairo::Context, shape: Shape, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    match shape {
        Shape::Rect => cr.rectangle(x, y, w, h),
        Shape::Rounded => {
            let r = (radius.clamp(0.0, 1.0) * w.min(h) / 2.0).max(0.0);
            let pi = std::f64::consts::PI;
            cr.new_sub_path();
            cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
            cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
            cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
            cr.arc(x + r, y + r, r, pi, 1.5 * pi);
            cr.close_path();
        }
        Shape::Ellipse => {
            cr.save().ok();
            cr.translate(x + w / 2.0, y + h / 2.0);
            cr.scale(w / 2.0, h / 2.0);
            cr.new_sub_path();
            cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI);
            cr.restore().ok();
            cr.close_path();
        }
    }
}

/// Dreifacher Box-Weichzeichner auf dem Alphakanal einer ARGB32-Fläche (ergibt einen weichen Schatten).
fn blur_alpha(data: &mut [u8], w: usize, h: usize, stride: usize, r: usize) {
    if r == 0 { return; }
    let mut a: Vec<u32> = vec![0; w * h];
    for y in 0..h { for x in 0..w { a[y * w + x] = data[y * stride + x * 4 + 3] as u32; } }
    let mut tmp = vec![0u32; w * h];
    for _ in 0..3 {
        // horizontal
        for y in 0..h {
            let row = &a[y * w..(y + 1) * w];
            let mut sum: u32 = 0;
            let win = (2 * r + 1) as u32;
            for i in 0..=r.min(w - 1) { sum += row[i]; }
            for x in 0..w {
                tmp[y * w + x] = sum / win;
                if x + r + 1 < w { sum += row[x + r + 1]; }
                if x >= r { sum -= row[x - r]; }
            }
        }
        // vertikal
        for x in 0..w {
            let mut sum: u32 = 0;
            let win = (2 * r + 1) as u32;
            for i in 0..=r.min(h - 1) { sum += tmp[i * w + x]; }
            for y in 0..h {
                a[y * w + x] = sum / win;
                if y + r + 1 < h { sum += tmp[(y + r + 1) * w + x]; }
                if y >= r { sum -= tmp[(y - r) * w + x]; }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * stride + x * 4;
            let v = a[y * w + x].min(255) as u8;
            data[i] = 0;
            data[i + 1] = 0;
            data[i + 2] = 0;
            data[i + 3] = v;
        }
    }
}

/// Erzeugt eine Schattenfläche für die im Callback gezeichnete Form.
fn shadow_surface(w: i32, h: i32, blur: usize, strength: f64, draw: &dyn Fn(&cairo::Context)) -> Option<cairo::ImageSurface> {
    let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).ok()?;
    {
        let cr = cairo::Context::new(&surf).ok()?;
        cr.set_source_rgba(0.0, 0.0, 0.0, strength);
        draw(&cr);
    }
    surf.flush();
    let stride = surf.stride() as usize;
    {
        let mut data = surf.data().ok()?;
        blur_alpha(&mut data, w as usize, h as usize, stride, blur);
    }
    surf.mark_dirty();
    Some(surf)
}

pub struct Rendered {
    pub path: String,
    /// Rand um das eigentliche Rechteck (Projektpixel)
    pub pad: f64,
}

fn arrow_points(c: &Clip, pad: f64) -> ((f64, f64), (f64, f64)) {
    let (x0, y0, x1, y1) = (pad, pad, pad + c.w, pad + c.h);
    match c.arrow_dir {
        1 => ((x0, y1), (x1, y0)),
        2 => ((x1, y1), (x0, y0)),
        3 => ((x1, y0), (x0, y1)),
        _ => ((x0, y0), (x1, y1)),
    }
}

fn draw_arrow(cr: &cairo::Context, c: &Clip, pad: f64, fill_only_path: bool) {
    let (a, b) = arrow_points(c, pad);
    let lw = c.stroke_w.max(2.0);
    let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt().max(1.0);
    let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let head = if c.arrow_head { (lw * 3.2).min(len * 0.6) } else { 0.0 };
    let (nx, ny) = (-uy, ux);
    let bx = b.0 - ux * head;
    let by = b.1 - uy * head;
    // Schaft als Polygon, damit Füllung und Schatten gleich aussehen
    cr.new_path();
    cr.move_to(a.0 + nx * lw / 2.0, a.1 + ny * lw / 2.0);
    cr.line_to(bx + nx * lw / 2.0, by + ny * lw / 2.0);
    if c.arrow_head {
        cr.line_to(bx + nx * head * 0.55, by + ny * head * 0.55);
        cr.line_to(b.0, b.1);
        cr.line_to(bx - nx * head * 0.55, by - ny * head * 0.55);
    }
    cr.line_to(bx - nx * lw / 2.0, by - ny * lw / 2.0);
    cr.line_to(a.0 - nx * lw / 2.0, a.1 - ny * lw / 2.0);
    cr.close_path();
    if !fill_only_path { let _ = cr.fill(); }
}

fn text_layout(cr: &cairo::Context, c: &Clip, colors: bool) -> gtk::pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    let mut fd = gtk::pango::FontDescription::from_string(&c.font);
    fd.set_absolute_size(c.font_size.max(4.0) * gtk::pango::SCALE as f64);
    if c.bold { fd.set_weight(gtk::pango::Weight::Bold); }
    if c.italic { fd.set_style(gtk::pango::Style::Italic); }
    layout.set_font_description(Some(&fd));
    layout.set_width((c.w.max(1.0) * gtk::pango::SCALE as f64) as i32);
    layout.set_wrap(gtk::pango::WrapMode::WordChar);
    layout.set_alignment(match c.align { 0 => gtk::pango::Alignment::Left, 2 => gtk::pango::Alignment::Right, _ => gtk::pango::Alignment::Center });
    layout.set_text(&c.text);
    if colors && !c.text_colors.is_empty() {
        // Zeichenpositionen in Bytepositionen umrechnen
        let mut idx: Vec<usize> = c.text.char_indices().map(|(i, _)| i).collect();
        idx.push(c.text.len());
        let attrs = gtk::pango::AttrList::new();
        for t in &c.text_colors {
            let (a, b) = ((t.start as usize).min(idx.len() - 1), (t.end as usize).min(idx.len() - 1));
            if b <= a { continue; }
            let k = parse_color(&t.color);
            let mut at = gtk::pango::AttrColor::new_foreground((k.0 * 65535.0) as u16, (k.1 * 65535.0) as u16, (k.2 * 65535.0) as u16);
            at.set_start_index(idx[a] as u32);
            at.set_end_index(idx[b] as u32);
            attrs.insert(at);
        }
        layout.set_attributes(Some(&attrs));
    }
    layout
}

/// Textfelder wachsen in der Höhe mit, damit kein Text abgeschnitten wird.
pub fn fit_text_height(c: &mut Clip) -> bool {
    if c.kind != Kind::Text { return false; }
    let Ok(surf) = cairo::ImageSurface::create(cairo::Format::ARgb32, 4, 4) else { return false };
    let Ok(cr) = cairo::Context::new(&surf) else { return false };
    let (_, lh) = text_layout(&cr, c, false).pixel_size();
    let need = (lh as f64 + c.font_size * 0.3).ceil();
    if need > c.h + 0.5 {
        let grow = need - c.h;
        c.h = need;
        for a in c.anims.iter_mut() { a.h += grow; }
        return true;
    }
    false
}

/// Zeichnet eine Anmerkung in eine PNG-Datei (mit Zwischenspeicher).
pub fn render(c: &Clip) -> Option<Rendered> {
    let pad = (c.stroke_w / 2.0).ceil() + c.outline.max(0.0).ceil() + if c.shadow { 28.0 } else { 2.0 };
    let key = format!(
        "v5|{:?}|{:.1}x{:.1}|{:?}|{:.3}|{}|{}|{}|{:.1}|{}|{}|{}|{}|{}|{:.1}|{}|{}|{}|{:.1}|{}|{:?}|{}",
        c.kind, c.w, c.h, c.shape, c.radius, c.fill, c.stroke, c.color, c.stroke_w, c.text, c.font, c.bold, c.italic, c.align, c.font_size, c.shadow, c.arrow_dir, c.arrow_head, c.outline, c.outline_color, c.text_colors, FONT_GEN.load(std::sync::atomic::Ordering::Relaxed)
    );
    let path = cache_dir().join(format!("a{:016x}.png", hash_str(&key)));
    let rendered = Rendered { path: path.to_string_lossy().to_string(), pad };
    if path.exists() { return Some(rendered); }
    let (sw, sh) = ((c.w + 2.0 * pad).ceil().max(4.0) as i32, (c.h + 2.0 * pad).ceil().max(4.0) as i32);
    let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, sw, sh).ok()?;
    {
        let cr = cairo::Context::new(&surf).ok()?;
        let fill = parse_color(&c.fill);
        let stroke = parse_color(&c.stroke);
        let col = parse_color(&c.color);
        match c.kind {
            Kind::Shape => {
                if c.shadow {
                    if let Some(s) = shadow_surface(sw, sh, 8, 0.55, &|k| { shape_path(k, c.shape, pad, pad + 6.0, c.w, c.h, c.radius); let _ = k.fill(); }) {
                        let _ = cr.set_source_surface(&s, 0.0, 0.0);
                        let _ = cr.paint();
                    }
                }
                shape_path(&cr, c.shape, pad, pad, c.w, c.h, c.radius);
                cr.set_source_rgba(fill.0, fill.1, fill.2, fill.3);
                let _ = cr.fill_preserve();
                if c.stroke_w > 0.0 {
                    cr.set_line_width(c.stroke_w);
                    cr.set_source_rgba(stroke.0, stroke.1, stroke.2, stroke.3);
                    let _ = cr.stroke();
                } else {
                    cr.new_path();
                }
            }
            Kind::Arrow => {
                if c.shadow {
                    if let Some(s) = shadow_surface(sw, sh, 6, 0.55, &|k| { k.translate(0.0, 5.0); draw_arrow(k, c, pad, false); }) {
                        let _ = cr.set_source_surface(&s, 0.0, 0.0);
                        let _ = cr.paint();
                    }
                }
                cr.set_source_rgba(fill.0, fill.1, fill.2, fill.3);
                draw_arrow(&cr, c, pad, false);
            }
            _ => {
                // Text: optionales Feld im Hintergrund, dann der Text mittig
                if fill.3 > 0.0 {
                    if c.shadow {
                        if let Some(s) = shadow_surface(sw, sh, 8, 0.5, &|k| { shape_path(k, c.shape, pad, pad + 6.0, c.w, c.h, c.radius); let _ = k.fill(); }) {
                            let _ = cr.set_source_surface(&s, 0.0, 0.0);
                            let _ = cr.paint();
                        }
                    }
                    shape_path(&cr, c.shape, pad, pad, c.w, c.h, c.radius);
                    cr.set_source_rgba(fill.0, fill.1, fill.2, fill.3);
                    let _ = cr.fill_preserve();
                    if c.stroke_w > 0.0 {
                        cr.set_line_width(c.stroke_w);
                        cr.set_source_rgba(stroke.0, stroke.1, stroke.2, stroke.3);
                        let _ = cr.stroke();
                    } else {
                        cr.new_path();
                    }
                }
                let layout = text_layout(&cr, c, true);
                let (_, lh) = layout.pixel_size();
                let ty = pad + ((c.h - lh as f64) / 2.0).max(0.0);
                if c.shadow && fill.3 <= 0.0 {
                    if let Some(s) = shadow_surface(sw, sh, 4, 0.8, &|k| {
                        let l = text_layout(k, c, false);
                        k.move_to(pad, ty + 4.0);
                        pangocairo::functions::show_layout(k, &l);
                    }) {
                        let _ = cr.set_source_surface(&s, 0.0, 0.0);
                        let _ = cr.paint();
                    }
                }
                if c.outline > 0.0 {
                    // Kontur: Umriss der Buchstaben in doppelter Breite, der Text liegt darüber
                    let oc = parse_color(&c.outline_color);
                    cr.move_to(pad, ty);
                    pangocairo::functions::layout_path(&cr, &layout);
                    cr.set_line_width(c.outline * 2.0);
                    cr.set_line_join(cairo::LineJoin::Round);
                    cr.set_source_rgba(oc.0, oc.1, oc.2, oc.3);
                    let _ = cr.stroke();
                }
                cr.set_source_rgba(col.0, col.1, col.2, col.3);
                cr.move_to(pad, ty);
                pangocairo::functions::show_layout(&cr, &layout);
            }
        }
    }
    let mut f = std::fs::File::create(&path).ok()?;
    surf.write_to_png(&mut f).ok()?;
    Some(rendered)
}

/// Schattenbild für einen Medien-Clip (Form des Clips, weich, leicht nach unten versetzt).
pub fn render_shadow(c: &Clip) -> Option<Rendered> {
    // Auflösung des Schattens begrenzen: er ist ohnehin weich
    let k = (480.0 / c.w.max(c.h).max(1.0)).min(1.0);
    let (w, h) = (c.w * k, c.h * k);
    let pad_px = 40.0 * k.max(0.25);
    let key = format!("s2|{:.0}x{:.0}|{:?}|{:.3}", w, h, c.shape, c.radius);
    let path = cache_dir().join(format!("s{:016x}.png", hash_str(&key)));
    let rendered = Rendered { path: path.to_string_lossy().to_string(), pad: pad_px / k };
    if path.exists() { return Some(rendered); }
    let (sw, sh) = ((w + 2.0 * pad_px).ceil() as i32, (h + 2.0 * pad_px).ceil() as i32);
    let blur = (pad_px / 3.5).round().max(2.0) as usize;
    let s = shadow_surface(sw, sh, blur, 0.6, &|cr| { shape_path(cr, c.shape, pad_px, pad_px + pad_px * 0.2, w, h, c.radius); let _ = cr.fill(); })?;
    let mut f = std::fs::File::create(&path).ok()?;
    s.write_to_png(&mut f).ok()?;
    Some(rendered)
}
