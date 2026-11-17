//! Designs (Themen) der Oberfläche.
//!
//! Ein Design ist eine Liste benannter Farben. Das Stylesheet in `ui::CSS` ist im grünen Grunddesign geschrieben;
//! für ein anderes Design werden dessen Farben an die Stelle der grünen gesetzt. Alles, was das Programm selbst
//! zeichnet (Zeitleiste, Vorschau, Zeichen, Lichtstreifen), fragt seine Farben hier ab.
//!
//! Eingebaut sind „Grün“ und „Electric Blue“. Eigene Designs liegen als JSON-Dateien in
//! `~/.config/ws-video/themen/`: `{"name": "...", "farben": {"akzent": "#ff8800", ...}, "css": "..."}`.
//! Farben, die fehlen, kommen aus dem grünen Grunddesign.
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

/// Farben des Stylesheets: Name und Wert im grünen Grunddesign. Jeder Wert kommt nur einmal vor.
const BASE: &[(&str, &str)] = &[
    ("grund", "#041318"), ("feld", "#041319"), ("flaeche", "#06151d"), ("leiste", "#021016"), ("karte_aus", "#04121a"),
    ("linie", "#12262c"), ("rand", "#1c343c"), ("knopf", "#0b2028"), ("knopf_maus", "#12303a"), ("flach_maus", "#0e2830"),
    ("zeile_maus", "#0b2229"), ("inaktiv", "#081a20"), ("rinne", "#173038"), ("schalter", "#1b343c"), ("haken_rand", "#2a4850"),
    ("rollbalken", "#1d3840"), ("akzent", "#00e676"), ("akzent_hell", "#4df0a0"), ("auf_akzent", "#02140f"), ("auswahl", "#06251f"),
    ("auswahl_rand", "#008853"), ("auswahl_ring", "#00a865"), ("text", "#f5f8fa"), ("text_matt", "#b4c4c8"), ("text_leise", "#8fa3a8"),
    ("text_aus", "#4d646b"), ("text_fenster", "#dfe9ea"),
];
/// Farben, die nur gezeichnet werden (Vorschau, Zeichen „W.“, Lichtstreifen)
const DRAW: &[(&str, &str)] = &[
    ("buehne", "#000b10"), ("leuchten", "#00f5a3"), ("logo_hell", "#3cffcc"), ("logo_mitte", "#00efa5"), ("logo_dunkel", "#00e676"),
    ("logo_punkt", "#00e676"), ("logo_glanz", "#9cffe6"),
];
const BLUE: &[(&str, &str)] = &[
    ("grund", "#020a13"), ("feld", "#071624"), ("flaeche", "#081b2c"), ("leiste", "#03111e"), ("karte_aus", "#051524"),
    ("linie", "#17344a"), ("rand", "#29465b"), ("knopf", "#0c2237"), ("knopf_maus", "#12314a"), ("flach_maus", "#0e2a40"),
    ("zeile_maus", "#0d2538"), ("inaktiv", "#071726"), ("rinne", "#163148"), ("schalter", "#1b3a52"), ("haken_rand", "#2b4a61"),
    ("rollbalken", "#1f4058"), ("akzent", "#00b8ff"), ("akzent_hell", "#58e0ff"), ("auf_akzent", "#001426"), ("auswahl", "#092c4b"),
    ("auswahl_rand", "#1a7cb6"), ("auswahl_ring", "#00a8f0"), ("text", "#f4f9ff"), ("text_matt", "#b9c8d7"), ("text_leise", "#8ca4b8"),
    ("text_aus", "#4e677c"), ("text_fenster", "#e0ebf5"),
    ("buehne", "#010f20"), ("leuchten", "#00dcff"), ("logo_hell", "#39f3ff"), ("logo_mitte", "#00cfff"), ("logo_dunkel", "#0089ff"),
    ("logo_punkt", "#00d9ff"), ("logo_glanz", "#9cebff"),
];
const BLUE_CSS: &str = r#"
button.ws-primary { background-image: linear-gradient(135deg, #00e6ff, #00b4ff 46%, #007bff); border-color: #00cfff; box-shadow: 0 0 10px alpha(#00d1ff, 0.35); }
button.ws-primary:hover { background-image: linear-gradient(135deg, #58f3ff, #00a7ff); }
button.ws-railbtn:checked { color: #00dcff; box-shadow: inset 0 0 0 1px #00b8ff, 0 0 12px alpha(#00a3ff, 0.25); }
"#;

pub struct Theme {
    pub id: String,
    pub name: String,
    hex: HashMap<String, String>,
    rgb: HashMap<String, (f64, f64, f64)>,
    css: String,
}

fn parse(hex: &str) -> Option<(f64, f64, f64)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) { return None; }
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
    Some((v(0), v(2), v(4)))
}

impl Theme {
    fn build(id: &str, name: &str, colors: &[(String, String)], css: &str) -> Theme {
        let mut hex: HashMap<String, String> = BASE.iter().chain(DRAW.iter()).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        for (k, v) in colors {
            // nur bekannte Namen und gültige Farbwerte übernehmen
            if hex.contains_key(k) && parse(v).is_some() { hex.insert(k.clone(), format!("#{}", v.trim().trim_start_matches('#').to_lowercase())); }
        }
        let rgb = hex.iter().filter_map(|(k, v)| parse(v).map(|c| (k.clone(), c))).collect();
        Theme { id: id.to_string(), name: name.to_string(), hex, rgb, css: css.to_string() }
    }
    fn green() -> Theme { Theme::build("gruen", "Grün", &[], "") }
    fn blue() -> Theme {
        let c: Vec<(String, String)> = BLUE.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Theme::build("blau", "Electric Blue", &c, BLUE_CSS)
    }
    /// Stylesheet dieses Designs: das Grund-Stylesheet mit ausgetauschten Farben, dahinter die eigenen Regeln.
    fn stylesheet(&self) -> String {
        let mut s = super::CSS.to_string();
        for (k, v) in BASE { s = s.replace(v, &format!("@@{k}@@")); }
        for (k, _) in BASE { s = s.replace(&format!("@@{k}@@"), self.hex.get(*k).map(|x| x.as_str()).unwrap_or("#ff00ff")); }
        s.push_str(&self.css);
        s
    }
    /// Als JSON, wie es in `themen/` liegen kann
    pub fn to_json(&self) -> String {
        let mut farben = serde_json::Map::new();
        for (k, _) in BASE.iter().chain(DRAW.iter()) { farben.insert(k.to_string(), serde_json::Value::String(self.hex[*k].clone())); }
        serde_json::to_string_pretty(&serde_json::json!({ "name": self.name, "farben": farben, "css": self.css })).unwrap_or_default()
    }
}

pub fn dir() -> PathBuf { gtk::glib::user_config_dir().join("ws-video").join("themen") }
fn choice_file() -> PathBuf { gtk::glib::user_config_dir().join("ws-video").join("thema.txt") }

fn load_file(path: &std::path::Path) -> Option<Theme> {
    let id = path.file_stem()?.to_str()?.to_string();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or(&id).to_string();
    let colors: Vec<(String, String)> = v.get("farben").and_then(|f| f.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default();
    let css = v.get("css").and_then(|c| c.as_str()).unwrap_or("");
    Some(Theme::build(&id, &name, &colors, css))
}

/// Alle Designs: die eingebauten, dann die eigenen aus dem Ordner `themen`.
pub fn all() -> Vec<Theme> {
    let mut v = vec![Theme::green(), Theme::blue()];
    if let Ok(rd) = std::fs::read_dir(dir()) {
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map(|x| x == "json").unwrap_or(false)).collect();
        files.sort();
        for f in files {
            if let Some(t) = load_file(&f) { if !v.iter().any(|o| o.id == t.id) { v.push(t); } }
        }
    }
    v
}

thread_local! {
    static CUR: RefCell<Theme> = RefCell::new(Theme::green());
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static HOOKS: RefCell<Vec<Box<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
}

pub fn current_id() -> String { CUR.with(|c| c.borrow().id.clone()) }
pub fn rgb(name: &str) -> (f64, f64, f64) { CUR.with(|c| c.borrow().rgb.get(name).copied().unwrap_or((1.0, 0.0, 1.0))) }
pub fn hex(name: &str) -> String { CUR.with(|c| c.borrow().hex.get(name).cloned().unwrap_or_else(|| "#ff00ff".into())) }
pub fn accent() -> (f64, f64, f64) { rgb("akzent") }
/// Farbe als Zeichenfarbe setzen
pub fn set(cr: &gtk::cairo::Context, name: &str) { let c = rgb(name); cr.set_source_rgb(c.0, c.1, c.2); }
pub fn set_a(cr: &gtk::cairo::Context, name: &str, a: f64) { let c = rgb(name); cr.set_source_rgba(c.0, c.1, c.2, a); }
/// Wird nach jedem Wechsel des Designs aufgerufen (für Dinge, die ihre Farbe als Text tragen).
pub fn on_change(f: impl Fn() + 'static) { HOOKS.with(|h| h.borrow_mut().push(Box::new(f))); }

fn redraw(w: &gtk::Widget) {
    if w.is::<gtk::DrawingArea>() { w.queue_draw(); }
    let mut c = w.first_child();
    while let Some(ch) = c { redraw(&ch); c = ch.next_sibling(); }
}

fn load(theme: Theme) {
    let css = theme.stylesheet();
    CUR.with(|c| *c.borrow_mut() = theme);
    PROVIDER.with(|p| {
        let mut p = p.borrow_mut();
        let provider = p.get_or_insert_with(|| {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            }
            provider
        });
        #[allow(deprecated)]
        provider.load_from_data(&css);
    });
}

/// Beim Start: das zuletzt gewählte Design laden.
pub fn init() {
    let want = std::env::var("WS_THEMA").ok().or_else(|| std::fs::read_to_string(choice_file()).ok()).map(|s| s.trim().to_string()).unwrap_or_default();
    let theme = all().into_iter().find(|t| t.id == want).unwrap_or_else(Theme::green);
    load(theme);
}

/// Design wechseln, merken und alles neu zeichnen.
pub fn apply(id: &str) {
    let Some(theme) = all().into_iter().find(|t| t.id == id) else { return };
    load(theme);
    let f = choice_file();
    if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
    let _ = std::fs::write(f, id);
    HOOKS.with(|h| for f in h.borrow().iter() { f(); });
    for t in gtk::Window::list_toplevels() { redraw(&t); }
}

/// Knopf für die Kopfzeile: öffnet die Auswahl der Designs.
pub fn switcher() -> gtk::MenuButton {
    let b = gtk::MenuButton::new();
    b.set_child(Some(&super::icons::widget("palette", 20)));
    b.set_tooltip_text(Some("Design wählen"));
    let pop = gtk::Popover::new();
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    list.set_margin_top(6);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);
    pop.set_child(Some(&list));
    b.set_popover(Some(&pop));
    let fill = {
        let (list, pop) = (list.clone(), pop.downgrade());
        move || {
            while let Some(c) = list.first_child() { list.remove(&c); }
            list.append(&super::label("Design", "ws-h"));
            let cur = current_id();
            for t in all() {
                let row = gtk::Button::new();
                row.add_css_class("ws-flat");
                let h = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                let (bg, ac) = (t.rgb["grund"], t.rgb["akzent"]);
                let sw = gtk::DrawingArea::new();
                sw.set_content_width(22);
                sw.set_content_height(22);
                sw.set_draw_func(move |_, cr, w, hh| {
                    let (w, hh) = (w as f64, hh as f64);
                    cr.arc(w / 2.0, hh / 2.0, w / 2.0 - 1.0, 0.0, std::f64::consts::TAU);
                    cr.set_source_rgb(bg.0, bg.1, bg.2);
                    let _ = cr.fill_preserve();
                    cr.set_source_rgb(ac.0, ac.1, ac.2);
                    cr.set_line_width(2.0);
                    let _ = cr.stroke();
                    cr.arc(w / 2.0, hh / 2.0, w / 4.5, 0.0, std::f64::consts::TAU);
                    let _ = cr.fill();
                });
                h.append(&sw);
                let l = gtk::Label::new(Some(&t.name));
                l.set_xalign(0.0);
                l.set_hexpand(true);
                h.append(&l);
                if t.id == cur { h.append(&gtk::Label::new(Some("✓"))); }
                row.set_child(Some(&h));
                let (id, pop) = (t.id.clone(), pop.clone());
                row.connect_clicked(move |_| {
                    if let Some(p) = pop.upgrade() { p.popdown(); }
                    apply(&id);
                });
                list.append(&row);
            }
            let hint = super::label(&format!("Eigene Designs: JSON-Dateien in\n{}", dir().display()), "ws-dim ws-small");
            hint.set_margin_top(4);
            list.append(&hint);
        }
    };
    pop.connect_show(move |_| fill());
    b
}
