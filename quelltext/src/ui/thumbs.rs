//! Kleine Schaubilder für die Kacheln der Menüs „Übergänge“, „Animationen“ und „Effekte“.
//! Jedes Bild zeigt an einer einfachen Beispielszene, was die Kachel tut. Steht die Maus auf der Kachel,
//! läuft das Bild als kurze Schleife (`k` von 0 bis 1); sonst zeigt es einen typischen Augenblick.
use gtk::cairo;
use gtk::prelude::*;
use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;

pub const W: i32 = 112;
pub const H: i32 = 63;

fn smooth(k: f64) -> f64 { let k = k.clamp(0.0, 1.0); k * k * (3.0 - 2.0 * k) }
fn bump(k: f64) -> f64 { (PI * k.clamp(0.0, 1.0)).sin() }

fn rounded(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

/// Beispielszene A (Landschaft am Tag) oder B (Stadt bei Nacht), formatfüllend in w × h.
fn scene(cr: &cairo::Context, w: f64, h: f64, b: bool) {
    let g = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
    if !b {
        g.add_color_stop_rgb(0.0, 0.16, 0.55, 0.78);
        g.add_color_stop_rgb(1.0, 0.62, 0.84, 0.90);
        let _ = cr.set_source(&g);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        cr.set_source_rgb(1.0, 0.85, 0.25);
        cr.arc(w * 0.76, h * 0.28, h * 0.15, 0.0, 2.0 * PI);
        let _ = cr.fill();
        cr.set_source_rgb(0.13, 0.50, 0.33);
        cr.move_to(0.0, h);
        cr.curve_to(w * 0.15, h * 0.35, w * 0.45, h * 0.45, w * 0.70, h);
        let _ = cr.fill();
        cr.set_source_rgb(0.08, 0.38, 0.27);
        cr.move_to(w * 0.35, h);
        cr.curve_to(w * 0.60, h * 0.50, w * 0.85, h * 0.55, w, h * 0.85);
        cr.line_to(w, h);
        let _ = cr.fill();
    } else {
        g.add_color_stop_rgb(0.0, 0.14, 0.10, 0.36);
        g.add_color_stop_rgb(1.0, 0.55, 0.20, 0.50);
        let _ = cr.set_source(&g);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        cr.set_source_rgb(0.98, 0.93, 0.80);
        cr.arc(w * 0.22, h * 0.26, h * 0.11, 0.0, 2.0 * PI);
        let _ = cr.fill();
        cr.set_source_rgb(0.07, 0.06, 0.18);
        for (x, bw, bh) in [(0.06, 0.14, 0.42), (0.24, 0.12, 0.62), (0.40, 0.17, 0.36), (0.61, 0.13, 0.70), (0.78, 0.18, 0.48)] {
            cr.rectangle(w * x, h * (1.0 - bh), w * bw, h * bh);
        }
        let _ = cr.fill();
        cr.set_source_rgb(1.0, 0.82, 0.35);
        for (x, y) in [(0.28, 0.50), (0.31, 0.64), (0.65, 0.42), (0.68, 0.56), (0.84, 0.66), (0.10, 0.70)] {
            cr.rectangle(w * x, h * y, w * 0.022, h * 0.06);
        }
        let _ = cr.fill();
    }
}

/// Szene um die Mitte skaliert und verschoben zeichnen
fn scene_at(cr: &cairo::Context, w: f64, h: f64, b: bool, scale: f64, dx: f64, dy: f64, alpha: f64) {
    let _ = cr.save();
    cr.translate(w / 2.0 + dx, h / 2.0 + dy);
    cr.scale(scale, scale);
    cr.translate(-w / 2.0, -h / 2.0);
    if alpha < 0.999 {
        cr.push_group();
        scene(cr, w, h, b);
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha(alpha.clamp(0.0, 1.0));
    } else {
        scene(cr, w, h, b);
    }
    let _ = cr.restore();
}

/// Karomuster für „durchsichtig“
fn checker(cr: &cairo::Context, w: f64, h: f64) {
    cr.set_source_rgb(0.16, 0.19, 0.22);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    cr.set_source_rgb(0.24, 0.28, 0.31);
    let s = h / 7.0;
    let mut y = 0;
    while (y as f64) * s < h {
        let mut x = y % 2;
        while (x as f64) * s < w { cr.rectangle(x as f64 * s, y as f64 * s, s, s); x += 2; }
        y += 1;
    }
    let _ = cr.fill();
}

fn dim(cr: &cairo::Context, w: f64, h: f64, a: f64) {
    cr.set_source_rgba(0.02, 0.07, 0.09, a);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
}

/// Das bewegte Objekt der Animationen: eine grüne Karte mit weißem Balken
fn object(cr: &cairo::Context, cx: f64, cy: f64, s: f64, alpha: f64) {
    if s <= 0.01 { return; }
    let (ow, oh) = (s * 1.5, s);
    rounded(cr, cx - ow / 2.0, cy - oh / 2.0, ow, oh, s * 0.2);
    super::theme::set_a(cr, "akzent", alpha);
    let _ = cr.fill();
    cr.set_source_rgba(1.0, 1.0, 1.0, alpha);
    rounded(cr, cx - ow * 0.32, cy - oh * 0.16, ow * 0.64, oh * 0.14, oh * 0.07);
    rounded(cr, cx - ow * 0.32, cy + oh * 0.10, ow * 0.40, oh * 0.12, oh * 0.06);
    let _ = cr.fill();
}

/// Person vor der Kamera (Kopf und Schultern), Mitte unten bei (cx, bottom)
fn person(cr: &cairo::Context, cx: f64, bottom: f64, s: f64) {
    cr.set_source_rgb(0.20, 0.33, 0.62);
    cr.new_path();
    cr.arc(cx, bottom + s * 0.25, s * 0.62, PI, 2.0 * PI);
    let _ = cr.fill();
    cr.set_source_rgb(0.96, 0.80, 0.66);
    cr.arc(cx, bottom - s * 0.68, s * 0.30, 0.0, 2.0 * PI);
    let _ = cr.fill();
    cr.set_source_rgb(0.30, 0.20, 0.14);
    cr.arc(cx, bottom - s * 0.76, s * 0.31, PI * 1.05, PI * 1.95);
    let _ = cr.fill();
}

fn no_sign(cr: &cairo::Context, w: f64, h: f64) {
    dim(cr, w, h, 0.45);
    let r = h * 0.26;
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.set_line_width(h * 0.07);
    cr.arc(w / 2.0, h / 2.0, r, 0.0, 2.0 * PI);
    let _ = cr.stroke();
    cr.move_to(w / 2.0 - r * 0.7, h / 2.0 + r * 0.7);
    cr.line_to(w / 2.0 + r * 0.7, h / 2.0 - r * 0.7);
    let _ = cr.stroke();
}

fn arrow(cr: &cairo::Context, x0: f64, y0: f64, x1: f64, y1: f64, s: f64) {
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
    cr.set_line_width(s * 0.35);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.move_to(x0, y0);
    cr.line_to(x1, y1);
    let _ = cr.stroke();
    let a = (y1 - y0).atan2(x1 - x0);
    for d in [2.6, -2.6] {
        cr.move_to(x1, y1);
        cr.line_to(x1 + s * (a + d).cos(), y1 + s * (a + d).sin());
    }
    let _ = cr.stroke();
}

/// Weichgezeichnete Szene (mehrfach leicht versetzt übereinander)
fn blurred(cr: &cairo::Context, w: f64, h: f64, b: bool, r: f64) {
    scene(cr, w, h, b);
    if r < 0.3 { return; }
    for ring in [1.0, 0.5] {
        for i in 0..8 {
            let a = i as f64 * PI / 4.0;
            scene_at(cr, w, h, b, 1.0, a.cos() * r * ring, a.sin() * r * ring, 0.22);
        }
    }
}

/// Karte mit der Person (Kamerabild) in der Bildmitte; `clip` legt die Form fest
fn camera(cr: &cairo::Context, w: f64, h: f64, shape: u8, back: impl Fn(&cairo::Context, f64, f64)) {
    let (cw, ch) = (h * 1.0, h * 0.74);
    let (x, y) = ((w - cw) / 2.0, (h - ch) / 2.0);
    let _ = cr.save();
    match shape {
        2 => { let r = ch / 2.0; cr.arc(w / 2.0, h / 2.0, r, 0.0, 2.0 * PI); }
        1 => rounded(cr, x, y, cw, ch, ch * 0.22),
        _ => cr.rectangle(x, y, cw, ch),
    }
    cr.clip();
    cr.translate(x, y);
    back(cr, cw, ch);
    person(cr, cw / 2.0, ch, ch * 0.62);
    let _ = cr.restore();
}

fn office(cr: &cairo::Context, w: f64, h: f64) {
    cr.set_source_rgb(0.80, 0.74, 0.62);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    cr.set_source_rgb(0.55, 0.42, 0.30);
    cr.rectangle(w * 0.06, h * 0.12, w * 0.26, h * 0.5);
    let _ = cr.fill();
    cr.set_source_rgb(0.62, 0.80, 0.92);
    cr.rectangle(w * 0.66, h * 0.10, w * 0.28, h * 0.36);
    let _ = cr.fill();
}

/// Schaubild zeichnen. `action` ist der Name der Kachel, `k` die Stelle in der Schleife (0..1).
pub fn draw(cr: &cairo::Context, action: &str, w: f64, h: f64, k: f64) {
    let _ = cr.save();
    rounded(cr, 0.0, 0.0, w, h, 6.0);
    cr.clip();
    let half = k < 0.5;
    let (k1, k2) = ((k * 2.0).clamp(0.0, 1.0), (k * 2.0 - 1.0).clamp(0.0, 1.0));
    match action {
        // ---- Übergänge ----
        "xfade" => { scene(cr, w, h, false); scene_at(cr, w, h, true, 1.0, 0.0, 0.0, smooth(k)); }
        "fadein" => { checker(cr, w, h); scene_at(cr, w, h, false, 1.0, 0.0, 0.0, smooth(k)); }
        "fadeout" => { checker(cr, w, h); scene_at(cr, w, h, false, 1.0, 0.0, 0.0, 1.0 - smooth(k)); }
        "fadeboth" => { checker(cr, w, h); scene_at(cr, w, h, false, 1.0, 0.0, 0.0, (bump(k) * 1.6).min(1.0)); }
        "rollin" | "rollout" | "rollboth" => {
            checker(cr, w, h);
            let a = match action { "rollin" => smooth(k), "rollout" => 1.0 - smooth(k), _ => (bump(k) * 1.6).min(1.0) };
            let _ = cr.save();
            cr.rectangle(0.0, 0.0, w * a, h);
            cr.clip();
            scene(cr, w, h, false);
            let _ = cr.restore();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
            cr.rectangle(w * a - 1.0, 0.0, 2.0, h);
            let _ = cr.fill();
        }
        "t_black" | "t_white" | "t_flash" => {
            scene(cr, w, h, !half);
            let a = if action == "t_flash" { (1.0 - (k - 0.5).abs() * 5.0).max(0.0) } else { smooth(1.0 - (k - 0.5).abs() * 2.0) };
            let c = if action == "t_black" { 0.0 } else { 1.0 };
            cr.set_source_rgba(c, c, c, a);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
        }
        "t_zoom" => {
            if half { scene_at(cr, w, h, false, 1.0 + 2.4 * k1 * k1, 0.0, 0.0, 1.0); }
            else { scene_at(cr, w, h, true, 1.0 + 2.4 * (1.0 - k2) * (1.0 - k2), 0.0, 0.0, 1.0); }
        }
        "t_whipl" | "t_whipr" => {
            let dir = if action == "t_whipl" { -1.0 } else { 1.0 };
            let off = smooth(k) * w;
            scene_at(cr, w, h, false, 1.0, dir * off, 0.0, 1.0);
            scene_at(cr, w, h, true, 1.0, dir * (off - w), 0.0, 1.0);
            let a = bump(k);
            cr.set_line_width(1.2);
            for i in 0..9 {
                let y = h * (0.08 + i as f64 * 0.105);
                let x = w * ((i * 37 % 100) as f64 / 100.0);
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.55 * a);
                cr.move_to(x - w * 0.3, y);
                cr.line_to(x + w * 0.3, y);
                let _ = cr.stroke();
            }
        }
        "t_blur" => blurred(cr, w, h, !half, bump(k) * 5.0),
        "t_glitch" => {
            scene(cr, w, h, !half);
            let a = bump(k);
            let step = (k * 14.0) as usize;
            for i in 0..5 {
                let y = h * (((i * 29 + step * 17) % 90) as f64 / 100.0);
                let bh = h * (0.06 + (i % 3) as f64 * 0.04);
                let dx = w * 0.14 * a * if (i + step) % 2 == 0 { 1.0 } else { -1.0 };
                let _ = cr.save();
                cr.rectangle(0.0, y, w, bh);
                cr.clip();
                scene_at(cr, w, h, !half, 1.0, dx, 0.0, 1.0);
                cr.set_source_rgba(if i % 2 == 0 { 1.0 } else { 0.0 }, 0.0, if i % 2 == 0 { 0.2 } else { 1.0 }, 0.30 * a);
                cr.rectangle(0.0, y, w, bh);
                let _ = cr.fill();
                let _ = cr.restore();
            }
        }
        "fadenone" | "animnone" | "c_reset" => { scene(cr, w, h, false); no_sign(cr, w, h); }
        // ---- Animationen ----
        "zoomin" | "zoomout" => {
            let a = if action == "zoomin" { smooth(k) } else { 1.0 - smooth(k) };
            let s = 1.0 + 0.9 * a;
            let (px, py) = (w * 0.66, h * 0.60);
            scene_at(cr, w, h, false, s, (w / 2.0 - px) * (s - 1.0), (h / 2.0 - py) * (s - 1.0), 1.0);
            // Mauszeiger
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.move_to(px, py);
            cr.line_to(px, py + h * 0.22);
            cr.line_to(px + h * 0.06, py + h * 0.16);
            cr.line_to(px + h * 0.15, py + h * 0.16);
            cr.close_path();
            let _ = cr.fill_preserve();
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.set_line_width(0.8);
            let _ = cr.stroke();
        }
        "animcustom" => {
            scene(cr, w, h, false);
            dim(cr, w, h, 0.35);
            let a = smooth(k);
            let (x0, y0, x1, y1) = (w * 0.24, h * 0.70, w * 0.72, h * 0.34);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.7);
            cr.set_line_width(1.0);
            cr.set_dash(&[3.0, 3.0], 0.0);
            cr.move_to(x0, y0);
            cr.line_to(x1, y1);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
            object(cr, x0 + (x1 - x0) * a, y0 + (y1 - y0) * a, h * (0.22 + 0.16 * a), 1.0);
        }
        a if a.starts_with("slide") => {
            scene(cr, w, h, false);
            dim(cr, w, h, 0.35);
            let enter = a.starts_with("slidein");
            let away = if enter { (1.0 - k).powi(3) } else { k.powi(3) };
            let (cx, cy, s) = (w / 2.0, h / 2.0, h * 0.34);
            let (dx, dy) = match a.rsplit('_').next().unwrap_or("") {
                "l" => (-(cx + s), 0.0), "r" => (w - cx + s, 0.0), "u" => (0.0, -(cy + s)), _ => (0.0, h - cy + s),
            };
            // Pfeil in Bewegungsrichtung
            let unit = |v: f64| if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 };
            let sign = if enter { -1.0 } else { 1.0 };
            let (ux, uy) = (unit(dx) * sign, unit(dy) * sign);
            object(cr, cx + dx * away, cy + dy * away, s, 1.0);
            let (bx, by) = if ux != 0.0 { (cx, h * 0.88) } else { (w * 0.86, cy) };
            arrow(cr, bx - ux * w * 0.12, by - uy * h * 0.16, bx + ux * w * 0.12, by + uy * h * 0.16, h * 0.09);
        }
        "popin" | "popout" => {
            scene(cr, w, h, false);
            dim(cr, w, h, 0.35);
            let f = if action == "popin" {
                let k = (k * 1.4).min(1.0);
                if k < 0.6 { smooth(k / 0.6) * 1.2 } else { 1.2 - 0.2 * smooth((k - 0.6) / 0.4) }
            } else { 1.0 - smooth((k * 1.3).min(1.0)) };
            object(cr, w / 2.0, h / 2.0, h * 0.36 * f, 1.0);
        }
        // ---- Effekte ----
        "circle" => { checker(cr, w, h); camera(cr, w, h, 2, office); }
        "rounded" => { checker(cr, w, h); camera(cr, w, h, 1, office); }
        "rectshape" => { checker(cr, w, h); camera(cr, w, h, 0, office); }
        "border" => {
            scene(cr, w, h, false);
            dim(cr, w, h, 0.3);
            camera(cr, w, h, 1, office);
            let (cw, ch) = (h * 1.0, h * 0.74);
            rounded(cr, (w - cw) / 2.0, (h - ch) / 2.0, cw, ch, ch * 0.22);
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.set_line_width(3.0);
            let _ = cr.stroke();
        }
        "shadow" => {
            cr.set_source_rgb(0.80, 0.84, 0.86);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
            let (cw, ch) = (h * 1.0, h * 0.74);
            for i in 0..6 {
                let g = i as f64;
                rounded(cr, (w - cw) / 2.0 + 4.0 - g, (h - ch) / 2.0 + 5.0 - g, cw + 2.0 * g, ch + 2.0 * g, ch * 0.22 + g);
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.10);
                let _ = cr.fill();
            }
            camera(cr, w, h, 1, office);
        }
        "chroma" | "bgremove" | "bgblur" | "bgimage" => {
            // links vorher, rechts nachher
            let green = |cr: &cairo::Context, w: f64, h: f64| { cr.set_source_rgb(0.10, 0.75, 0.20); cr.rectangle(0.0, 0.0, w, h); let _ = cr.fill(); };
            let _ = cr.save();
            cr.rectangle(0.0, 0.0, w / 2.0, h);
            cr.clip();
            if action == "chroma" { green(cr, w, h); } else { office(cr, w, h); }
            let _ = cr.restore();
            let _ = cr.save();
            cr.rectangle(w / 2.0, 0.0, w / 2.0, h);
            cr.clip();
            match action {
                "bgblur" => {
                    office(cr, w, h);
                    for i in 0..8 {
                        let a = i as f64 * PI / 4.0;
                        let _ = cr.save();
                        cr.translate(a.cos() * 4.0, a.sin() * 4.0);
                        cr.push_group();
                        office(cr, w, h);
                        let _ = cr.pop_group_to_source();
                        let _ = cr.paint_with_alpha(0.25);
                        let _ = cr.restore();
                    }
                }
                "bgimage" => scene(cr, w, h, true),
                _ => checker(cr, w, h),
            }
            let _ = cr.restore();
            person(cr, w / 2.0, h, h * 0.66);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
            cr.rectangle(w / 2.0 - 0.75, 0.0, 1.5, h);
            let _ = cr.fill();
        }
        "opacity" => { checker(cr, w, h); scene_at(cr, w, h, false, 1.0, 0.0, 0.0, 0.5); }
        "c_bright" | "c_dark" | "c_contrast" | "c_sat" | "c_bw" | "c_warm" | "c_cool" => {
            scene(cr, w, h, false);
            let _ = cr.save();
            cr.rectangle(w / 2.0, 0.0, w / 2.0, h);
            cr.clip();
            let fill = |r: f64, g: f64, b: f64, a: f64, op: cairo::Operator| {
                cr.set_operator(op);
                cr.set_source_rgba(r, g, b, a);
                cr.rectangle(0.0, 0.0, w, h);
                let _ = cr.fill();
                cr.set_operator(cairo::Operator::Over);
            };
            match action {
                "c_bright" => fill(1.0, 1.0, 1.0, 0.32, cairo::Operator::Over),
                "c_dark" => fill(0.0, 0.0, 0.0, 0.40, cairo::Operator::Over),
                "c_contrast" => { cr.push_group(); scene(cr, w, h, false); let _ = cr.pop_group_to_source(); cr.set_operator(cairo::Operator::Overlay); let _ = cr.paint(); cr.set_operator(cairo::Operator::Over); }
                "c_sat" => fill(1.0, 0.0, 0.0, 0.75, cairo::Operator::HslSaturation),
                "c_bw" => fill(0.5, 0.5, 0.5, 1.0, cairo::Operator::HslSaturation),
                "c_warm" => fill(1.0, 0.55, 0.10, 0.30, cairo::Operator::Over),
                _ => fill(0.10, 0.45, 1.0, 0.30, cairo::Operator::Over),
            }
            let _ = cr.restore();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
            cr.rectangle(w / 2.0 - 0.75, 0.0, 1.5, h);
            let _ = cr.fill();
        }
        "fliph" | "flipv" => {
            // in der Schleife klappt das Bild um; im Stand ist es gespiegelt
            let f = (PI * k).cos();
            let f = if f.abs() < 0.04 { 0.04 } else { f };
            cr.set_source_rgb(0.02, 0.07, 0.09);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
            let _ = cr.save();
            cr.translate(w / 2.0, h / 2.0);
            if action == "fliph" { cr.scale(f, 1.0); } else { cr.scale(1.0, f); }
            cr.translate(-w / 2.0, -h / 2.0);
            scene(cr, w, h, false);
            let _ = cr.restore();
            if action == "fliph" { arrow(cr, w * 0.5, h * 0.86, w * 0.68, h * 0.86, h * 0.08); arrow(cr, w * 0.5, h * 0.86, w * 0.32, h * 0.86, h * 0.08); }
            else { arrow(cr, w * 0.90, h * 0.5, w * 0.90, h * 0.80, h * 0.08); arrow(cr, w * 0.90, h * 0.5, w * 0.90, h * 0.20, h * 0.08); }
        }
        "vignette" => {
            scene(cr, w, h, false);
            let g = cairo::RadialGradient::new(w / 2.0, h / 2.0, h * 0.25, w / 2.0, h / 2.0, w * 0.62);
            g.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            g.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.92);
            let _ = cr.set_source(&g);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
        }
        "pixelate" => {
            scene(cr, w, h, true);
            let (x0, y0, bs) = (w * 0.44, h * 0.22, h * 0.13);
            let cols = [(0.07, 0.06, 0.18), (0.36, 0.16, 0.44), (1.0, 0.82, 0.35), (0.25, 0.13, 0.40), (0.50, 0.20, 0.50), (0.12, 0.08, 0.26)];
            for j in 0..4 {
                for i in 0..4 {
                    let c = cols[(i * 5 + j * 3 + i * j) % cols.len()];
                    cr.set_source_rgb(c.0, c.1, c.2);
                    cr.rectangle(x0 + i as f64 * bs, y0 + j as f64 * bs, bs + 0.5, bs + 0.5);
                    let _ = cr.fill();
                }
            }
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
            cr.set_line_width(1.2);
            cr.set_dash(&[3.0, 2.0], 0.0);
            cr.rectangle(x0, y0, bs * 4.0, bs * 4.0);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
        }
        _ => scene(cr, w, h, false),
    }
    let _ = cr.restore();
}

/// Stelle der Schleife, die das Bild im Stand zeigt
fn still(action: &str) -> f64 {
    match action {
        "t_black" | "t_white" => 0.36,
        "t_flash" => 0.44,
        "t_zoom" => 0.30,
        "t_blur" | "t_glitch" => 0.30,
        "rollin" | "rollout" | "rollboth" => 0.55,
        "zoomin" => 0.8,
        "zoomout" => 0.2,
        "animcustom" => 0.6,
        "popin" => 0.38,
        "popout" => 0.4,
        "fliph" | "flipv" => 1.0,
        a if a.starts_with("slidein") => 0.22,
        a if a.starts_with("slideout") => 0.62,
        _ => 0.5,
    }
}

/// Zeichenfläche mit dem Schaubild. `hover` meldet, ob die Maus auf der Kachel steht; dann läuft die Schleife.
pub fn widget(action: &'static str, hover: &impl IsA<gtk::Widget>) -> gtk::DrawingArea {
    let a = gtk::DrawingArea::new();
    a.set_content_width(W);
    a.set_content_height(H);
    a.set_halign(gtk::Align::Center);
    let start: Rc<Cell<Option<std::time::Instant>>> = Rc::new(Cell::new(None));
    {
        let start = start.clone();
        a.set_draw_func(move |_, cr, w, h| {
            let k = match start.get() {
                Some(t) => { let p = t.elapsed().as_secs_f64() / 1.8; let f = p.fract(); (f * 1.15).min(1.0) }
                None => still(action),
            };
            draw(cr, action, w as f64, h as f64, k);
        });
    }
    let motion = gtk::EventControllerMotion::new();
    {
        let (start, area) = (start.clone(), a.clone());
        motion.connect_enter(move |_, _, _| {
            if start.get().is_some() { return; }
            start.set(Some(std::time::Instant::now()));
            let (start, area) = (start.clone(), area.downgrade());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
                let Some(area) = area.upgrade() else { return gtk::glib::ControlFlow::Break };
                area.queue_draw();
                if start.get().is_some() { gtk::glib::ControlFlow::Continue } else { gtk::glib::ControlFlow::Break }
            });
        });
    }
    {
        let (start, area) = (start.clone(), a.clone());
        motion.connect_leave(move |_| { start.set(None); area.queue_draw(); });
    }
    hover.add_controller(motion);
    a
}
