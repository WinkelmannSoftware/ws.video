//! Eigene, mit Cairo gezeichnete Symbole (kein Icon-Theme nötig).
use gtk::cairo;
use gtk::prelude::*;
use std::f64::consts::PI;

pub fn widget(name: &'static str, size: i32) -> gtk::DrawingArea {
    let a = gtk::DrawingArea::new();
    a.set_content_width(size);
    a.set_content_height(size);
    a.set_valign(gtk::Align::Center);
    a.set_draw_func(move |w, cr, width, height| {
        let c = w.color();
        let s = width.min(height) as f64;
        cr.translate((width as f64 - s) / 2.0, (height as f64 - s) / 2.0);
        draw(cr, name, s, (c.red() as f64, c.green() as f64, c.blue() as f64, c.alpha() as f64));
    });
    a
}

/// Das Zeichen „W.“ nach der gelieferten Vorlage (ws-video-logo.svg), mit Cairo gezeichnet:
/// dunkle Fläche, grün leuchtender Rahmen, weißes W, grüner Punkt. Maße im 512er-Raster der Vorlage.
pub fn draw_logo(cr: &cairo::Context, s: f64) {
    let _ = cr.save();
    // Der sichtbare Teil der Vorlage reicht von 50 bis 462: So füllt das Zeichen seine Fläche aus.
    cr.scale(s / 412.0, s / 412.0);
    cr.translate(-50.0, -50.0);
    use super::theme::rgb;
    for (w, col, a) in [(50.0, rgb("akzent"), 0.035), (36.0, rgb("akzent"), 0.08), (26.0, rgb("leuchten"), 0.16)] {
        super::rounded_rect(cr, 67.0, 67.0, 378.0, 378.0, 94.0);
        cr.set_source_rgba(col.0, col.1, col.2, a);
        cr.set_line_width(w);
        let _ = cr.stroke();
    }
    let face = cairo::LinearGradient::new(67.0, 67.0, 445.0, 445.0);
    let (f0, f1) = (rgb("knopf"), rgb("grund"));
    face.add_color_stop_rgb(0.0, f0.0, f0.1, f0.2);
    face.add_color_stop_rgb(0.65, f1.0, f1.1, f1.2);
    face.add_color_stop_rgb(1.0, f1.0, f1.1, f1.2);
    super::rounded_rect(cr, 67.0, 67.0, 378.0, 378.0, 94.0);
    let _ = cr.set_source(&face);
    let _ = cr.fill_preserve();
    let mint = cairo::LinearGradient::new(86.0, 86.0, 445.0, 445.0);
    let (m0, m1, m2) = (rgb("logo_hell"), rgb("logo_mitte"), rgb("logo_dunkel"));
    mint.add_color_stop_rgb(0.0, m0.0, m0.1, m0.2);
    mint.add_color_stop_rgb(0.46, m1.0, m1.1, m1.2);
    mint.add_color_stop_rgb(1.0, m2.0, m2.1, m2.2);
    let _ = cr.set_source(&mint);
    cr.set_line_width(17.0);
    let _ = cr.stroke();
    super::rounded_rect(cr, 78.0, 78.0, 356.0, 356.0, 84.0);
    super::theme::set_a(cr, "logo_glanz", 0.32);
    cr.set_line_width(3.0);
    let _ = cr.stroke();
    cr.set_source_rgb(0.976, 0.988, 0.992);
    let w: [(f64, f64); 13] = [(118.0, 175.0), (168.0, 175.0), (197.0, 303.0), (230.0, 187.0), (272.0, 187.0), (306.0, 303.0), (335.0, 175.0), (385.0, 175.0), (341.0, 354.0), (290.0, 354.0), (251.0, 236.0), (213.0, 354.0), (161.0, 354.0)];
    for (i, p) in w.iter().enumerate() { if i == 0 { cr.move_to(p.0, p.1) } else { cr.line_to(p.0, p.1) } }
    cr.close_path();
    let _ = cr.fill();
    super::theme::set(cr, "logo_punkt");
    cr.arc(386.0, 339.0, 19.0, 0.0, 2.0 * PI);
    let _ = cr.fill();
    let _ = cr.restore();
}

pub fn logo(size: i32) -> gtk::DrawingArea {
    let a = gtk::DrawingArea::new();
    a.set_content_width(size);
    a.set_content_height(size);
    a.set_valign(gtk::Align::Center);
    a.set_draw_func(move |_, cr, width, height| {
        let s = width.min(height) as f64;
        cr.translate((width as f64 - s) / 2.0, (height as f64 - s) / 2.0);
        draw_logo(cr, s);
    });
    a
}

/// Grüner Lichtstreifen der Kopfzeile nach der gelieferten Vorlage (ws-video-header-neon.svg, 1200 × 120).
/// `right` ist die rechte Kante, an der die Vorlage ausgerichtet wird; die Höhe bestimmt den Maßstab.
pub fn draw_header_neon(cr: &cairo::Context, right: f64, h: f64) {
    let _ = cr.save();
    let k = h / 120.0;
    cr.translate(right - 1200.0 * k, 0.0);
    cr.scale(k, k);
    let wing = cairo::LinearGradient::new(645.0, 120.0, 1120.0, 0.0);
    let (ac, gl, hi, sh) = (super::theme::rgb("akzent"), super::theme::rgb("leuchten"), super::theme::rgb("logo_hell"), super::theme::rgb("logo_glanz"));
    wing.add_color_stop_rgba(0.0, ac.0, ac.1, ac.2, 0.03 * 0.7);
    wing.add_color_stop_rgba(0.55, ac.0, ac.1, ac.2, 0.5 * 0.7);
    wing.add_color_stop_rgba(1.0, gl.0, gl.1, gl.2, 0.0);
    cr.move_to(645.0, 120.0);
    cr.line_to(1000.0, 0.0);
    cr.line_to(1120.0, 0.0);
    cr.line_to(752.0, 120.0);
    cr.close_path();
    let _ = cr.set_source(&wing);
    let _ = cr.fill();
    for (w, a) in [(62.0, 0.045), (31.0, 0.11)] {
        cr.move_to(654.0, 120.0);
        cr.line_to(1018.0, 0.0);
        cr.set_source_rgba(ac.0, ac.1, ac.2, a);
        cr.set_line_width(w);
        let _ = cr.stroke();
    }
    let ray = cairo::LinearGradient::new(654.0, 120.0, 1018.0, 0.0);
    ray.add_color_stop_rgba(0.0, ac.0, ac.1, ac.2, 0.0);
    ray.add_color_stop_rgba(0.32, ac.0, ac.1, ac.2, 0.94);
    ray.add_color_stop_rgba(0.7, hi.0, hi.1, hi.2, 0.94);
    ray.add_color_stop_rgba(1.0, ac.0, ac.1, ac.2, 0.0);
    cr.move_to(654.0, 120.0);
    cr.line_to(1018.0, 0.0);
    let _ = cr.set_source(&ray);
    cr.set_line_width(8.0);
    let _ = cr.stroke();
    cr.move_to(662.0, 120.0);
    cr.line_to(1024.0, 0.0);
    cr.set_source_rgba(sh.0, sh.1, sh.2, 0.88);
    cr.set_line_width(1.8);
    let _ = cr.stroke();
    let _ = cr.restore();
}

/// Zeichnet das Symbol in ein Quadrat der Kantenlänge `s`.
pub fn draw(cr: &cairo::Context, name: &str, s: f64, col: (f64, f64, f64, f64)) {
    let _ = cr.save();
    cr.scale(s / 20.0, s / 20.0);
    cr.set_source_rgba(col.0, col.1, col.2, col.3);
    cr.set_line_width(1.8);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    let line = |pts: &[(f64, f64)]| {
        for (i, p) in pts.iter().enumerate() { if i == 0 { cr.move_to(p.0, p.1) } else { cr.line_to(p.0, p.1) } }
    };
    match name {
        "play" => { line(&[(6.0, 3.5), (16.5, 10.0), (6.0, 16.5)]); cr.close_path(); let _ = cr.fill(); }
        "pause" => { cr.rectangle(5.0, 4.0, 3.6, 12.0); cr.rectangle(11.4, 4.0, 3.6, 12.0); let _ = cr.fill(); }
        "stop" => { super::rounded_rect(cr, 4.5, 4.5, 11.0, 11.0, 2.0); let _ = cr.fill(); }
        "record" => { cr.arc(10.0, 10.0, 6.0, 0.0, 2.0 * PI); let _ = cr.fill(); }
        "start" => { line(&[(15.0, 4.0), (7.0, 10.0), (15.0, 16.0)]); cr.close_path(); let _ = cr.fill(); cr.rectangle(4.0, 4.0, 2.0, 12.0); let _ = cr.fill(); }
        "end" => { line(&[(5.0, 4.0), (13.0, 10.0), (5.0, 16.0)]); cr.close_path(); let _ = cr.fill(); cr.rectangle(14.0, 4.0, 2.0, 12.0); let _ = cr.fill(); }
        "prev" => { line(&[(12.5, 5.0), (7.0, 10.0), (12.5, 15.0)]); let _ = cr.stroke(); }
        "next" => { line(&[(7.5, 5.0), (13.0, 10.0), (7.5, 15.0)]); let _ = cr.stroke(); }
        "undo" => { cr.arc(10.5, 11.0, 5.0, PI * 1.1, PI * 0.45); let _ = cr.stroke(); line(&[(3.2, 6.0), (5.6, 9.6), (9.2, 7.4)]); let _ = cr.stroke(); }
        "redo" => { cr.arc_negative(9.5, 11.0, 5.0, -PI * 0.1, PI * 0.55); let _ = cr.stroke(); line(&[(16.8, 6.0), (14.4, 9.6), (10.8, 7.4)]); let _ = cr.stroke(); }
        "scissors" => {
            for cy in [5.5, 14.5] { cr.new_sub_path(); cr.arc(5.2, cy, 2.4, 0.0, 2.0 * PI); }
            let _ = cr.stroke();
            line(&[(7.2, 6.9), (17.0, 14.6)]);
            line(&[(7.2, 13.1), (17.0, 5.4)]);
            let _ = cr.stroke();
        }
        "split" => { line(&[(10.0, 2.5), (10.0, 17.5)]); let _ = cr.stroke(); line(&[(3.0, 10.0), (7.0, 10.0)]); line(&[(5.0, 7.5), (7.5, 10.0), (5.0, 12.5)]); line(&[(17.0, 10.0), (13.0, 10.0)]); line(&[(15.0, 7.5), (12.5, 10.0), (15.0, 12.5)]); let _ = cr.stroke(); }
        "delete" => { line(&[(4.5, 6.0), (15.5, 6.0)]); line(&[(8.0, 6.0), (8.5, 3.5), (11.5, 3.5), (12.0, 6.0)]); line(&[(5.8, 6.0), (6.6, 16.5), (13.4, 16.5), (14.2, 6.0)]); line(&[(8.6, 9.0), (8.9, 13.8)]); line(&[(11.4, 9.0), (11.1, 13.8)]); let _ = cr.stroke(); }
        "ripple" => { line(&[(3.0, 5.0), (8.0, 5.0), (8.0, 15.0), (3.0, 15.0)]); line(&[(17.0, 5.0), (12.0, 5.0), (12.0, 15.0), (17.0, 15.0)]); let _ = cr.stroke(); line(&[(9.0, 10.0), (11.0, 10.0)]); let _ = cr.stroke(); }
        "plus" => { line(&[(10.0, 4.0), (10.0, 16.0)]); line(&[(4.0, 10.0), (16.0, 10.0)]); let _ = cr.stroke(); }
        "minus" => { line(&[(4.0, 10.0), (16.0, 10.0)]); let _ = cr.stroke(); }
        "fit" => { line(&[(3.5, 7.0), (3.5, 3.5), (7.0, 3.5)]); line(&[(13.0, 3.5), (16.5, 3.5), (16.5, 7.0)]); line(&[(16.5, 13.0), (16.5, 16.5), (13.0, 16.5)]); line(&[(7.0, 16.5), (3.5, 16.5), (3.5, 13.0)]); let _ = cr.stroke(); }
        "select" => { line(&[(5.0, 3.0), (5.0, 15.5), (8.3, 12.4), (10.6, 17.2), (12.6, 16.2), (10.3, 11.6), (14.8, 11.6)]); cr.close_path(); let _ = cr.fill(); }
        "hand" => { line(&[(10.0, 2.5), (10.0, 17.5)]); line(&[(2.5, 10.0), (17.5, 10.0)]); line(&[(8.0, 4.5), (10.0, 2.5), (12.0, 4.5)]); line(&[(8.0, 15.5), (10.0, 17.5), (12.0, 15.5)]); line(&[(4.5, 8.0), (2.5, 10.0), (4.5, 12.0)]); line(&[(15.5, 8.0), (17.5, 10.0), (15.5, 12.0)]); let _ = cr.stroke(); }
        "crop" => { line(&[(6.0, 2.5), (6.0, 14.0), (17.5, 14.0)]); line(&[(2.5, 6.0), (14.0, 6.0), (14.0, 17.5)]); let _ = cr.stroke(); }
        "text" => { line(&[(4.5, 6.0), (4.5, 4.0), (15.5, 4.0), (15.5, 6.0)]); line(&[(10.0, 4.0), (10.0, 16.0)]); line(&[(7.5, 16.0), (12.5, 16.0)]); let _ = cr.stroke(); }
        "rect" => { cr.rectangle(3.5, 5.0, 13.0, 10.0); let _ = cr.stroke(); }
        "rounded" => { super::rounded_rect(cr, 3.5, 5.0, 13.0, 10.0, 3.5); let _ = cr.stroke(); }
        "ellipse" => { let _ = cr.save(); cr.translate(10.0, 10.0); cr.scale(6.8, 5.2); cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * PI); let _ = cr.restore(); let _ = cr.stroke(); }
        "circle" => { cr.arc(10.0, 10.0, 6.3, 0.0, 2.0 * PI); let _ = cr.stroke(); }
        "arrow" => { line(&[(4.0, 16.0), (15.5, 4.5)]); line(&[(9.5, 4.5), (15.5, 4.5), (15.5, 10.5)]); let _ = cr.stroke(); }
        "line" => { line(&[(4.0, 16.0), (16.0, 4.0)]); let _ = cr.stroke(); }
        "blur" => { for (i, y) in [5.0, 8.3, 11.6, 15.0].iter().enumerate() { for (j, x) in [5.0, 8.3, 11.6, 15.0].iter().enumerate() { if (i + j) % 2 == 0 { cr.rectangle(x - 1.4, y - 1.4, 2.8, 2.8); } } } let _ = cr.fill(); }
        "screen" => { super::rounded_rect(cr, 2.5, 4.0, 15.0, 10.0, 1.5); let _ = cr.stroke(); line(&[(7.0, 17.0), (13.0, 17.0)]); line(&[(10.0, 14.0), (10.0, 17.0)]); let _ = cr.stroke(); }
        "camera" => { super::rounded_rect(cr, 2.5, 5.5, 10.5, 9.0, 2.0); let _ = cr.stroke(); line(&[(13.0, 9.0), (17.5, 6.2), (17.5, 13.8), (13.0, 11.0)]); let _ = cr.stroke(); }
        "mic" => { super::rounded_rect(cr, 7.5, 2.5, 5.0, 9.5, 2.5); let _ = cr.stroke(); cr.arc(10.0, 9.5, 5.2, 0.0, PI); let _ = cr.stroke(); line(&[(10.0, 14.7), (10.0, 17.5)]); line(&[(7.0, 17.5), (13.0, 17.5)]); let _ = cr.stroke(); }
        "speaker" => { line(&[(3.0, 8.0), (6.0, 8.0), (10.0, 4.5), (10.0, 15.5), (6.0, 12.0), (3.0, 12.0)]); cr.close_path(); let _ = cr.fill(); cr.arc(10.5, 10.0, 3.2, -PI / 3.5, PI / 3.5); let _ = cr.stroke(); cr.arc(10.5, 10.0, 6.2, -PI / 3.5, PI / 3.5); let _ = cr.stroke(); }
        "mute" => { line(&[(3.0, 8.0), (6.0, 8.0), (10.0, 4.5), (10.0, 15.5), (6.0, 12.0), (3.0, 12.0)]); cr.close_path(); let _ = cr.fill(); line(&[(12.5, 7.5), (17.0, 12.5)]); line(&[(17.0, 7.5), (12.5, 12.5)]); let _ = cr.stroke(); }
        "eye" => { cr.move_to(2.5, 10.0); cr.curve_to(6.0, 4.5, 14.0, 4.5, 17.5, 10.0); cr.curve_to(14.0, 15.5, 6.0, 15.5, 2.5, 10.0); let _ = cr.stroke(); cr.arc(10.0, 10.0, 2.4, 0.0, 2.0 * PI); let _ = cr.fill(); }
        "eye-off" => { cr.move_to(2.5, 10.0); cr.curve_to(6.0, 4.5, 14.0, 4.5, 17.5, 10.0); cr.curve_to(14.0, 15.5, 6.0, 15.5, 2.5, 10.0); let _ = cr.stroke(); line(&[(4.0, 16.0), (16.0, 4.0)]); let _ = cr.stroke(); }
        "lock" => { super::rounded_rect(cr, 5.0, 9.0, 10.0, 7.5, 1.5); let _ = cr.fill(); cr.arc(10.0, 9.0, 3.2, PI, 0.0); let _ = cr.stroke(); }
        "folder" => { line(&[(2.5, 5.0), (8.0, 5.0), (9.5, 7.0), (17.5, 7.0), (17.5, 15.5), (2.5, 15.5)]); cr.close_path(); let _ = cr.stroke(); }
        "import" => { line(&[(10.0, 3.0), (10.0, 12.5)]); line(&[(6.5, 9.0), (10.0, 12.5), (13.5, 9.0)]); line(&[(3.5, 13.0), (3.5, 16.5), (16.5, 16.5), (16.5, 13.0)]); let _ = cr.stroke(); }
        "export" => { line(&[(10.0, 12.5), (10.0, 3.0)]); line(&[(6.5, 6.5), (10.0, 3.0), (13.5, 6.5)]); line(&[(3.5, 13.0), (3.5, 16.5), (16.5, 16.5), (16.5, 13.0)]); let _ = cr.stroke(); }
        "save" => { line(&[(3.5, 3.5), (13.5, 3.5), (16.5, 6.5), (16.5, 16.5), (3.5, 16.5)]); cr.close_path(); let _ = cr.stroke(); cr.rectangle(6.5, 3.5, 6.0, 4.0); let _ = cr.stroke(); cr.rectangle(6.5, 11.0, 7.0, 5.5); let _ = cr.stroke(); }
        "restart" => { cr.arc(10.0, 10.5, 5.8, -PI * 0.3, PI * 1.25); let _ = cr.stroke(); line(&[(9.0, 2.2), (13.4, 5.0), (9.6, 8.2)]); let _ = cr.stroke(); }
        "home" => { line(&[(3.0, 10.0), (10.0, 3.5), (17.0, 10.0)]); line(&[(5.0, 8.5), (5.0, 16.5), (15.0, 16.5), (15.0, 8.5)]); let _ = cr.stroke(); }
        "zoom" => { cr.arc(8.8, 8.8, 5.2, 0.0, 2.0 * PI); let _ = cr.stroke(); line(&[(12.8, 12.8), (17.0, 17.0)]); let _ = cr.stroke(); line(&[(6.4, 8.8), (11.2, 8.8)]); line(&[(8.8, 6.4), (8.8, 11.2)]); let _ = cr.stroke(); }
        "cursor" => { line(&[(5.0, 3.0), (5.0, 15.5), (8.3, 12.4), (10.6, 17.2), (12.6, 16.2), (10.3, 11.6), (14.8, 11.6)]); cr.close_path(); let _ = cr.stroke(); }
        "film" => { super::rounded_rect(cr, 3.0, 4.0, 14.0, 12.0, 1.5); let _ = cr.stroke(); line(&[(6.5, 4.0), (6.5, 16.0)]); line(&[(13.5, 4.0), (13.5, 16.0)]); let _ = cr.stroke(); }
        "image" => { super::rounded_rect(cr, 3.0, 4.0, 14.0, 12.0, 1.5); let _ = cr.stroke(); line(&[(4.0, 14.5), (8.0, 10.0), (11.0, 13.0), (13.5, 10.5), (16.0, 14.0)]); let _ = cr.stroke(); cr.arc(13.0, 7.5, 1.2, 0.0, 2.0 * PI); let _ = cr.fill(); }
        "note" => { cr.arc(7.0, 14.5, 2.4, 0.0, 2.0 * PI); let _ = cr.fill(); line(&[(9.4, 14.5), (9.4, 4.0), (15.0, 5.5), (15.0, 8.0)]); let _ = cr.stroke(); }
        "magnet" => { cr.arc(10.0, 9.0, 5.5, PI, 0.0); let _ = cr.stroke(); line(&[(4.5, 9.0), (4.5, 16.0)]); line(&[(15.5, 9.0), (15.5, 16.0)]); let _ = cr.stroke(); }
        "transition" => { super::rounded_rect(cr, 3.0, 5.0, 14.0, 10.0, 1.5); let _ = cr.stroke(); line(&[(3.5, 14.5), (16.5, 5.5), (16.5, 14.5)]); cr.close_path(); let _ = cr.fill(); }
        "motion" => { line(&[(3.0, 10.0), (13.0, 10.0)]); line(&[(9.5, 6.5), (13.0, 10.0), (9.5, 13.5)]); let _ = cr.stroke(); cr.arc(16.0, 10.0, 1.6, 0.0, 2.0 * PI); let _ = cr.fill(); }
        "wand" => { line(&[(4.0, 16.0), (12.5, 7.5)]); let _ = cr.stroke(); line(&[(14.5, 2.5), (14.5, 6.5)]); line(&[(12.5, 4.5), (16.5, 4.5)]); line(&[(16.5, 9.5), (16.5, 12.5)]); line(&[(15.0, 11.0), (18.0, 11.0)]); line(&[(8.0, 3.0), (8.0, 5.5)]); line(&[(6.8, 4.2), (9.2, 4.2)]); let _ = cr.stroke(); }
        "palette" => {
            cr.arc(10.0, 10.0, 7.0, 0.0, 2.0 * PI);
            let _ = cr.stroke();
            for (x, y) in [(7.3, 7.6), (12.7, 7.6), (10.0, 12.9)] { cr.arc(x, y, 1.4, 0.0, 2.0 * PI); let _ = cr.fill(); }
        }
        "win-min" => { line(&[(5.0, 14.0), (15.0, 14.0)]); let _ = cr.stroke(); }
        "win-max" => { super::rounded_rect(cr, 5.0, 5.0, 10.0, 10.0, 1.5); let _ = cr.stroke(); }
        "close" => { line(&[(5.0, 5.0), (15.0, 15.0)]); line(&[(15.0, 5.0), (5.0, 15.0)]); let _ = cr.stroke(); }
        _ => { cr.arc(10.0, 10.0, 3.0, 0.0, 2.0 * PI); let _ = cr.fill(); }
    }
    let _ = cr.restore();
}
