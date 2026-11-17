//! Oberfläche (GTK4) von WS.Video.
pub mod app;
pub mod canvas;
pub mod editor;
pub mod export;
pub mod icons;
pub mod props;
pub mod recorder;
pub mod start;
pub mod theme;
pub mod thumbs;
pub mod timeline;

pub use app::{project_file_in, App};

use crate::engine::Frame;
use gtk::prelude::*;
use gtk::{cairo, gdk, glib};


/// Schriftzug „Winkelmann.Software“ mit grünem Punkt.
pub fn wordmark() -> gtk::Label {
    let l = gtk::Label::new(None);
    let mark = |l: &gtk::Label| l.set_markup(&format!("Winkelmann<span foreground=\"{}\">.</span>Software", theme::hex("akzent")));
    mark(&l);
    let weak = l.downgrade();
    theme::on_change(move || { if let Some(l) = weak.upgrade() { mark(&l); } });
    l.add_css_class("ws-brand");
    l
}

/// Kopfzeile des Fensters: Zeichen, Name, Fensterknöpfe. Sie dient zugleich als Titelleiste (ziehen, Doppelklick).
pub fn header(window: &gtk::ApplicationWindow) -> gtk::WindowHandle {
    let h = hbox(10);
    h.add_css_class("ws-header");
    h.append(&icons::logo(28));
    h.append(&wordmark());
    let sep = gtk::Separator::new(gtk::Orientation::Vertical);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    sep.set_margin_start(8);
    sep.set_margin_end(8);
    h.append(&sep);
    h.append(&label("Video Editor & Screen Recording", "ws-brand-sub"));
    let sp = hbox(0);
    sp.set_hexpand(true);
    h.append(&sp);
    let b_theme = theme::switcher();
    b_theme.add_css_class("ws-winbtn");
    b_theme.set_valign(gtk::Align::Center);
    h.append(&b_theme);
    let b_min = icon_button("win-min", "", "Minimieren");
    let b_max = icon_button("win-max", "", "Maximieren");
    let b_close = icon_button("close", "", "Schließen");
    for b in [&b_min, &b_max, &b_close] { b.add_css_class("ws-winbtn"); b.set_valign(gtk::Align::Center); h.append(b); }
    b_close.add_css_class("ws-winclose");
    let w = window.clone();
    b_min.connect_clicked(move |_| w.minimize());
    let w = window.clone();
    b_max.connect_clicked(move |_| { if w.is_maximized() { w.unmaximize() } else { w.maximize() } });
    let w = window.clone();
    b_close.connect_clicked(move |_| w.close());
    // Der Lichtstreifen liegt als Zeichnung hinter dem Inhalt: Er verdeckt keine Klickfläche.
    let glow = gtk::DrawingArea::new();
    glow.set_draw_func(|_, cr, w, hgt| {
        theme::set(cr, "grund");
        let _ = cr.paint();
        // links neben den Fensterknöpfen enden lassen
        icons::draw_header_neon(cr, w as f64 - 150.0, hgt as f64);
        theme::set(cr, "linie");
        cr.rectangle(0.0, hgt as f64 - 1.0, w as f64, 1.0);
        let _ = cr.fill();
    });
    let ov = gtk::Overlay::new();
    ov.set_child(Some(&glow));
    ov.add_overlay(&h);
    ov.set_measure_overlay(&h, true);
    gtk::WindowHandle::builder().child(&ov).build()
}

pub const CSS: &str = r#"
window, .ws-root { background: #041318; color: #f5f8fa; font-family: "Inter", "Cantarell", "Noto Sans", sans-serif; }
* { outline-color: alpha(#00e676, 0.6); }
.ws-panel { background: #06151d; }
.ws-side { background: #041318; border-right: 1px solid #12262c; }
.ws-toolbar { background: #06151d; border-bottom: 1px solid #12262c; padding: 6px 8px; }
.ws-bottombar { background: #06151d; border-top: 1px solid #12262c; padding: 4px 8px; }
.ws-title { font-size: 26px; font-weight: 700; color: #f4f4f8; }
.ws-logo { font-size: 20px; font-weight: 800; color: #ffffff; }
.ws-h { font-size: 13px; font-weight: 700; color: #f0f0f5; }
.ws-dim { color: #8fa3a8; }
.ws-small { font-size: 12px; }
.ws-time { font-family: monospace; font-size: 15px; color: #f4f4f8; }
.ws-bigtime { font-family: monospace; font-size: 22px; font-weight: 700; color: #f4f4f8; }
button { background: #0b2028; color: #f5f8fa; border: 1px solid #1c343c; border-radius: 8px; padding: 5px 10px; box-shadow: none; text-shadow: none; background-image: none; min-height: 22px; }
button:hover { background: #12303a; }
button:active, button:checked { background: #00e676; color: #02140f; border-color: #00e676; }
button:disabled { color: #4d646b; background: #081a20; }
button.ws-flat { background: transparent; border-color: transparent; padding: 5px 7px; }
button.ws-flat:hover { background: #0e2830; }
button.ws-flat:checked { background: #00e676; }
button.ws-cutall { font-size: 12px; padding: 5px 10px; border: 1px solid #1c343c; }
button.ws-cutall:checked { color: #02140f; border-color: #00e676; }
button.ws-primary { background: #00e676; color: #02140f; font-weight: 700; border-color: #00e676; padding: 6px 16px; }
button.ws-primary:hover { background: #4df0a0; }
button.ws-rec { background: #e5383b; color: #ffffff; font-weight: 800; border-color: #e5383b; border-radius: 999px; padding: 14px 26px; font-size: 16px; }
button.ws-rec:hover { background: #f25558; }
button.ws-tile { background: #06151d; border: 1px solid #12262c; border-radius: 14px; padding: 18px 14px; }
button.ws-tile:hover { background: #0b2229; border-color: #00e676; }
.ws-rail { background: #021016; border-right: 1px solid #12262c; padding: 6px 4px; }
button.ws-railbtn { background: transparent; border-color: transparent; border-radius: 10px; padding: 9px 9px; color: #b4c4c8; }
button.ws-railbtn:hover { background: #0b2229; }
button.ws-railbtn:checked { background: #06251f; color: #00e676; border-color: #008853; }
button.ws-action { padding: 8px 10px; }
button.ws-fxtile { padding: 5px 2px 6px 2px; border-radius: 10px; background: #06151d; border: 1px solid #12262c; }
button.ws-fxtile:hover { background: #0b2229; border-color: #00e676; }
button.ws-fxtile label { font-size: 11px; }
.ws-card { background: #06151d; border: 1px solid #12262c; border-radius: 14px; padding: 12px; }
.ws-card-off { background: #04121a; }
entry, spinbutton, textview, dropdown > button, combobox button { background: #041319; color: #f5f8fa; border: 1px solid #1c343c; border-radius: 6px; min-height: 26px; }
textview text { background: #041319; color: #f5f8fa; }
spinbutton button { border: none; background: transparent; padding: 0 2px; min-height: 0; min-width: 20px; }
popover contents, popover { background: #081a20; color: #f5f8fa; }
scale trough { background: #173038; min-height: 4px; border-radius: 4px; }
scale highlight { background: #00e676; border-radius: 4px; }
scale slider { background: #f0f0f5; min-width: 14px; min-height: 14px; border-radius: 999px; margin: -6px; }
switch { background: #1b343c; border-radius: 999px; }
switch:checked { background: #00e676; }
switch slider { background: #f4f4f8; border-radius: 999px; }
checkbutton check { background: #041319; border: 1px solid #2a4850; border-radius: 4px; }
checkbutton check:checked { background: #00e676; border-color: #00e676; }
list, listview, .ws-list { background: transparent; }
list row { border-radius: 8px; padding: 4px 6px; }
list row:hover { background: #0b2229; }
list row:selected { background: #06251f; }
separator { background: #12262c; min-height: 1px; min-width: 1px; }
scrollbar { background: transparent; }
scrollbar slider { background: #1d3840; border-radius: 8px; min-width: 6px; min-height: 6px; }
tooltip { background: #12262c; color: #f0f0f5; border-radius: 6px; }
progressbar trough { background: #173038; min-height: 8px; border-radius: 8px; }
progressbar progress { background: #00e676; min-height: 8px; border-radius: 8px; }
stackswitcher button { border-radius: 8px; margin: 2px; }
expander title { font-weight: 700; padding: 4px 0; }
.ws-note { background: #2b2417; border: 1px solid #5e4a22; border-radius: 8px; padding: 8px 10px; color: #f1d9a8; }
.ws-err { background: #2e1a1b; border: 1px solid #6d2b2e; border-radius: 8px; padding: 8px 10px; color: #f5b5b7; }
.ws-header { background: transparent; padding: 6px 10px; min-height: 34px; }
.ws-brand { font-size: 15px; font-weight: 800; color: #ffffff; }
.ws-brand-sub { font-size: 13px; color: #b4c4c8; }
menubutton.ws-winbtn > button { background: transparent; border-color: transparent; border-radius: 8px; padding: 5px 9px; color: #dfe9ea; }
menubutton.ws-winbtn > button:hover { background: #12303a; }
button.ws-winbtn { background: transparent; border-color: transparent; border-radius: 8px; padding: 5px 9px; color: #dfe9ea; }
button.ws-winbtn:hover { background: #12303a; }
button.ws-winclose:hover { background: #c62f3a; color: #ffffff; }
button.ws-tile { box-shadow: none; }
button.ws-railbtn:checked { box-shadow: inset 0 0 0 1px #00a865; }
"#;

pub fn install_css() {
    theme::init();
    if let Some(s) = gtk::Settings::default() { s.set_gtk_application_prefer_dark_theme(true); }
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in class.split_whitespace() { l.add_css_class(c); }
    l
}

pub fn hbox(spacing: i32) -> gtk::Box { gtk::Box::new(gtk::Orientation::Horizontal, spacing) }
pub fn vbox(spacing: i32) -> gtk::Box { gtk::Box::new(gtk::Orientation::Vertical, spacing) }

/// Knopf mit selbst gezeichnetem Symbol und optionalem Text.
pub fn icon_button(icon: &'static str, text: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_child(Some(&icon_content(icon, text)));
    if !tip.is_empty() { b.set_tooltip_text(Some(tip)); }
    b
}
pub fn icon_toggle(icon: &'static str, text: &str, tip: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::new();
    b.set_child(Some(&icon_content(icon, text)));
    if !tip.is_empty() { b.set_tooltip_text(Some(tip)); }
    b
}
fn icon_content(icon: &'static str, text: &str) -> gtk::Box {
    let h = hbox(6);
    h.set_halign(gtk::Align::Center);
    h.append(&icons::widget(icon, 18));
    if !text.is_empty() { h.append(&gtk::Label::new(Some(text))); }
    h
}

pub fn rgba(hex: &str) -> gdk::RGBA {
    let (r, g, b, a) = crate::model::parse_color(hex);
    gdk::RGBA::new(r as f32, g as f32, b as f32, a as f32)
}
pub fn hex(c: &gdk::RGBA) -> String {
    crate::model::color_hex(c.red() as f64, c.green() as f64, c.blue() as f64, c.alpha() as f64)
}

/// Bilddaten (BGRx) als Cairo-Fläche.
pub fn frame_surface(f: &Frame) -> Option<cairo::ImageSurface> {
    cairo::ImageSurface::create_for_data(f.data.clone(), cairo::Format::Rgb24, f.w, f.h, f.stride).ok()
}

/// Fläche in ein Rechteck zeichnen: `cover` füllt das Rechteck (beschneidet), sonst wird eingepasst.
pub fn draw_surface(cr: &cairo::Context, s: &cairo::ImageSurface, x: f64, y: f64, w: f64, h: f64, cover: bool) {
    let (sw, sh) = (s.width() as f64, s.height() as f64);
    if sw <= 0.0 || sh <= 0.0 || w <= 0.0 || h <= 0.0 { return; }
    let k = if cover { (w / sw).max(h / sh) } else { (w / sw).min(h / sh) };
    let (dw, dh) = (sw * k, sh * k);
    let _ = cr.save();
    cr.rectangle(x, y, w, h);
    cr.clip();
    cr.translate(x + (w - dw) / 2.0, y + (h - dh) / 2.0);
    cr.scale(k, k);
    let _ = cr.set_source_surface(s, 0.0, 0.0);
    cr.source().set_filter(cairo::Filter::Bilinear);
    let _ = cr.paint();
    let _ = cr.restore();
}

pub fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 1.5 * pi);
    cr.close_path();
}

/// Text mit Pango zeichnen; liefert die Breite.
pub fn draw_text(cr: &cairo::Context, text: &str, x: f64, y: f64, size: f64, color: (f64, f64, f64, f64), bold: bool, max_w: f64) -> f64 {
    let layout = pangocairo::functions::create_layout(cr);
    let mut fd = gtk::pango::FontDescription::from_string("Sans");
    fd.set_absolute_size(size * gtk::pango::SCALE as f64);
    if bold { fd.set_weight(gtk::pango::Weight::Bold); }
    layout.set_font_description(Some(&fd));
    layout.set_text(text);
    if max_w > 0.0 {
        layout.set_width((max_w * gtk::pango::SCALE as f64) as i32);
        layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
    }
    cr.set_source_rgba(color.0, color.1, color.2, color.3);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
    layout.pixel_size().0 as f64
}

pub fn message(parent: &impl IsA<gtk::Window>, title: &str, detail: &str) {
    let d = gtk::AlertDialog::builder().message(title).detail(detail).modal(true).buttons(["Schließen"]).build();
    d.show(Some(parent));
}

/// Kleine Pegelanzeige (0..1), wird von außen mit Werten versorgt.
pub fn level_bar() -> (gtk::DrawingArea, std::rc::Rc<std::cell::Cell<f32>>) {
    let value = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let area = gtk::DrawingArea::new();
    area.set_content_height(8);
    area.set_content_width(120);
    area.set_hexpand(true);
    let v = value.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        rounded_rect(cr, 0.0, 0.0, w, h, h / 2.0);
        theme::set(cr, "rinne");
        let _ = cr.fill();
        // logarithmische Darstellung: -50 dB .. 0 dB
        let lin = v.get().max(1e-5) as f64;
        let db = 20.0 * lin.log10();
        let k = ((db + 50.0) / 50.0).clamp(0.0, 1.0);
        if k > 0.0 {
            rounded_rect(cr, 0.0, 0.0, w * k, h, h / 2.0);
            if k > 0.92 { cr.set_source_rgb(0.9, 0.22, 0.23); } else if k > 0.75 { cr.set_source_rgb(0.98, 0.8, 0.25); } else { cr.set_source_rgb(0.33, 0.8, 0.45); }
            let _ = cr.fill();
        }
    });
    (area, value)
}

pub fn spawn_local<F: std::future::Future<Output = ()> + 'static>(f: F) {
    glib::MainContext::default().spawn_local(f);
}
