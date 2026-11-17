//! Eigenschaften des ausgewählten Clips (rechte Seite) bzw. des Projekts.
#![allow(unused_braces)]
use super::editor::Editor;
use super::{canvas, hbox, label, vbox};
use crate::cursordata::CursorData;
use crate::model::*;
use crate::ops;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

/// Breite der rechten Leiste. Daraus ergibt sich für alle Felder dieselbe Länge: gerade so viel, dass der
/// längste Eintrag („1920 × 1080 (Full HD)“) hineinpasst.
pub const PANEL_W: i32 = 316;
/// Breite der Beschriftung vor einem Feld (Zeichen)
const LABEL_CHARS: i32 = 10;

fn section(root: &gtk::Box, title: &str) -> gtk::Box {
    let b = vbox(6);
    b.set_margin_top(10);
    let l = label(title, "ws-h");
    b.append(&l);
    root.append(&b);
    b
}
fn row(parent: &gtk::Box, text: &str, w: &impl IsA<gtk::Widget>) {
    let h = hbox(8);
    let l = label(text, "ws-dim ws-small");
    l.set_width_chars(LABEL_CHARS);
    h.append(&l);
    w.set_hexpand(true);
    h.append(w);
    parent.append(&h);
}
fn spin(min: f64, max: f64, step: f64, digits: u32) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(min, max, step);
    s.set_digits(digits);
    s.set_width_chars(6);
    s
}
fn scale(min: f64, max: f64, step: f64) -> gtk::Scale {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
    s.set_draw_value(false);
    s.set_hexpand(true);
    s
}
fn color_button() -> gtk::ColorDialogButton {
    let d = gtk::ColorDialog::new();
    d.set_with_alpha(true);
    gtk::ColorDialogButton::new(Some(d))
}

pub struct Props {
    pub root: gtk::Box,
    ed: Weak<Editor>,
    updating: Cell<bool>,
    shown_clip: Cell<u64>,
    cursor_cache: RefCell<HashMap<String, Rc<CursorData>>>,
    s_project: gtk::Box, s_head: gtk::Box, s_geo: gtk::Box, s_crop: gtk::Box, s_shape: gtk::Box, s_chroma: gtk::Box, s_bg: gtk::Box, bg_mode: gtk::DropDown, b_bg: gtk::Button, s_fade: gtk::Box,
    s_anim: gtk::Box, s_audio: gtk::Box, s_text: gtk::Box, s_colors: gtk::Box, s_arrow: gtk::Box, s_blur: gtk::Box, s_cursor: gtk::Box,
    title: gtk::Label, name: gtk::Entry, time_info: gtk::Label,
    x: gtk::SpinButton, y: gtk::SpinButton, w: gtk::SpinButton, h: gtk::SpinButton, opacity: gtk::Scale,
    crop: [gtk::Scale; 4],
    shape: gtk::DropDown, radius: gtk::Scale, border: gtk::SpinButton, border_color: gtk::ColorDialogButton, shadow: gtk::CheckButton, border_row: gtk::Box, b_circle: gtk::Button,
    chroma: gtk::CheckButton, chroma_color: gtk::ColorDialogButton, chroma_tol: gtk::Scale,
    fade_in: gtk::SpinButton, fade_out: gtk::SpinButton, fade_in_kind: gtk::DropDown, fade_out_kind: gtk::DropDown,
    anim_list: gtk::Box,
    volume: gtk::Scale, muted: gtk::CheckButton, volume_label: gtk::Label,
    a_denoise: gtk::Scale, a_lowcut: gtk::CheckButton, a_compress: gtk::Scale, a_normalize: gtk::CheckButton, a_voice: gtk::CheckButton,
    a_gate: gtk::Scale, a_bass: gtk::Scale, a_treble: gtk::Scale, a_status: gtk::Label,
    text: gtk::TextView, font: gtk::FontDialogButton, font_size: gtk::SpinButton, bold: gtk::ToggleButton, italic: gtk::ToggleButton, align: gtk::DropDown, text_color: gtk::ColorDialogButton, outline: gtk::SpinButton, outline_color: gtk::ColorDialogButton, sel_color: gtk::ColorDialogButton, gfont: gtk::Entry, b_gfont: gtk::Button,
    fill: gtk::ColorDialogButton, stroke: gtk::ColorDialogButton, stroke_w: gtk::SpinButton,
    arrow_dir: gtk::DropDown, arrow_head: gtk::CheckButton,
    blur: gtk::Scale, pixelate: gtk::CheckButton,
    s_look: gtk::Box, brightness: gtk::Scale, contrast: gtk::Scale, saturation: gtk::Scale, temperature: gtk::Scale, vignette: gtk::Scale, flip_h: gtk::CheckButton, flip_v: gtk::CheckButton,
    cur_show: gtk::CheckButton, cur_scale: gtk::Scale, cur_smooth: gtk::Scale, cur_hl: gtk::CheckButton, cur_hl_color: gtk::ColorDialogButton,
    cur_clicks: gtk::CheckButton, cur_click_color: gtk::ColorDialogButton, cur_monitor: gtk::DropDown, cur_info: gtk::Label,
    proj_size: gtk::DropDown, proj_w: gtk::SpinButton, proj_h: gtk::SpinButton, proj_fps: gtk::DropDown,
}

const SIZES: [(u32, u32, &str); 5] = [(1920, 1080, "1920 × 1080 (Full HD)"), (2560, 1440, "2560 × 1440 (2K)"), (3840, 2160, "3840 × 2160 (4K)"), (1280, 720, "1280 × 720 (HD)"), (1080, 1920, "1080 × 1920 (Hochformat)")];
const FPS: [u32; 4] = [30, 60, 25, 24];

impl Props {
    pub fn new(ed: &Rc<Editor>) -> Rc<Props> {
        let root = vbox(2);
        root.set_margin_start(12);
        root.set_margin_end(12);
        root.set_margin_top(6);
        root.set_margin_bottom(16);

        // Projekt (wenn nichts ausgewählt ist)
        let s_project = section(&root, "Projekt");
        let names: Vec<&str> = SIZES.iter().map(|s| s.2).chain(std::iter::once("Eigene Größe")).collect();
        let proj_size = gtk::DropDown::from_strings(&names);
        row(&s_project, "Größe", &proj_size);
        let proj_w = spin(320.0, 7680.0, 2.0, 0);
        let proj_h = spin(240.0, 4320.0, 2.0, 0);
        row(&s_project, "Breite", &proj_w);
        row(&s_project, "Höhe", &proj_h);
        let proj_fps = gtk::DropDown::from_strings(&["30 Bilder/s", "60 Bilder/s", "25 Bilder/s", "24 Bilder/s"]);
        row(&s_project, "Bildrate", &proj_fps);
        let ph = label("Wähle einen Clip in der Vorschau oder in der Zeitleiste, um ihn hier anzupassen.", "ws-dim ws-small");
        ph.set_wrap(true);
        ph.set_margin_top(8);
        s_project.append(&ph);
        let b_saveas = gtk::Button::with_label("Projekt speichern unter …");
        b_saveas.set_margin_top(8);
        s_project.append(&b_saveas);

        // Kopf
        let s_head = vbox(6);
        s_head.set_margin_top(8);
        let title = label("", "ws-h");
        s_head.append(&title);
        let name = gtk::Entry::new();
        row(&s_head, "Name", &name);
        let time_info = label("", "ws-dim ws-small");
        s_head.append(&time_info);
        root.append(&s_head);

        // Position und Größe
        let s_geo = section(&root, "Position und Größe");
        let (x, y, w, h) = (spin(-20000.0, 20000.0, 1.0, 0), spin(-20000.0, 20000.0, 1.0, 0), spin(1.0, 40000.0, 1.0, 0), spin(1.0, 40000.0, 1.0, 0));
        let xy = hbox(6);
        xy.append(&label("X", "ws-dim ws-small"));
        xy.append(&x);
        xy.append(&label("Y", "ws-dim ws-small"));
        xy.append(&y);
        s_geo.append(&xy);
        let wh = hbox(6);
        wh.append(&label("B", "ws-dim ws-small"));
        wh.append(&w);
        wh.append(&label("H", "ws-dim ws-small"));
        wh.append(&h);
        s_geo.append(&wh);
        let geo_btns = hbox(6);
        let b_fit = gtk::Button::with_label("Einpassen");
        let b_fill = gtk::Button::with_label("Füllen");
        let b_center = gtk::Button::with_label("Zentrieren");
        for b in [&b_fit, &b_fill, &b_center] { b.set_hexpand(true); geo_btns.append(b); }
        s_geo.append(&geo_btns);
        let opacity = scale(0.0, 100.0, 1.0);
        row(&s_geo, "Deckkraft", &opacity);

        // Zuschnitt
        let s_crop = section(&root, "Zuschnitt");
        let crop = [scale(0.0, 90.0, 0.5), scale(0.0, 90.0, 0.5), scale(0.0, 90.0, 0.5), scale(0.0, 90.0, 0.5)];
        for (i, n) in ["Links", "Oben", "Rechts", "Unten"].iter().enumerate() { row(&s_crop, n, &crop[i]); }
        let b_crop_reset = gtk::Button::with_label("Zuschnitt zurücksetzen");
        s_crop.append(&b_crop_reset);

        // Form und Rahmen
        let s_shape = section(&root, "Form und Rahmen");
        let shape = gtk::DropDown::from_strings(&["Rechteck", "Abgerundet", "Ellipse / Kreis"]);
        row(&s_shape, "Form", &shape);
        let b_circle = gtk::Button::with_label("Als Kreis (mittig zuschneiden)");
        s_shape.append(&b_circle);
        let radius = scale(0.0, 100.0, 1.0);
        row(&s_shape, "Rundung", &radius);
        let border_row = hbox(8);
        let border = spin(0.0, 200.0, 1.0, 0);
        let border_color = color_button();
        let bl = label("Rahmen", "ws-dim ws-small");
        bl.set_width_chars(LABEL_CHARS);
        border_row.append(&bl);
        border_row.append(&border);
        border_row.append(&border_color);
        s_shape.append(&border_row);
        let shadow = gtk::CheckButton::with_label("Schatten");
        s_shape.append(&shadow);

        // Kamera-Hintergrund
        let s_bg = section(&root, "Hintergrund ersetzen");
        let bg_mode = gtk::DropDown::from_strings(&["Aus (Original)", "Entfernen (durchsichtig)", "Unscharf", "Eigenes Bild"]);
        row(&s_bg, "Hintergrund", &bg_mode);
        let b_bg = gtk::Button::with_label("Bild wählen …");
        s_bg.append(&b_bg);
        let bg_info = label("Erkennt die Person im Kamerabild, ganz ohne grüne Wand. Bei „Entfernen“ scheint durch, was auf den Spuren darunter liegt.", "ws-dim ws-small");
        bg_info.set_wrap(true);
        s_bg.append(&bg_info);

        // Greenscreen
        let s_chroma = section(&root, "Greenscreen");
        let chroma = gtk::CheckButton::with_label("Farbe freistellen");
        s_chroma.append(&chroma);
        let chroma_color = color_button();
        row(&s_chroma, "Farbe", &chroma_color);
        let chroma_tol = scale(1.0, 100.0, 1.0);
        row(&s_chroma, "Toleranz", &chroma_tol);

        // Blenden
        let s_fade = section(&root, "Anfang und Ende");
        let fade_in = spin(0.0, 30.0, 0.1, 1);
        let fade_out = spin(0.0, 30.0, 0.1, 1);
        let kinds_in: Vec<&str> = FadeKind::ALL.iter().map(|k| k.1).collect();
        let kinds_out: Vec<&str> = FadeKind::ALL.iter().map(|k| k.2).collect();
        let fade_in_kind = gtk::DropDown::from_strings(&kinds_in);
        let fade_out_kind = gtk::DropDown::from_strings(&kinds_out);
        row(&s_fade, "Anfang", &fade_in_kind);
        row(&s_fade, "Dauer (s)", &fade_in);
        row(&s_fade, "Ende", &fade_out_kind);
        row(&s_fade, "Dauer (s)", &fade_out);
        let fh = label("Für einen Übergang zwischen zwei Clips: beim ersten Clip das Ende, beim zweiten den Anfang auf dieselbe Art stellen. Das Menü Übergänge links macht beides mit einem Klick.", "ws-dim ws-small");
        fh.set_wrap(true);
        s_fade.append(&fh);

        // Animation
        let s_anim = section(&root, "Zoom und Schwenk");
        let ab = hbox(6);
        let b_zoom = gtk::Button::with_label("Hineinzoomen");
        b_zoom.set_tooltip_text(Some("Fügt am Abspielkopf einen Zoom ein. Bei Bildschirmaufnahmen mit Zeigerdaten wird auf den Mauszeiger gezoomt."));
        let b_reset = gtk::Button::with_label("Zurück");
        b_reset.set_tooltip_text(Some("Fügt am Abspielkopf eine Bewegung zurück zur Ausgangsgröße ein."));
        let b_custom = gtk::Button::with_label("Eigene");
        b_custom.set_tooltip_text(Some("Fügt eine Animation ein. Danach den Clip in der Vorschau verschieben oder skalieren: das wird das Ziel der Bewegung."));
        for b in [&b_zoom, &b_reset, &b_custom] { b.set_hexpand(true); ab.append(b); }
        s_anim.append(&ab);
        let anim_list = vbox(4);
        s_anim.append(&anim_list);

        // Ton
        let s_audio = section(&root, "Ton");
        let volume = scale(0.0, 200.0, 1.0);
        let volume_label = label("100 %", "ws-dim ws-small");
        let vr = hbox(8);
        let vl = label("Lautstärke", "ws-dim ws-small");
        vl.set_width_chars(LABEL_CHARS);
        vr.append(&vl);
        vr.append(&volume);
        vr.append(&volume_label);
        s_audio.append(&vr);
        let muted = gtk::CheckButton::with_label("Stumm");
        s_audio.append(&muted);
        // Tonbearbeitung: wirkt auf alle Teile derselben Aufnahme
        let b_improve = gtk::Button::with_label("Ton verbessern");
        b_improve.set_tooltip_text(Some("Ein Klick: misst das Rauschen (zum Beispiel den Lüfter) und entfernt es, nimmt tiefes Poltern heraus, gleicht die Lautstärke an und bringt sie auf Normalmaß."));
        s_audio.append(&b_improve);
        let a_denoise = scale(0.0, 100.0, 1.0);
        a_denoise.set_tooltip_text(Some("Rauschen entfernen. Links aus, rechts stark. Das Rauschen wird in den Pausen der Aufnahme gemessen."));
        row(&s_audio, "Rauschen", &a_denoise);
        let a_lowcut = gtk::CheckButton::with_label("Brummen und Poltern entfernen");
        a_lowcut.set_tooltip_text(Some("Nimmt alles unter 80 Hertz heraus: Trittschall, Tischstöße, tiefes Brummen."));
        s_audio.append(&a_lowcut);
        let a_compress = scale(0.0, 100.0, 1.0);
        a_compress.set_tooltip_text(Some("Lautstärke angleichen: Leise und laute Stellen rücken näher zusammen."));
        row(&s_audio, "Angleichen", &a_compress);
        let a_normalize = gtk::CheckButton::with_label("Normale Lautstärke");
        a_normalize.set_tooltip_text(Some("Bringt die Aufnahme auf eine übliche Lautstärke, damit alle Aufnahmen ähnlich laut sind. Ein Begrenzer verhindert Übersteuern."));
        s_audio.append(&a_normalize);
        let a_voice = gtk::CheckButton::with_label("Stimme klarer");
        a_voice.set_tooltip_text(Some("Hebt den Bereich an, in dem Sprache verständlich wird, und nimmt etwas Dumpfes heraus."));
        s_audio.append(&a_voice);
        let a_gate = scale(0.0, 100.0, 1.0);
        a_gate.set_tooltip_text(Some("Pausen leiser: Zwischen den Sätzen wird der Ton abgesenkt."));
        row(&s_audio, "Pausen", &a_gate);
        let a_bass = scale(-12.0, 12.0, 0.5);
        a_bass.add_mark(0.0, gtk::PositionType::Bottom, None);
        a_bass.set_tooltip_text(Some("Tiefen anheben oder absenken"));
        row(&s_audio, "Tiefen", &a_bass);
        let a_treble = scale(-12.0, 12.0, 0.5);
        a_treble.add_mark(0.0, gtk::PositionType::Bottom, None);
        a_treble.set_tooltip_text(Some("Höhen anheben oder absenken"));
        row(&s_audio, "Höhen", &a_treble);
        let a_status = label("", "ws-dim ws-small");
        a_status.set_wrap(true);
        s_audio.append(&a_status);
        let b_areset = gtk::Button::with_label("Tonbearbeitung zurücksetzen");
        s_audio.append(&b_areset);

        // Text
        let s_text = section(&root, "Text");
        let text = gtk::TextView::new();
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        text.set_top_margin(6);
        text.set_bottom_margin(6);
        text.set_left_margin(6);
        text.set_right_margin(6);
        let tscroll = gtk::ScrolledWindow::builder().child(&text).min_content_height(70).hscrollbar_policy(gtk::PolicyType::Never).build();
        s_text.append(&tscroll);
        let fd = gtk::FontDialog::new();
        let font = gtk::FontDialogButton::new(Some(fd));
        font.set_level(gtk::FontLevel::Family);
        row(&s_text, "Schrift", &font);
        let font_size = spin(6.0, 800.0, 1.0, 0);
        let tr = hbox(6);
        let fl = label("Größe", "ws-dim ws-small");
        fl.set_width_chars(LABEL_CHARS);
        tr.append(&fl);
        tr.append(&font_size);
        let bold = gtk::ToggleButton::with_label("F");
        bold.set_tooltip_text(Some("Fett"));
        let italic = gtk::ToggleButton::with_label("K");
        italic.set_tooltip_text(Some("Kursiv"));
        tr.append(&bold);
        tr.append(&italic);
        s_text.append(&tr);
        let align = gtk::DropDown::from_strings(&["Links", "Mitte", "Rechts"]);
        row(&s_text, "Ausrichtung", &align);
        let text_color = color_button();
        row(&s_text, "Textfarbe", &text_color);
        let srow = hbox(6);
        let sel_color = color_button();
        sel_color.set_rgba(&super::rgba("#ff8a3d"));
        sel_color.set_tooltip_text(Some("Farbe für die markierten Buchstaben"));
        let b_sel_color = gtk::Button::with_label("Auswahl färben");
        b_sel_color.set_tooltip_text(Some("Oben im Textfeld Buchstaben markieren, Farbe wählen, dann hier drücken."));
        b_sel_color.set_hexpand(true);
        let b_sel_clear = super::icon_button("restart", "", "");
        b_sel_clear.set_tooltip_text(Some("Entfernt die eigene Farbe der markierten Buchstaben (ohne Markierung: aller Buchstaben)."));
        srow.append(&sel_color);
        srow.append(&b_sel_color);
        srow.append(&b_sel_clear);
        s_text.append(&srow);
        let grow = hbox(6);
        let gfont = gtk::Entry::new();
        gfont.set_placeholder_text(Some("Google-Schrift, z. B. Poppins"));
        gfont.set_hexpand(true);
        let b_gfont = gtk::Button::with_label("Laden");
        b_gfont.set_tooltip_text(Some("Lädt die freie Schrift von Google Fonts, installiert sie für dich und wählt sie für diesen Text aus."));
        grow.append(&gfont);
        grow.append(&b_gfont);
        s_text.append(&grow);
        let orow = hbox(8);
        let ol = label("Kontur", "ws-dim ws-small");
        ol.set_width_chars(LABEL_CHARS);
        let outline = spin(0.0, 60.0, 1.0, 0);
        outline.set_tooltip_text(Some("Breite der Kontur um die Buchstaben (0 = keine)"));
        let outline_color = color_button();
        orow.append(&ol);
        orow.append(&outline);
        orow.append(&outline_color);
        s_text.append(&orow);
        s_text.append(&label("Fertige Stile", "ws-dim ws-small"));
        let styles = gtk::FlowBox::new();
        styles.set_selection_mode(gtk::SelectionMode::None);
        styles.set_max_children_per_line(2);
        styles.set_min_children_per_line(2);
        styles.set_homogeneous(true);
        styles.set_column_spacing(4);
        styles.set_row_spacing(4);
        let style_btns: Vec<gtk::Button> = ["Schlicht", "Kontur", "Titel", "Untertitel", "Hinweis", "Marker"].iter().map(|n| { let b = gtk::Button::with_label(n); styles.insert(&b, -1); b }).collect();
        s_text.append(&styles);
        let th = label("Ein- und Ausblenden sowie Bewegung findest du weiter unten bei „Ein- und Ausblenden“ und „Zoom und Schwenk“.", "ws-dim ws-small");
        th.set_wrap(true);
        s_text.append(&th);

        // Farben (Formen, Pfeile, Textfeld)
        let s_colors = section(&root, "Füllung und Linie");
        let fill = color_button();
        row(&s_colors, "Füllung", &fill);
        let stroke = color_button();
        row(&s_colors, "Linie", &stroke);
        let stroke_w = spin(0.0, 200.0, 1.0, 0);
        row(&s_colors, "Linienstärke", &stroke_w);

        let s_arrow = section(&root, "Pfeil");
        let arrow_dir = gtk::DropDown::from_strings(&["nach rechts unten", "nach rechts oben", "nach links oben", "nach links unten"]);
        row(&s_arrow, "Richtung", &arrow_dir);
        let arrow_head = gtk::CheckButton::with_label("Pfeilspitze");
        s_arrow.append(&arrow_head);

        let s_blur = section(&root, "Weichzeichner");
        let blur = scale(5.0, 100.0, 1.0);
        row(&s_blur, "Stärke", &blur);
        let pixelate = gtk::CheckButton::with_label("Verpixeln statt weichzeichnen");
        s_blur.append(&pixelate);
        let bh = label("Macht alles unkenntlich, was auf den Spuren darunter in diesem Bereich liegt.", "ws-dim ws-small");
        bh.set_wrap(true);
        s_blur.append(&bh);

        // Farbe und Bild
        let s_look = section(&root, "Farbe und Bild");
        let brightness = scale(-100.0, 100.0, 1.0);
        let contrast = scale(-100.0, 100.0, 1.0);
        let saturation = scale(-100.0, 100.0, 1.0);
        let temperature = scale(-100.0, 100.0, 1.0);
        let vignette = scale(0.0, 100.0, 1.0);
        for sc in [&brightness, &contrast, &saturation, &temperature] { sc.add_mark(0.0, gtk::PositionType::Bottom, None); }
        row(&s_look, "Helligkeit", &brightness);
        row(&s_look, "Kontrast", &contrast);
        row(&s_look, "Sättigung", &saturation);
        row(&s_look, "Temperatur", &temperature);
        row(&s_look, "Vignette", &vignette);
        let flip_h = gtk::CheckButton::with_label("Spiegeln");
        let flip_v = gtk::CheckButton::with_label("Auf den Kopf");
        let fl = hbox(12);
        fl.append(&flip_h);
        fl.append(&flip_v);
        s_look.append(&fl);
        let b_look_reset = gtk::Button::with_label("Farbe zurücksetzen");
        s_look.append(&b_look_reset);

        // Mauszeiger
        let s_cursor = section(&root, "Mauszeiger");
        let cur_show = gtk::CheckButton::with_label("Mauszeiger zeigen");
        s_cursor.append(&cur_show);
        let cur_scale = scale(50.0, 500.0, 5.0);
        row(&s_cursor, "Größe", &cur_scale);
        let cur_smooth = scale(0.0, 100.0, 1.0);
        row(&s_cursor, "Glätten", &cur_smooth);
        let hl = hbox(8);
        let cur_hl = gtk::CheckButton::with_label("Hervorheben");
        cur_hl.set_hexpand(true);
        let cur_hl_color = color_button();
        hl.append(&cur_hl);
        hl.append(&cur_hl_color);
        s_cursor.append(&hl);
        let cl = hbox(8);
        let cur_clicks = gtk::CheckButton::with_label("Klick-Ringe");
        cur_clicks.set_hexpand(true);
        let cur_click_color = color_button();
        cl.append(&cur_clicks);
        cl.append(&cur_click_color);
        s_cursor.append(&cl);
        let cur_monitor = gtk::DropDown::from_strings(&["Automatisch"]);
        row(&s_cursor, "Monitor", &cur_monitor);
        let cur_info = label("", "ws-dim ws-small");
        cur_info.set_wrap(true);
        s_cursor.append(&cur_info);

        // Reihenfolge: das Wichtigste je Clip-Art steht oben
        {
            let order: [&gtk::Box; 15] = [&s_head, &s_text, &s_colors, &s_arrow, &s_blur, &s_geo, &s_anim, &s_cursor, &s_shape, &s_bg, &s_crop, &s_chroma, &s_look, &s_fade, &s_audio];
            let mut prev: gtk::Widget = s_project.clone().upcast();
            for b in order { root.reorder_child_after(b, Some(&prev)); prev = b.clone().upcast(); }
        }
        let p = Rc::new(Props {
            root, ed: Rc::downgrade(ed), updating: Cell::new(false), shown_clip: Cell::new(0), cursor_cache: RefCell::new(HashMap::new()),
            s_project, s_head, s_geo, s_crop, s_shape, s_chroma, s_bg, bg_mode, b_bg, s_fade, s_anim, s_audio, s_text, s_colors, s_arrow, s_blur, s_cursor,
            title, name, time_info, x, y, w, h, opacity, crop, shape, radius, border, border_color, shadow, border_row, b_circle: b_circle.clone(),
            chroma, chroma_color, chroma_tol, fade_in, fade_out, fade_in_kind, fade_out_kind, anim_list, volume, muted, volume_label,
            a_denoise, a_lowcut, a_compress, a_normalize, a_voice, a_gate, a_bass, a_treble, a_status,
            text, font, font_size, bold, italic, align, text_color, outline, outline_color, sel_color, gfont, b_gfont, fill, stroke, stroke_w, arrow_dir, arrow_head, blur, pixelate,
            s_look, brightness, contrast, saturation, temperature, vignette, flip_h, flip_v,
            cur_show, cur_scale, cur_smooth, cur_hl, cur_hl_color, cur_clicks, cur_click_color, cur_monitor, cur_info,
            proj_size, proj_w, proj_h, proj_fps,
        });

        // ---- Verknüpfungen
        macro_rules! bind {
            ($w:expr, $sig:ident, $key:expr, |$wv:ident, $c:ident, $st:ident| $body:expr) => {{
                let weak = Rc::downgrade(&p);
                $w.$sig(move |$wv| {
                    if let Some(pp) = weak.upgrade() { pp.apply($key, |$c: &mut Clip, $st: f64| { let _ = $st; $body }); }
                });
            }};
        }
        bind!(p.name, connect_changed, "name", |w, c, st| c.name = w.text().to_string());
        bind!(p.x, connect_value_changed, "x", |w, c, st| { let r = canvas::target_rect(c, st); canvas::set_target_rect(c, st, (w.value(), r.1, r.2, r.3)); });
        bind!(p.y, connect_value_changed, "y", |w, c, st| { let r = canvas::target_rect(c, st); canvas::set_target_rect(c, st, (r.0, w.value(), r.2, r.3)); });
        bind!(p.w, connect_value_changed, "w", |w, c, st| {
            let r = canvas::target_rect(c, st);
            let nw = w.value().max(1.0);
            // Medien behalten ihr Seitenverhältnis
            let nh = if matches!(c.kind, Kind::Video | Kind::Image) && r.2 > 0.0 { (r.3 * nw / r.2).round().max(1.0) } else { r.3 };
            canvas::set_target_rect(c, st, (r.0, r.1, nw, nh));
        });
        bind!(p.h, connect_value_changed, "h", |w, c, st| {
            let r = canvas::target_rect(c, st);
            let nh = w.value().max(1.0);
            let nw = if matches!(c.kind, Kind::Video | Kind::Image) && r.3 > 0.0 { (r.2 * nh / r.3).round().max(1.0) } else { r.2 };
            canvas::set_target_rect(c, st, (r.0, r.1, nw, nh));
        });
        bind!(p.opacity, connect_value_changed, "opacity", |w, c, st| c.opacity = w.value() / 100.0);
        for i in 0..4 {
            let weak = Rc::downgrade(&p);
            p.crop[i].connect_value_changed(move |w| {
                if let Some(pp) = weak.upgrade() {
                    pp.apply("crop", |c: &mut Clip, st: f64| {
                        // das Bild bleibt stehen, nur der sichtbare Ausschnitt ändert sich
                        let r = canvas::target_rect(c, st);
                        let fw = r.2 / (1.0 - c.crop[0] - c.crop[2]).max(0.01);
                        let fh = r.3 / (1.0 - c.crop[1] - c.crop[3]).max(0.01);
                        let (fx, fy) = (r.0 - c.crop[0] * fw, r.1 - c.crop[1] * fh);
                        let mut v = w.value() / 100.0;
                        let other = c.crop[(i + 2) % 4];
                        if v + other > 0.97 { v = 0.97 - other; }
                        c.crop[i] = v.max(0.0);
                        let nr = (fx + c.crop[0] * fw, fy + c.crop[1] * fh, fw * (1.0 - c.crop[0] - c.crop[2]), fh * (1.0 - c.crop[1] - c.crop[3]));
                        canvas::set_target_rect(c, st, nr);
                    });
                }
            });
        }
        bind!(p.shape, connect_selected_notify, "shape", |w, c, st| c.shape = [Shape::Rect, Shape::Rounded, Shape::Ellipse][w.selected().min(2) as usize]);
        bind!(p.radius, connect_value_changed, "radius", |w, c, st| c.radius = w.value() / 100.0);
        bind!(p.border, connect_value_changed, "border", |w, c, st| c.border = w.value());
        bind!(p.border_color, connect_rgba_notify, "border_color", |w, c, st| c.border_color = super::hex(&w.rgba()));
        bind!(p.shadow, connect_toggled, "shadow", |w, c, st| c.shadow = w.is_active());
        bind!(p.chroma, connect_toggled, "chroma", |w, c, st| c.chroma = w.is_active());
        bind!(p.chroma_color, connect_rgba_notify, "chroma_color", |w, c, st| c.chroma_color = super::hex(&w.rgba()));
        bind!(p.chroma_tol, connect_value_changed, "chroma_tol", |w, c, st| c.chroma_tolerance = w.value() / 100.0);
        bind!(p.bg_mode, connect_selected_notify, "", |w, c, st| c.bg_mode = BgMode::from_index(w.selected()));
        {
            let weak = Rc::downgrade(&p);
            p.b_bg.connect_clicked(move |_| { if let Some(pp) = weak.upgrade() { pp.choose_bg(); } });
        }
        bind!(p.fade_in_kind, connect_selected_notify, "fade_in_kind", |w, c, st| {
            c.fade_in_kind = FadeKind::from_index(w.selected());
            // eine andere Art als „weich“ ohne Dauer wäre unsichtbar
            if c.fade_in_kind != FadeKind::Blend && c.fade_in <= 0.0 { c.fade_in = 0.5f64.min(c.duration / 2.0); }
        });
        bind!(p.fade_out_kind, connect_selected_notify, "fade_out_kind", |w, c, st| {
            c.fade_out_kind = FadeKind::from_index(w.selected());
            if c.fade_out_kind != FadeKind::Blend && c.fade_out <= 0.0 { c.fade_out = 0.5f64.min(c.duration / 2.0); }
        });
        bind!(p.pixelate, connect_toggled, "pixelate", |w, c, st| c.pixelate = w.is_active());
        bind!(p.brightness, connect_value_changed, "brightness", |w, c, st| c.brightness = w.value() / 100.0);
        bind!(p.contrast, connect_value_changed, "contrast", |w, c, st| c.contrast = w.value() / 100.0);
        bind!(p.saturation, connect_value_changed, "saturation", |w, c, st| c.saturation = w.value() / 100.0);
        bind!(p.temperature, connect_value_changed, "temperature", |w, c, st| c.temperature = w.value() / 100.0);
        bind!(p.vignette, connect_value_changed, "vignette", |w, c, st| c.vignette = w.value() / 100.0);
        bind!(p.flip_h, connect_toggled, "flip_h", |w, c, st| c.flip_h = w.is_active());
        bind!(p.flip_v, connect_toggled, "flip_v", |w, c, st| c.flip_v = w.is_active());
        bind!(b_look_reset, connect_clicked, "look_reset", |_w, c, st| { c.brightness = 0.0; c.contrast = 0.0; c.saturation = 0.0; c.temperature = 0.0; c.vignette = 0.0; });
        bind!(p.fade_in, connect_value_changed, "fade_in", |w, c, st| c.fade_in = w.value().min(c.duration));
        bind!(p.fade_out, connect_value_changed, "fade_out", |w, c, st| c.fade_out = w.value().min(c.duration));
        bind!(p.volume, connect_value_changed, "volume", |w, c, st| c.volume = w.value() / 100.0);
        bind!(p.muted, connect_toggled, "muted", |w, c, st| c.muted = w.is_active());
        macro_rules! abind {
            ($w:expr, $sig:ident, $key:expr, |$wv:ident, $a:ident| $body:expr) => {{
                let weak = Rc::downgrade(&p);
                $w.$sig(move |$wv| {
                    let Some(pp) = weak.upgrade() else { return };
                    if pp.updating.get() { return; }
                    if let Some(ed) = pp.ed.upgrade() { ed.audio_change($key, |$a: &mut crate::model::AudioFx| $body); }
                });
            }};
        }
        abind!(p.a_denoise, connect_value_changed, "a_denoise", |w, a| a.denoise = w.value() / 100.0);
        abind!(p.a_lowcut, connect_toggled, "", |w, a| a.lowcut = w.is_active());
        abind!(p.a_compress, connect_value_changed, "a_compress", |w, a| a.compress = w.value() / 100.0);
        abind!(p.a_normalize, connect_toggled, "", |w, a| a.normalize = w.is_active());
        abind!(p.a_voice, connect_toggled, "", |w, a| a.voice = w.is_active());
        abind!(p.a_gate, connect_value_changed, "a_gate", |w, a| a.gate = w.value() / 100.0);
        abind!(p.a_bass, connect_value_changed, "a_bass", |w, a| a.bass = w.value());
        abind!(p.a_treble, connect_value_changed, "a_treble", |w, a| a.treble = w.value());
        abind!(b_improve, connect_clicked, "", |_w, a| *a = crate::model::AudioFx::improved());
        abind!(b_areset, connect_clicked, "", |_w, a| *a = crate::model::AudioFx::default());
        bind!(p.font_size, connect_value_changed, "font_size", |w, c, st| c.font_size = w.value());
        bind!(p.bold, connect_toggled, "bold", |w, c, st| c.bold = w.is_active());
        bind!(p.italic, connect_toggled, "italic", |w, c, st| c.italic = w.is_active());
        bind!(p.align, connect_selected_notify, "align", |w, c, st| c.align = w.selected() as i32);
        bind!(p.text_color, connect_rgba_notify, "color", |w, c, st| c.color = super::hex(&w.rgba()));
        bind!(p.outline, connect_value_changed, "outline", |w, c, st| c.outline = w.value());
        bind!(p.outline_color, connect_rgba_notify, "outline_color", |w, c, st| c.outline_color = super::hex(&w.rgba()));
        bind!(p.fill, connect_rgba_notify, "fill", |w, c, st| c.fill = super::hex(&w.rgba()));
        bind!(p.stroke, connect_rgba_notify, "stroke", |w, c, st| c.stroke = super::hex(&w.rgba()));
        bind!(p.stroke_w, connect_value_changed, "stroke_w", |w, c, st| c.stroke_w = w.value());
        bind!(p.arrow_dir, connect_selected_notify, "arrow_dir", |w, c, st| c.arrow_dir = w.selected() as i32);
        bind!(p.arrow_head, connect_toggled, "arrow_head", |w, c, st| c.arrow_head = w.is_active());
        bind!(p.blur, connect_value_changed, "blur", |w, c, st| c.blur = w.value() / 100.0);
        bind!(p.font, connect_font_desc_notify, "font", |w, c, st| { if let Some(f) = w.font_desc().and_then(|d| d.family()) { c.font = f.to_string(); } });
        bind!(p.cur_show, connect_toggled, "cur_show", |w, c, st| { if let Some(k) = c.cursor.as_mut() { k.show = w.is_active(); } });
        bind!(p.cur_scale, connect_value_changed, "cur_scale", |w, c, st| { if let Some(k) = c.cursor.as_mut() { k.scale = w.value() / 100.0; } });
        bind!(p.cur_smooth, connect_value_changed, "cur_smooth", |w, c, st| { if let Some(k) = c.cursor.as_mut() { k.smooth = w.value() / 100.0; } });
        bind!(p.cur_hl, connect_toggled, "cur_hl", |w, c, st| { if let Some(k) = c.cursor.as_mut() { k.highlight = w.is_active(); } });
        bind!(p.cur_hl_color, connect_rgba_notify, "cur_hl_color", |w, c, st| { if let Some(k) = c.cursor.as_mut() { k.highlight_color = super::hex(&w.rgba()); } });
        bind!(p.cur_clicks, connect_toggled, "cur_clicks", |w, c, st| { if let Some(k) = c.cursor.as_mut() { k.clicks = w.is_active(); } });
        bind!(p.cur_click_color, connect_rgba_notify, "cur_click_color", |w, c, st| { if let Some(k) = c.cursor.as_mut() { k.click_color = super::hex(&w.rgba()); } });
        bind!(p.cur_monitor, connect_selected_notify, "cur_monitor", |w, c, st| { if let Some(k) = c.cursor.as_mut() { k.monitor = w.selected() as i32 - 1; } });
        {
            let weak = Rc::downgrade(&p);
            p.text.buffer().connect_changed(move |b| {
                if let Some(pp) = weak.upgrade() {
                    let t = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
                    // die Farbmarken wandern beim Tippen mit dem Text mit
                    let cols = collect_colors(b);
                    pp.apply("text", move |c: &mut Clip, _| { c.text = t.clone(); c.text_colors = cols.clone(); });
                }
            });
        }
        for (btn, set) in [(&b_sel_color, true), (&b_sel_clear, false)] {
            let weak = Rc::downgrade(&p);
            btn.connect_clicked(move |_| {
                let Some(pp) = weak.upgrade() else { return };
                let b = pp.text.buffer();
                let (s, e) = match b.selection_bounds() {
                    Some(r) => r,
                    None if !set => (b.start_iter(), b.end_iter()),
                    None => {
                        if let Some(ed) = pp.ed.upgrade() { ed.show_note("Markiere zuerst im Textfeld die Buchstaben, die du färben möchtest."); }
                        return;
                    }
                };
                clear_color_tags(&b, &s, &e);
                if set { b.apply_tag(&color_tag(&b, &super::hex(&pp.sel_color.rgba())), &s, &e); }
                let cols = collect_colors(&b);
                pp.apply("text_colors", move |c: &mut Clip, _| c.text_colors = cols.clone());
            });
        }
        {
            let weak = Rc::downgrade(&p);
            let start = move || {
                let Some(pp) = weak.upgrade() else { return };
                let name = pp.gfont.text().trim().to_string();
                if name.is_empty() { return; }
                pp.b_gfont.set_sensitive(false);
                pp.b_gfont.set_label("Lädt …");
                let (tx, rx) = std::sync::mpsc::channel();
                let n2 = name.clone();
                std::thread::spawn(move || { let _ = tx.send(crate::fonts::install(&n2)); });
                let weak = Rc::downgrade(&pp);
                gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                    let Ok(res) = rx.try_recv() else { return gtk::glib::ControlFlow::Continue };
                    let Some(pp) = weak.upgrade() else { return gtk::glib::ControlFlow::Break };
                    pp.b_gfont.set_sensitive(true);
                    pp.b_gfont.set_label("Laden");
                    let ed = pp.ed.upgrade();
                    match res {
                        Ok(n) => {
                            crate::fonts::reload();
                            crate::annot::clear_cache();
                            let fam = name.clone();
                            pp.apply("font", move |c: &mut Clip, _| c.font = fam.clone());
                            if let Some(ed) = &ed { ed.show_note(&format!("Schrift „{name}“ ist installiert ({n} Dateien) und für diesen Text gewählt. Du findest sie ab jetzt auch unter „Schrift“.")); pp.refresh(ed); }
                        }
                        Err(e) => { if let Some(ed) = &ed { ed.show_note(&format!("Schrift konnte nicht geladen werden: {e}")); } }
                    }
                    gtk::glib::ControlFlow::Break
                });
            };
            let s2 = start.clone();
            p.b_gfont.connect_clicked(move |_| s2());
            p.gfont.connect_activate(move |_| start());
        }
        macro_rules! click {
            ($b:expr, |$pp:ident, $e:ident| $body:expr) => {{
                let weak = Rc::downgrade(&p);
                $b.connect_clicked(move |_| {
                    if let Some($pp) = weak.upgrade() { if let Some($e) = $pp.ed.upgrade() { $body } }
                });
            }};
        }
        for (i, b) in style_btns.iter().enumerate() {
            click!(b, |pp, _e| pp.apply("", move |c: &mut Clip, _st: f64| {
                if c.kind != Kind::Text { return; }
                let k = (c.font_size / 72.0).max(0.3);
                // Grundzustand, dann der gewählte Stil
                c.fill = "#00000000".into();
                c.stroke_w = 0.0;
                c.outline = 0.0;
                c.shadow = true;
                c.color = "#ffffff".into();
                c.shape = Shape::Rounded;
                c.radius = 0.3;
                match i {
                    1 => { c.outline = (6.0 * k).round().max(2.0); c.outline_color = "#000000".into(); c.shadow = false; }
                    2 => { c.fill = "#141418e0".into(); c.bold = true; }
                    3 => { c.fill = "#000000b0".into(); c.shape = Shape::Rect; c.shadow = false; c.bold = false; }
                    4 => { c.fill = "#ff8a3d".into(); c.color = "#17181c".into(); c.bold = true; }
                    5 => { c.fill = "#ffe44d".into(); c.color = "#17181c".into(); c.shape = Shape::Rect; c.shadow = false; }
                    _ => {}
                }
            }));
        }
        click!(b_crop_reset, |pp, _e| pp.apply("", |c: &mut Clip, st: f64| {
            let r = canvas::target_rect(c, st);
            let fw = r.2 / (1.0 - c.crop[0] - c.crop[2]).max(0.01);
            let fh = r.3 / (1.0 - c.crop[1] - c.crop[3]).max(0.01);
            let nr = (r.0 - c.crop[0] * fw, r.1 - c.crop[1] * fh, fw, fh);
            c.crop = [0.0; 4];
            canvas::set_target_rect(c, st, nr);
        }));
        click!(b_circle, |pp, _e| pp.apply("", |c: &mut Clip, st: f64| {
            let r = canvas::target_rect(c, st);
            let side = r.2.min(r.3);
            let (sw, sh) = (c.src_w.max(1) as f64, c.src_h.max(1) as f64);
            c.crop = if sw > sh { let k = (1.0 - sh / sw) / 2.0; [k, 0.0, k, 0.0] } else { let k = (1.0 - sw / sh) / 2.0; [0.0, k, 0.0, k] };
            c.shape = Shape::Ellipse;
            canvas::set_target_rect(c, st, ((r.0 + (r.2 - side) / 2.0).round(), (r.1 + (r.3 - side) / 2.0).round(), side.round(), side.round()));
        }));
        click!(b_fit, |pp, e| { let (pw, ph) = { let s = e.st.borrow(); (s.project.width as f64, s.project.height as f64) }; pp.apply("", move |c: &mut Clip, st: f64| {
            let r = canvas::target_rect(c, st);
            let a = if r.3 > 0.0 { r.2 / r.3 } else { 1.0 };
            let (w, h) = if pw / ph > a { (ph * a, ph) } else { (pw, pw / a) };
            canvas::set_target_rect(c, st, (((pw - w) / 2.0).round(), ((ph - h) / 2.0).round(), w.round(), h.round()));
        }); });
        click!(b_fill, |pp, e| { let (pw, ph) = { let s = e.st.borrow(); (s.project.width as f64, s.project.height as f64) }; pp.apply("", move |c: &mut Clip, st: f64| {
            let r = canvas::target_rect(c, st);
            let a = if r.3 > 0.0 { r.2 / r.3 } else { 1.0 };
            let (w, h) = if pw / ph > a { (pw, pw / a) } else { (ph * a, ph) };
            canvas::set_target_rect(c, st, (((pw - w) / 2.0).round(), ((ph - h) / 2.0).round(), w.round(), h.round()));
        }); });
        click!(b_center, |pp, e| { let (pw, ph) = { let s = e.st.borrow(); (s.project.width as f64, s.project.height as f64) }; pp.apply("", move |c: &mut Clip, st: f64| {
            let r = canvas::target_rect(c, st);
            canvas::set_target_rect(c, st, (((pw - r.2) / 2.0).round(), ((ph - r.3) / 2.0).round(), r.2, r.3));
        }); });
        click!(b_zoom, |pp, e| pp.add_anim(&e, 0));
        click!(b_reset, |pp, e| pp.add_anim(&e, 1));
        click!(b_custom, |pp, e| pp.add_anim(&e, 2));
        click!(b_saveas, |_pp, e| e.save_as_dialog());
        // Projektgröße
        {
            let weak = Rc::downgrade(&p);
            p.proj_size.connect_selected_notify(move |d| {
                let Some(pp) = weak.upgrade() else { return };
                if pp.updating.get() { return; }
                if let Some((w, h, _)) = SIZES.get(d.selected() as usize) { pp.set_project_size(*w, *h); }
            });
            for s in [&p.proj_w, &p.proj_h] {
                let weak = Rc::downgrade(&p);
                s.connect_value_changed(move |_| {
                    let Some(pp) = weak.upgrade() else { return };
                    if pp.updating.get() { return; }
                    let (w, h) = (pp.proj_w.value() as u32 / 2 * 2, pp.proj_h.value() as u32 / 2 * 2);
                    pp.set_project_size(w, h);
                });
            }
            let weak = Rc::downgrade(&p);
            p.proj_fps.connect_selected_notify(move |d| {
                let Some(pp) = weak.upgrade() else { return };
                if pp.updating.get() { return; }
                let Some(e) = pp.ed.upgrade() else { return };
                let f = FPS[d.selected().min(3) as usize];
                e.edit(|p| p.fps = f);
            });
        }
        p
    }

    fn set_project_size(&self, w: u32, h: u32) {
        let Some(e) = self.ed.upgrade() else { return };
        let same = { let s = e.st.borrow(); s.project.width == w && s.project.height == h };
        if same || w < 16 || h < 16 { return; }
        e.edit_coalesced("project_size", |p| ops::resize_project(p, w, h));
    }

    /// Änderung auf alle ausgewählten Clips anwenden.
    fn apply(&self, key: &str, f: impl Fn(&mut Clip, f64)) {
        if self.updating.get() { return; }
        let Some(ed) = self.ed.upgrade() else { return };
        let (ids, ph) = { let st = ed.st.borrow(); (st.sel.clone(), st.playhead) };
        if ids.is_empty() { return; }
        let change = |p: &mut Project| {
            for id in &ids {
                if let Some(c) = p.clip_mut(*id) {
                    let st = c.source_time(ph);
                    f(c, st);
                }
            }
        };
        if key.is_empty() { ed.edit(change); } else { ed.edit_coalesced(key, change); }
    }

    /// Hintergrundbild für die ausgewählten Kamera-Clips wählen.
    pub fn choose_bg(self: &Rc<Self>) {
        let Some(ed) = self.ed.upgrade() else { return };
        let Some(win) = ed.window() else { return };
        let filter = gtk::FileFilter::new();
        filter.add_pixbuf_formats();
        filter.set_name(Some("Bilder"));
        let d = gtk::FileDialog::builder().title("Hintergrundbild wählen").default_filter(&filter).build();
        let weak = Rc::downgrade(self);
        d.open(Some(&win), gtk::gio::Cancellable::NONE, move |res| {
            let (Ok(f), Some(pp)) = (res, weak.upgrade()) else { return };
            let Some(path) = f.path() else { return };
            let path = path.to_string_lossy().to_string();
            // auch das Hintergrundbild gehört in den Projektordner
            let path = pp.ed.upgrade().map(|e| e.adopt(&path)).unwrap_or(path);
            pp.apply("", move |c: &mut Clip, _| { if c.kind.is_media() && c.is_visual() { c.bg_image = path.clone(); c.bg_mode = BgMode::Image; } });
        });
    }

    pub fn focus_text(&self) {
        self.text.grab_focus();
        let b = self.text.buffer();
        b.select_range(&b.start_iter(), &b.end_iter());
    }

    fn cursor_data(&self, path: &str) -> Option<Rc<CursorData>> {
        if let Some(d) = self.cursor_cache.borrow().get(path) { return Some(d.clone()); }
        let d = Rc::new(CursorData::load(path)?);
        self.cursor_cache.borrow_mut().insert(path.to_string(), d.clone());
        Some(d)
    }

    /// Animation am Abspielkopf einfügen: 0 = hineinzoomen, 1 = zurück zur Ausgangsgröße, 2 = eigene.
    pub fn add_anim(&self, ed: &Rc<Editor>, kind: u32) {
        let Some(c) = ed.selected_clip() else { return };
        let (ph, pw, pht) = { let s = ed.st.borrow(); (s.playhead, s.project.width as f64, s.project.height as f64) };
        if ph < c.start || ph > c.end() { ed.show_note("Der Abspielkopf muss auf dem ausgewählten Clip stehen."); return; }
        let st = c.source_time(ph);
        let cur = c.rect_at_source(st);
        let dur = 0.6;
        let target = match kind {
            0 => {
                // Mittelpunkt des Zooms: Mauszeiger (falls Daten vorhanden), sonst Bildmitte
                let mut focus = (pw / 2.0, pht / 2.0);
                if let Some(k) = &c.cursor {
                    if let Some(d) = self.cursor_data(&k.data) {
                        if let (Some(m), Some((gx, gy))) = (d.pick_monitor(k.monitor, c.src_w, c.src_h), d.pos_at(st + dur)) {
                            let (rx, ry) = ((gx - m.x) / m.w, (gy - m.y) / m.h);
                            if (0.0..=1.0).contains(&rx) && (0.0..=1.0).contains(&ry) {
                                let fw = cur.2 / (1.0 - c.crop[0] - c.crop[2]).max(0.01);
                                let fh = cur.3 / (1.0 - c.crop[1] - c.crop[3]).max(0.01);
                                focus = (cur.0 - c.crop[0] * fw + rx * fw, cur.1 - c.crop[1] * fh + ry * fh);
                            }
                        }
                    }
                }
                let k = 1.8;
                let (nw, nh) = (cur.2 * k, cur.3 * k);
                // der Punkt unter dem Fokus wandert in die Bildmitte, ohne dass Ränder frei werden
                let mut nx = pw / 2.0 - (focus.0 - cur.0) * k;
                let mut ny = pht / 2.0 - (focus.1 - cur.1) * k;
                if nw >= pw { nx = nx.clamp(pw - nw, 0.0); }
                if nh >= pht { ny = ny.clamp(pht - nh, 0.0); }
                (nx.round(), ny.round(), nw.round(), nh.round())
            }
            1 => (c.x, c.y, c.w, c.h),
            _ => cur,
        };
        let id = c.id;
        ed.edit(|p| {
            if let Some(k) = p.clip_mut(id) {
                k.anims.push(Anim { t: st, dur, x: target.0, y: target.1, w: target.2, h: target.3 });
                k.anims.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
            }
        });
        // ans Ende der Bewegung springen: dort sieht und bearbeitet man das Ziel
        ed.seek((ph + dur).min(c.end() - 0.001));
        if kind == 2 { ed.show_note("Jetzt den Clip in der Vorschau verschieben oder skalieren: das ist das Ziel der Bewegung."); }
    }

    /// Rollt die rechte Leiste so, dass „Anfang und Ende“ bzw. die Liste der Bewegungen zu sehen ist.
    pub fn reveal(&self, anim: bool) {
        let sec = if anim { &self.s_anim } else { &self.s_fade };
        if !sec.is_visible() { return; }
        let Some(sw) = self.root.ancestor(gtk::ScrolledWindow::static_type()).and_then(|w| w.downcast::<gtk::ScrolledWindow>().ok()) else { return };
        let Some(pt) = sec.compute_point(&self.root, &gtk::graphene::Point::new(0.0, 0.0)) else { return };
        let adj = sw.vadjustment();
        let (top, h) = (pt.y() as f64, sec.height() as f64);
        let (v, page) = (adj.value(), adj.page_size());
        if top < v || top + h > v + page { adj.set_value((top - 8.0).min(adj.upper() - page).max(0.0)); }
    }

    pub fn refresh(self: &Rc<Self>, ed: &Editor) {
        self.updating.set(true);
        let st = ed.st.borrow();
        let clip = st.sel.first().and_then(|id| st.project.clip(*id)).cloned();
        let multi = st.sel.len() > 1;
        let ph = st.playhead;
        let (pw, pht, fps) = (st.project.width, st.project.height, st.project.fps);
        drop(st);
        let vis = |b: &gtk::Box, v: bool| b.set_visible(v);
        match clip {
            None => {
                for b in [&self.s_head, &self.s_geo, &self.s_crop, &self.s_shape, &self.s_chroma, &self.s_bg, &self.s_fade, &self.s_anim, &self.s_audio, &self.s_text, &self.s_colors, &self.s_arrow, &self.s_blur, &self.s_cursor, &self.s_look] { vis(b, false); }
                vis(&self.s_project, true);
                self.shown_clip.set(0);
                let idx = SIZES.iter().position(|s| s.0 == pw && s.1 == pht).unwrap_or(SIZES.len());
                self.proj_size.set_selected(idx as u32);
                self.proj_w.set_value(pw as f64);
                self.proj_h.set_value(pht as f64);
                self.proj_fps.set_selected(FPS.iter().position(|f| *f == fps).unwrap_or(0) as u32);
            }
            Some(c) => {
                vis(&self.s_project, false);
                vis(&self.s_head, true);
                let media = c.kind.is_media();
                let visual = c.is_visual();
                vis(&self.s_geo, visual);
                vis(&self.s_crop, media && visual);
                vis(&self.s_shape, (media && visual) || matches!(c.kind, Kind::Shape | Kind::Text));
                vis(&self.s_chroma, media && visual);
                vis(&self.s_bg, media && visual);
                self.bg_mode.set_selected(c.bg_mode.index());
                self.b_bg.set_label(&if c.bg_image.is_empty() { "Bild wählen …".to_string() } else { format!("Bild: {}", std::path::Path::new(&c.bg_image).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()) });
                vis(&self.s_fade, c.kind != Kind::Blur);
                vis(&self.s_anim, visual && c.kind != Kind::Blur);
                vis(&self.s_audio, c.carries_audio());
                vis(&self.s_text, c.kind == Kind::Text);
                vis(&self.s_colors, matches!(c.kind, Kind::Shape | Kind::Text | Kind::Arrow));
                vis(&self.s_arrow, c.kind == Kind::Arrow);
                vis(&self.s_blur, c.kind == Kind::Blur);
                vis(&self.s_look, visual && c.kind != Kind::Blur);
                self.pixelate.set_active(c.pixelate);
                self.brightness.set_value(c.brightness * 100.0);
                self.contrast.set_value(c.contrast * 100.0);
                self.saturation.set_value(c.saturation * 100.0);
                self.temperature.set_value(c.temperature * 100.0);
                self.vignette.set_value(c.vignette * 100.0);
                self.flip_h.set_active(c.flip_h);
                self.flip_v.set_active(c.flip_v);
                vis(&self.s_cursor, c.cursor.is_some());
                self.title.set_text(&if multi { "Mehrere Clips".to_string() } else { c.kind.label().to_string() });
                // Felder, in denen gerade getippt wird, nicht überschreiben (sonst springt die Schreibmarke)
                let same_clip = self.shown_clip.replace(c.id) == c.id;
                let typing_name = same_clip && self.name.has_focus();
                if !typing_name && self.name.text() != c.name { self.name.set_text(&c.name); }
                self.time_info.set_text(&format!("Beginn {} · Dauer {}", format_time(c.start, fps), format_time(c.duration, fps)));
                let stt = c.source_time(ph.clamp(c.start, c.end()));
                let r = canvas::target_rect(&c, stt);
                self.x.set_value(r.0.round());
                self.y.set_value(r.1.round());
                self.w.set_value(r.2.round());
                self.h.set_value(r.3.round());
                self.opacity.set_value(c.opacity * 100.0);
                for i in 0..4 { self.crop[i].set_value(c.crop[i] * 100.0); }
                self.shape.set_selected(match c.shape { Shape::Rect => 0, Shape::Rounded => 1, Shape::Ellipse => 2 });
                self.radius.set_value(c.radius * 100.0);
                self.radius.set_sensitive(c.shape == Shape::Rounded);
                self.border.set_value(c.border);
                self.border_color.set_rgba(&super::rgba(&c.border_color));
                self.border_row.set_visible(media);
                self.b_circle.set_visible(media);
                self.shadow.set_active(c.shadow);
                self.chroma.set_active(c.chroma);
                self.chroma_color.set_rgba(&super::rgba(&c.chroma_color));
                self.chroma_tol.set_value(c.chroma_tolerance * 100.0);
                self.fade_in_kind.set_selected(c.fade_in_kind.index());
                self.fade_out_kind.set_selected(c.fade_out_kind.index());
                self.fade_in.set_value(c.fade_in);
                self.fade_out.set_value(c.fade_out);
                self.volume.set_value(c.volume * 100.0);
                self.volume_label.set_text(&format!("{:.0} %", c.volume * 100.0));
                self.muted.set_active(c.muted);
                self.a_denoise.set_value(c.audio.denoise * 100.0);
                self.a_lowcut.set_active(c.audio.lowcut);
                self.a_compress.set_value(c.audio.compress * 100.0);
                self.a_normalize.set_active(c.audio.normalize);
                self.a_voice.set_active(c.audio.voice);
                self.a_gate.set_value(c.audio.gate * 100.0);
                self.a_bass.set_value(c.audio.bass);
                self.a_treble.set_value(c.audio.treble);
                self.a_status.set_text(&ed.audio_status(&c));
                let b = self.text.buffer();
                let typing_text = same_clip && self.text.has_focus();
                if !typing_text && b.text(&b.start_iter(), &b.end_iter(), false) != c.text { b.set_text(&c.text); }
                if !typing_text && collect_colors(&b) != c.text_colors {
                    clear_color_tags(&b, &b.start_iter(), &b.end_iter());
                    for t in &c.text_colors {
                        b.apply_tag(&color_tag(&b, &t.color), &b.iter_at_offset(t.start as i32), &b.iter_at_offset(t.end as i32));
                    }
                }
                self.font.set_font_desc(&gtk::pango::FontDescription::from_string(&c.font));
                self.font_size.set_value(c.font_size);
                self.bold.set_active(c.bold);
                self.italic.set_active(c.italic);
                self.align.set_selected(c.align.clamp(0, 2) as u32);
                self.text_color.set_rgba(&super::rgba(&c.color));
                self.outline.set_value(c.outline);
                self.outline_color.set_rgba(&super::rgba(&c.outline_color));
                self.fill.set_rgba(&super::rgba(&c.fill));
                self.stroke.set_rgba(&super::rgba(&c.stroke));
                self.stroke_w.set_value(c.stroke_w);
                self.arrow_dir.set_selected(c.arrow_dir.clamp(0, 3) as u32);
                self.arrow_head.set_active(c.arrow_head);
                self.blur.set_value(c.blur * 100.0);
                if let Some(k) = &c.cursor {
                    self.cur_show.set_active(k.show);
                    self.cur_scale.set_value(k.scale * 100.0);
                    self.cur_smooth.set_value(k.smooth * 100.0);
                    self.cur_hl.set_active(k.highlight);
                    self.cur_hl_color.set_rgba(&super::rgba(&k.highlight_color));
                    self.cur_clicks.set_active(k.clicks);
                    self.cur_click_color.set_rgba(&super::rgba(&k.click_color));
                    let data = self.cursor_data(&k.data);
                    let mut names = vec!["Automatisch".to_string()];
                    let mut info = String::new();
                    if let Some(d) = &data {
                        for m in &d.monitors { names.push(format!("{} ({} × {})", m.name, m.pw, m.ph)); }
                        if d.clicks.is_empty() { info = "Diese Aufnahme enthält keine Klickdaten (siehe LIESMICH: Gruppe „input“).".into(); }
                        if d.pick_monitor(k.monitor, c.src_w, c.src_h).is_none() { info = "Der aufgenommene Monitor ist nicht eindeutig. Bitte oben den richtigen Monitor wählen.".into(); }
                    } else {
                        info = "Die Zeigerdaten dieser Aufnahme fehlen.".into();
                    }
                    let model_len = self.cur_monitor.model().map(|m| m.n_items()).unwrap_or(0);
                    if model_len as usize != names.len() {
                        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                        self.cur_monitor.set_model(Some(&gtk::StringList::new(&refs)));
                    }
                    self.cur_monitor.set_selected((k.monitor + 1).max(0) as u32);
                    self.cur_info.set_text(&info);
                    self.cur_info.set_visible(!info.is_empty());
                }
                // Liste der Animationen
                while let Some(ch) = self.anim_list.first_child() { self.anim_list.remove(&ch); }
                for (i, a) in c.anims.iter().enumerate() {
                    let rowb = hbox(6);
                    let t = label(&format!("bei {}", format_time((c.start + a.t - c.inpoint).max(0.0), fps)), "ws-small");
                    t.set_hexpand(true);
                    rowb.append(&t);
                    let d = spin(0.0, 20.0, 0.1, 1);
                    d.set_value(a.dur);
                    d.set_tooltip_text(Some("Dauer der Bewegung in Sekunden"));
                    rowb.append(&d);
                    let del = super::icon_button("delete", "", "Animation entfernen");
                    del.add_css_class("ws-flat");
                    rowb.append(&del);
                    let go = super::icon_button("next", "", "Zum Ende dieser Bewegung springen");
                    go.add_css_class("ws-flat");
                    rowb.append(&go);
                    let (weak, id) = (Rc::downgrade(self), c.id);
                    d.connect_value_changed(move |s| {
                        if let Some(pp) = weak.upgrade() { if let Some(e) = pp.ed.upgrade() {
                            let v = s.value();
                            e.edit_coalesced("anim_dur", |p| { if let Some(k) = p.clip_mut(id) { if let Some(an) = k.anims.get_mut(i) { an.dur = v; } } });
                        } }
                    });
                    let weak = Rc::downgrade(self);
                    del.connect_clicked(move |_| {
                        if let Some(pp) = weak.upgrade() { if let Some(e) = pp.ed.upgrade() {
                            e.edit(|p| { if let Some(k) = p.clip_mut(id) { if i < k.anims.len() { k.anims.remove(i); } } });
                        } }
                    });
                    let weak = Rc::downgrade(self);
                    let end_t = c.start + a.t - c.inpoint + a.dur;
                    go.connect_clicked(move |_| { if let Some(pp) = weak.upgrade() { if let Some(e) = pp.ed.upgrade() { e.seek(end_t); } } });
                    self.anim_list.append(&rowb);
                }
            }
        }
        self.updating.set(false);
        if std::env::var_os("WS_DEBUG").is_some() {
            let (min, _, _, _) = self.root.measure(gtk::Orientation::Horizontal, -1);
            // (die Ränder der Leiste sind im gemessenen Wert schon enthalten)
            if min > PANEL_W { eprintln!("ws-video: rechte Leiste zu schmal ({}): Inhalt braucht {min} px, Platz {PANEL_W} px", self.title.text()); }
        }
    }
}

/// Marke im Textfeld für eine Buchstabenfarbe (Name "c#rrggbb")
fn color_tag(b: &gtk::TextBuffer, color: &str) -> gtk::TextTag {
    let name = format!("c{color}");
    if let Some(t) = b.tag_table().lookup(&name) { return t; }
    let t = gtk::TextTag::builder().name(&name).foreground(color).build();
    b.tag_table().add(&t);
    t
}

fn clear_color_tags(b: &gtk::TextBuffer, s: &gtk::TextIter, e: &gtk::TextIter) {
    let mut tags = Vec::new();
    b.tag_table().foreach(|t| if t.name().map(|n| n.starts_with("c#")).unwrap_or(false) { tags.push(t.clone()); });
    for t in tags { b.remove_tag(&t, s, e); }
}

/// Buchstabenfarben aus den Marken des Textfelds lesen
fn collect_colors(b: &gtk::TextBuffer) -> Vec<crate::model::TextColor> {
    let mut out: Vec<crate::model::TextColor> = Vec::new();
    let mut it = b.start_iter();
    let end = b.end_iter();
    while it < end {
        let col = it.tags().iter().filter_map(|t| t.name()).find(|n| n.starts_with("c#")).map(|n| n[1..].to_string());
        let o = it.offset() as u32;
        if let Some(col) = col {
            match out.last_mut() {
                Some(l) if l.end == o && l.color == col => l.end = o + 1,
                _ => out.push(crate::model::TextColor { start: o, end: o + 1, color: col }),
            }
        }
        if !it.forward_char() { break; }
    }
    out
}
