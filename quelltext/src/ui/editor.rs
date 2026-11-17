//! Der Editor: Zustand, Aufbau der Oberfläche und Bearbeitungsbefehle.
use super::{canvas, export, hbox, icons, label, props, timeline, vbox, App};
use crate::engine::{self, Engine};
use crate::model::*;
use crate::ops;
use gtk::prelude::*;
use gtk::{cairo, gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool { Select, Hand, Crop }

pub struct State {
    pub project: Project,
    pub path: PathBuf,
    pub dirty: bool,
    pub sel: Vec<u64>,
    pub playhead: f64,
    pub undo: Vec<Project>,
    pub redo: Vec<Project>,
    pub tool: Tool,
    /// Vorschau-Zoom (0 = einpassen) und Verschiebung
    pub view_zoom: f64,
    pub view_pan: (f64, f64),
    /// Zeitleiste: Pixel je Sekunde, Anfang in Sekunden, senkrechter Versatz in Pixeln
    pub tl_pps: f64,
    pub tl_scroll: f64,
    pub tl_vscroll: f64,
    pub range: Option<(f64, f64)>,
    /// Spuren, für die der markierte Bereich gilt (leer = alle)
    pub range_tracks: Vec<u64>,
    /// Schere: Werkzeug aktiv, Schnitt durch alle Ebenen, Stelle unter der Maus (Zeit, Spur, alle)
    pub tl_cut: bool,
    pub cut_all: bool,
    pub cut_hover: Option<(f64, u64, bool)>,
    pub clipboard: Vec<Clip>,
    pub canvas_drag: Option<canvas::Drag>,
    pub tl_drag: Option<timeline::Drag>,
    pub guides: Vec<(bool, f64)>,
}

enum BgResult {
    Thumb(String, Option<engine::Frame>),
    Wave(String, Option<Vec<f32>>),
    Audio(String, Option<crate::audiofx::Profile>),
}

pub struct Editor {
    pub app: Weak<App>,
    pub root: gtk::Box,
    pub st: RefCell<State>,
    pub engine: Rc<RefCell<Engine>>,
    pub canvas: gtk::DrawingArea,
    pub timeline: gtk::DrawingArea,
    pub tl_scrollbar: gtk::Scrollbar,
    pub time_label: gtk::Label,
    play_icon: Rc<Cell<&'static str>>,
    play_btn: gtk::Button,
    pub props: RefCell<Option<Rc<props::Props>>>,
    media_box: gtk::Box,
    note: gtk::Label,
    note_rev: gtk::Revealer,
    tool_btns: RefCell<Vec<gtk::ToggleButton>>,
    cut_btns: RefCell<Vec<gtk::ToggleButton>>,
    pub frame_cache: RefCell<(u64, Option<cairo::ImageSurface>)>,
    pub thumbs: RefCell<HashMap<String, Option<cairo::ImageSurface>>>,
    pub waves: RefCell<HashMap<String, Rc<Vec<f32>>>>,
    bg: Arc<Mutex<Vec<BgResult>>>,
    bg_pending: RefCell<HashSet<String>>,
    last_edit: RefCell<(String, std::time::Instant)>,
    last_serial: Cell<u64>,
    was_playing: Cell<bool>,
    /// zuletzt gezeichnete Stellung der Abspielmarke (Bildpunkt, Verschiebung) und zuletzt gezeigte Vorschau-Bildrate
    last_mark: Cell<(i64, i64)>,
    last_hint: Cell<u32>,
    fps_hint: gtk::Label,
    last_autosave: Cell<std::time::Instant>,
    media_sig: RefCell<String>,
    updating_scroll: Cell<bool>,
}

impl Editor {
    pub fn new(app: &Rc<App>, project: Project, path: PathBuf) -> anyhow::Result<Rc<Editor>> {
        let engine = Engine::new(&project)?;
        let root = vbox(0);
        root.add_css_class("ws-root");

        // ---- Werkzeugleiste oben
        let bar = hbox(6);
        bar.add_css_class("ws-toolbar");
        let b_home = super::icon_button("home", "", "Zur Startseite");
        b_home.add_css_class("ws-flat");
        let b_rec = super::icon_button("record", "Aufnahme", "Neue Aufnahme starten");
        let b_import = super::icon_button("import", "Importieren", "Videos, Bilder oder Ton importieren");
        let b_save = super::icon_button("save", "", "Projekt speichern (Strg+S)");
        b_save.add_css_class("ws-flat");
        let b_undo = super::icon_button("undo", "", "Rückgängig (Strg+Z)");
        b_undo.add_css_class("ws-flat");
        let b_redo = super::icon_button("redo", "", "Wiederholen (Strg+Umschalt+Z)");
        b_redo.add_css_class("ws-flat");
        let t_select = super::icon_toggle("select", "", "Auswählen und verschieben (V)");
        let t_hand = super::icon_toggle("hand", "", "Ansicht verschieben (H)");
        let t_crop = super::icon_toggle("crop", "", "Zuschneiden (C)");
        for t in [&t_select, &t_hand, &t_crop] { t.add_css_class("ws-flat"); }
        t_select.set_active(true);
        let zoom = gtk::DropDown::from_strings(&["Einpassen", "25 %", "50 %", "100 %", "200 %"]);
        zoom.set_tooltip_text(Some("Größe der Vorschau (Strg+Mausrad)"));
        let b_export = super::icon_button("export", "Exportieren", "Fertiges Video speichern");
        b_export.add_css_class("ws-primary");
        bar.append(&b_home);
        bar.append(&b_rec);
        bar.append(&b_import);
        let b_text = super::icon_button("text", "Text", "Text am Abspielkopf einfügen und gleich lostippen");
        bar.append(&b_text);
        bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        bar.append(&b_save);
        bar.append(&b_undo);
        bar.append(&b_redo);
        bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        bar.append(&t_select);
        bar.append(&t_hand);
        bar.append(&t_crop);
        bar.append(&zoom);
        let sp = hbox(0);
        sp.set_hexpand(true);
        bar.append(&sp);
        bar.append(&b_export);
        root.append(&bar);

        // ---- Hinweiszeile
        let note = label("", "ws-note");
        note.set_wrap(true);
        note.set_margin_start(10);
        note.set_margin_end(10);
        note.set_margin_top(6);
        let note_row = hbox(6);
        note.set_hexpand(true);
        note_row.append(&note);
        let note_close = super::icon_button("close", "", "Hinweis schließen");
        note_close.add_css_class("ws-flat");
        note_close.set_valign(gtk::Align::Center);
        note_row.append(&note_close);
        let note_rev = gtk::Revealer::builder().child(&note_row).reveal_child(false).build();
        root.append(&note_rev);

        // ---- Mitte: links Medien/Anmerkungen, Vorschau, rechts Eigenschaften
        let left = vbox(0);
        left.add_css_class("ws-panel");
        let lstack = gtk::Stack::new();
        lstack.set_vexpand(true);
        left.append(&lstack);
        // Medien
        let media_page = vbox(6);
        media_page.set_margin_start(8);
        media_page.set_margin_end(8);
        let media_box = vbox(4);
        let mscroll = gtk::ScrolledWindow::builder().child(&media_box).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        media_page.append(&mscroll);
        let mhint = label("Dateien hierher oder in die Zeitleiste ziehen. Doppelklick fügt am Abspielkopf ein.", "ws-dim ws-small");
        mhint.set_wrap(true);
        mhint.set_margin_bottom(8);
        media_page.append(&mhint);
        lstack.add_titled(&media_page, Some("media"), "Medien");
        // Anmerkungen
        let ann_page = vbox(6);
        ann_page.set_margin_start(8);
        ann_page.set_margin_end(8);
        ann_page.set_margin_top(4);
        let ann_hint = label("Fügt die Anmerkung am Abspielkopf ein. Danach in der Vorschau verschieben und rechts anpassen.", "ws-dim ws-small");
        ann_hint.set_wrap(true);
        ann_page.append(&ann_hint);
        let ann_items: [(&'static str, &str, Kind, Shape); 7] = [
            ("text", "Text", Kind::Text, Shape::Rounded),
            ("rect", "Rechteck", Kind::Shape, Shape::Rect),
            ("rounded", "Abgerundetes Rechteck", Kind::Shape, Shape::Rounded),
            ("ellipse", "Ellipse", Kind::Shape, Shape::Ellipse),
            ("arrow", "Pfeil", Kind::Arrow, Shape::Rect),
            ("line", "Linie", Kind::Arrow, Shape::Ellipse),
            ("blur", "Weichzeichner", Kind::Blur, Shape::Rect),
        ];
        let mut ann_btns = Vec::new();
        for (icon, name, kind, shape) in ann_items {
            let b = gtk::Button::new();
            let h = hbox(10);
            h.append(&icons::widget(icon, 20));
            h.append(&gtk::Label::new(Some(name)));
            b.set_child(Some(&h));
            ann_page.append(&b);
            ann_btns.push((b, kind, shape, icon));
        }
        lstack.add_titled(&ann_page, Some("ann"), "Anmerkungen");

        // Weitere Kategorien: Schaltflächen, die auf den ausgewählten Clip wirken
        let mut action_btns: Vec<(gtk::Button, &'static str)> = Vec::new();
        let mut action_page = |id: &str, title: &str, hint: &str, items: &[(&'static str, &str, &'static str)]| {
            let page = vbox(6);
            page.set_margin_start(8);
            page.set_margin_end(8);
            page.set_margin_top(8);
            page.append(&label(title, "ws-h"));
            let h = label(hint, "ws-dim ws-small");
            h.set_wrap(true);
            page.append(&h);
            for (icon, name, action) in items {
                let b = gtk::Button::new();
                b.add_css_class("ws-action");
                let hb = hbox(10);
                hb.append(&icons::widget(icon, 20));
                hb.append(&gtk::Label::new(Some(name)));
                b.set_child(Some(&hb));
                page.append(&b);
                action_btns.push((b, *action));
            }
            let sc = gtk::ScrolledWindow::builder().child(&page).hscrollbar_policy(gtk::PolicyType::Never).build();
            lstack.add_named(&sc, Some(id));
        };
        // Übergänge, Animationen und Effekte als Kachelraster: Jede Kachel zeigt im Bild, was sie tut.
        // Einträge: Gruppe (leer = ohne Überschrift), Name auf der Kachel, Erklärung, Aktion
        let mut tile_btns: Vec<(gtk::Button, &'static str)> = Vec::new();
        let mut tile_page = |id: &str, title: &str, hint: &str, items: &[(&str, &str, &str, &'static str)]| {
            let page = vbox(6);
            page.set_margin_start(8);
            page.set_margin_end(8);
            page.set_margin_top(8);
            page.set_margin_bottom(8);
            page.append(&label(title, "ws-h"));
            let h = label(hint, "ws-dim ws-small");
            h.set_wrap(true);
            page.append(&h);
            let mut grid: Option<gtk::Grid> = None;
            let mut n = 0;
            for (group, name, tip, action) in items {
                if !group.is_empty() || grid.is_none() {
                    if !group.is_empty() {
                        let g = label(group, "ws-dim ws-small");
                        g.set_margin_top(6);
                        page.append(&g);
                    }
                    let g = gtk::Grid::new();
                    g.set_row_spacing(6);
                    g.set_column_spacing(6);
                    g.set_column_homogeneous(true);
                    page.append(&g);
                    grid = Some(g);
                    n = 0;
                }
                let b = gtk::Button::new();
                b.add_css_class("ws-fxtile");
                b.set_tooltip_text(Some(tip));
                let v = vbox(4);
                v.append(&super::thumbs::widget(action, &b));
                let l = gtk::Label::new(Some(name));
                l.set_wrap(true);
                l.set_justify(gtk::Justification::Center);
                l.set_lines(2);
                l.set_max_width_chars(14);
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                v.append(&l);
                b.set_child(Some(&v));
                // Kachel lässt sich auf einen Clip in der Zeitleiste ziehen
                {
                    let drag = gtk::DragSource::new();
                    drag.set_actions(gdk::DragAction::COPY);
                    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
                    let act: &'static str = action;
                    drag.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&format!("wsfx:{act}").to_value())));
                    let wb = b.downgrade();
                    drag.connect_drag_begin(move |src, _| {
                        if let Some(b) = wb.upgrade() { src.set_icon(Some(&gtk::WidgetPaintable::new(Some(&b))), 20, 20); }
                    });
                    b.add_controller(drag);
                }
                if let Some(g) = &grid { g.attach(&b, n % 2, n / 2, 1, 1); }
                n += 1;
                tile_btns.push((b, *action));
            }
            let sc = gtk::ScrolledWindow::builder().child(&page).hscrollbar_policy(gtk::PolicyType::Never).build();
            lstack.add_named(&sc, Some(id));
        };
        tile_page("trans", "Übergänge", "Clip auswählen und Kachel anklicken, oder die Kachel auf einen Clip in der Zeitleiste ziehen. Die Dauer stellst du danach an den Griffen oben am Clip ein.", &[
            ("Zwischen zwei Clips", "Überblenden", "Weich überblenden (zum Vorgänger). Die Clips überlappen sich dabei um eine halbe Sekunde.", "xfade"),
            ("", "Über Schwarz", "Der Clip davor blendet nach Schwarz, dieser kommt aus Schwarz (je 0,4 s).", "t_black"),
            ("", "Über Weiß", "Der Clip davor blendet nach Weiß, dieser kommt aus Weiß (je 0,4 s).", "t_white"),
            ("", "Lichtblitz", "Kurzer weißer Blitz am Schnitt (je 0,15 s).", "t_flash"),
            ("", "Zoom", "Hinein in den Clip davor, heraus aus diesem (je 0,4 s).", "t_zoom"),
            ("", "Schwenk nach links", "Schneller Schwenk mit Bewegungsunschärfe, das Bild zieht nach links weg.", "t_whipl"),
            ("", "Schwenk nach rechts", "Schneller Schwenk mit Bewegungsunschärfe, das Bild zieht nach rechts weg.", "t_whipr"),
            ("", "Unschärfe", "Der Clip davor wird unscharf, dieser wird scharf (je 0,5 s).", "t_blur"),
            ("", "Störung (Glitch)", "Kurze digitale Störung mit versetzten Farben am Schnitt.", "t_glitch"),
            ("Am Anfang und Ende", "Einblenden", "Einblenden (0,5 s)", "fadein"),
            ("", "Ausblenden", "Ausblenden (0,5 s)", "fadeout"),
            ("", "Ein und aus", "Ein- und ausblenden", "fadeboth"),
            ("", "Aufrollen", "Aufrollen (von links, 0,6 s)", "rollin"),
            ("", "Zusammenrollen", "Zusammenrollen (nach links, 0,6 s)", "rollout"),
            ("", "Auf und zu", "Auf- und zusammenrollen", "rollboth"),
            ("", "Entfernen", "Alle Blenden und Übergänge des Clips entfernen", "fadenone"),
        ]);
        tile_page("anim", "Animationen", "Kachel anklicken oder auf einen Clip in der Zeitleiste ziehen. Zoom und Bewegung beginnen dort, wo du loslässt, und erscheinen als gelber Balken zum Verschieben.", &[
            ("Zoom und Bewegung", "Hineinzoomen", "Hineinzoomen (auf den Mauszeiger)", "zoomin"),
            ("", "Zurückzoomen", "Zurück auf die Ausgangsgröße", "zoomout"),
            ("", "Eigene Bewegung", "Eigene Bewegung: Zielgröße und Ziellage danach in der Vorschau einstellen", "animcustom"),
            ("", "Entfernen", "Alle Bewegungen des Clips entfernen", "animnone"),
            ("Hineinkommen", "Von links", "Gleitet am Anfang von links ins Bild (0,5 s)", "slidein_l"),
            ("", "Von rechts", "Gleitet am Anfang von rechts ins Bild (0,5 s)", "slidein_r"),
            ("", "Von oben", "Gleitet am Anfang von oben ins Bild (0,5 s)", "slidein_u"),
            ("", "Von unten", "Gleitet am Anfang von unten ins Bild (0,5 s)", "slidein_d"),
            ("", "Aufspringen", "Springt am Anfang mit kurzem Überschwingen auf (0,5 s)", "popin"),
            ("Hinausgehen", "Nach links", "Gleitet am Ende nach links aus dem Bild (0,5 s)", "slideout_l"),
            ("", "Nach rechts", "Gleitet am Ende nach rechts aus dem Bild (0,5 s)", "slideout_r"),
            ("", "Nach oben", "Gleitet am Ende nach oben aus dem Bild (0,5 s)", "slideout_u"),
            ("", "Nach unten", "Gleitet am Ende nach unten aus dem Bild (0,5 s)", "slideout_d"),
            ("", "Schrumpfen", "Schrumpft am Ende zusammen (0,4 s)", "popout"),
        ]);
        tile_page("fx", "Effekte", "Wirken auf den ausgewählten Clip. Jede Kachel schaltet an und wieder aus; die Feineinstellung steht rechts.", &[
            ("Form", "Als Kreis", "Als Kreis (mittig zuschneiden)", "circle"),
            ("", "Runde Ecken", "Abgerundete Ecken", "rounded"),
            ("", "Rechteck", "Rechteck", "rectshape"),
            ("", "Rahmen", "Rahmen an / aus", "border"),
            ("", "Schatten", "Schatten an / aus", "shadow"),
            ("", "Halb durchsichtig", "Halb durchsichtig an / aus", "opacity"),
            ("Kamera", "Greenscreen", "Greenscreen an / aus", "chroma"),
            ("", "Hintergrund weg", "Hintergrund entfernen an / aus (ohne Greenscreen)", "bgremove"),
            ("", "Hintergrund unscharf", "Hintergrund unscharf an / aus", "bgblur"),
            ("", "Hintergrundbild", "Hintergrundbild wählen …", "bgimage"),
            ("Farbe", "Heller", "Heller (jeder Klick ein Schritt)", "c_bright"),
            ("", "Dunkler", "Dunkler (jeder Klick ein Schritt)", "c_dark"),
            ("", "Mehr Kontrast", "Mehr Kontrast (jeder Klick ein Schritt)", "c_contrast"),
            ("", "Kräftigere Farben", "Kräftigere Farben (jeder Klick ein Schritt)", "c_sat"),
            ("", "Schwarzweiß", "Schwarzweiß an / aus", "c_bw"),
            ("", "Wärmer", "Wärmere Farben (jeder Klick ein Schritt)", "c_warm"),
            ("", "Kälter", "Kältere Farben (jeder Klick ein Schritt)", "c_cool"),
            ("", "Zurücksetzen", "Farbkorrektur zurücksetzen", "c_reset"),
            ("Bild", "Spiegeln", "Waagerecht spiegeln an / aus", "fliph"),
            ("", "Auf den Kopf", "Senkrecht spiegeln an / aus", "flipv"),
            ("", "Vignette", "Bildränder abdunkeln an / aus", "vignette"),
            ("", "Verpixeln", "Legt einen Bereich an, der alles darunter verpixelt (Datenschutz). Größe und Lage in der Vorschau einstellen.", "pixelate"),
        ]);
        action_page("cursor", "Mauszeiger", "Wirkt auf Bildschirmaufnahmen, die mit Zeigerdaten aufgenommen wurden.", &[
            ("cursor", "Mauszeiger zeigen / verbergen", "curshow"),
            ("circle", "Hervorheben an / aus", "curhl"),
            ("circle", "Klick-Ringe an / aus", "curclicks"),
            ("plus", "Zeiger größer", "curbig"),
            ("minus", "Zeiger kleiner", "cursmall"),
            ("motion", "Bewegung glätten an / aus", "cursmooth"),
        ]);
        action_page("audio", "Ton", "Die Tonbearbeitung gilt für alle Teile einer Aufnahme. Ohne Auswahl für allen Ton im Projekt.", &[
            ("wand", "Ton verbessern (alles mit einem Klick)", "a_improve"),
            ("speaker", "Rauschen entfernen an / aus", "a_denoise"),
            ("minus", "Brummen und Poltern entfernen an / aus", "a_lowcut"),
            ("motion", "Lautstärke angleichen an / aus", "a_compress"),
            ("plus", "Normale Lautstärke an / aus", "a_normalize"),
            ("mic", "Stimme klarer an / aus", "a_voice"),
            ("pause", "Pausen leiser an / aus", "a_gate"),
            ("undo", "Tonbearbeitung zurücksetzen", "a_reset"),
            ("mute", "Stumm an / aus", "mute"),
            ("plus", "Lauter", "louder"),
            ("minus", "Leiser", "quieter"),
            ("transition", "Ton einblenden (1 s)", "afadein"),
            ("transition", "Ton ausblenden (1 s)", "afadeout"),
        ]);

        // Kategorienleiste ganz links
        let rail = vbox(4);
        rail.add_css_class("ws-rail");
        let rail_items: [(&'static str, &str, &str); 7] = [
            ("film", "Medien", "media"), ("text", "Anmerkungen", "ann"), ("transition", "Übergänge", "trans"), ("motion", "Animationen", "anim"),
            ("wand", "Effekte", "fx"), ("cursor", "Mauszeiger", "cursor"), ("speaker", "Ton", "audio"),
        ];
        let mut first_rail: Option<gtk::ToggleButton> = None;
        for (icon, name, id) in rail_items {
            let b = gtk::ToggleButton::new();
            b.add_css_class("ws-railbtn");
            // nur das Symbol: Der Name steht im Tooltip, die Leiste bleibt schmal
            let ic = icons::widget(icon, 22);
            ic.set_halign(gtk::Align::Center);
            b.set_child(Some(&ic));
            b.set_tooltip_text(Some(name));
            match &first_rail { Some(f) => b.set_group(Some(f)), None => { b.set_active(true); first_rail = Some(b.clone()); } }
            let (st, id) = (lstack.clone(), id.to_string());
            b.connect_toggled(move |b| { if b.is_active() { st.set_visible_child_name(&id); } });
            rail.append(&b);
        }

        let canvas_area = gtk::DrawingArea::new();
        canvas_area.set_hexpand(true);
        canvas_area.set_vexpand(true);
        canvas_area.set_focusable(true);
        let center = vbox(0);
        center.append(&canvas_area);
        // Abspielleiste
        let transport = hbox(6);
        transport.add_css_class("ws-bottombar");
        let b_start = super::icon_button("start", "", "Zum Anfang (Pos1)");
        let b_prev = super::icon_button("prev", "", "Ein Bild zurück (Pfeil links)");
        let play_icon = Rc::new(Cell::new("play"));
        let play_btn = gtk::Button::new();
        play_btn.set_tooltip_text(Some("Abspielen / Anhalten (Leertaste)"));
        let pa = gtk::DrawingArea::new();
        pa.set_content_width(20);
        pa.set_content_height(20);
        let pi = play_icon.clone();
        pa.set_draw_func(move |w, cr, width, _| {
            let c = w.color();
            icons::draw(cr, pi.get(), width as f64, (c.red() as f64, c.green() as f64, c.blue() as f64, 1.0));
        });
        play_btn.set_child(Some(&pa));
        play_btn.add_css_class("ws-primary");
        let b_next = super::icon_button("next", "", "Ein Bild vor (Pfeil rechts)");
        let b_end = super::icon_button("end", "", "Zum Ende (Ende)");
        for b in [&b_start, &b_prev, &b_next, &b_end] { b.add_css_class("ws-flat"); }
        let time_label = label("00:00;00 / 00:00;00", "ws-time");
        let tsp = hbox(0);
        tsp.set_hexpand(true);
        transport.append(&tsp);
        transport.append(&b_start);
        transport.append(&b_prev);
        transport.append(&play_btn);
        transport.append(&b_next);
        transport.append(&b_end);
        transport.append(&time_label);
        // Hinweis, wenn die Vorschau mit gesenkter Bildrate läuft (der Export ist davon nicht betroffen)
        let fps_hint = label("", "ws-dim");
        fps_hint.set_tooltip_text(Some("Der Rechner schafft die Vorschau nicht mit voller Bildrate. Damit Bild und Ton zusammenbleiben, zeigt die Vorschau weniger Bilder je Sekunde. Der Export ist davon nicht betroffen."));
        transport.append(&fps_hint);
        let tsp2 = hbox(0);
        tsp2.set_hexpand(true);
        transport.append(&tsp2);
        center.append(&transport);

        // Beide Seitenleisten haben eine feste Breite: sonst springt die Vorschau, sobald sich ihr Inhalt ändert.
        let right = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::External).build();
        right.add_css_class("ws-panel");
        {
            // Das Mausrad blättert nur durch die Leiste: Es darf nie nebenbei Regler oder Zahlenfelder verstellen.
            let sc = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
            sc.set_propagation_phase(gtk::PropagationPhase::Capture);
            let r = right.clone();
            sc.connect_scroll(move |_, _, dy| {
                let a = r.vadjustment();
                a.set_value((a.value() + dy * 60.0).clamp(a.lower(), (a.upper() - a.page_size()).max(a.lower())));
                glib::Propagation::Stop
            });
            right.add_controller(sc);
        }
        // feste Breite, die sich nicht mit der Vorschau um den freien Platz streitet
        right.set_size_request(super::props::PANEL_W, -1);
        right.set_hexpand(false);
        let left_wrap = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::External).vscrollbar_policy(gtk::PolicyType::Never).child(&left).build();
        left_wrap.add_css_class("ws-panel");
        left_wrap.set_size_request(310, -1);
        left_wrap.set_hexpand(false);

        let mid = hbox(0);
        mid.append(&rail);
        mid.append(&left_wrap);
        mid.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        mid.append(&center);
        mid.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        mid.append(&right);

        // ---- Zeitleiste
        let tl_bar = hbox(6);
        tl_bar.add_css_class("ws-toolbar");
        let b_cut = super::icon_toggle("scissors", "Schneiden", "Schere (X): Klick schneidet an der roten Linie. Ziehen markiert einen Bereich, den du danach herausnimmst. Esc beendet.");
        b_cut.add_css_class("ws-flat");
        let b_cut_all = gtk::ToggleButton::with_label("Alle Ebenen");
        b_cut_all.add_css_class("ws-flat");
        b_cut_all.add_css_class("ws-cutall");
        b_cut_all.set_active(true);
        b_cut_all.set_visible(false);
        b_cut_all.set_tooltip_text(Some("An: der Schnitt geht durch alle Ebenen (gesperrte bleiben aus). Aus: nur die Ebene unter der Maus. Umschalt kehrt es für einen Schnitt um."));
        tl_bar.append(&b_cut);
        tl_bar.append(&b_cut_all);
        let b_split = super::icon_button("split", "Teilen", "Am Abspielkopf teilen (S)");
        let b_del = super::icon_button("delete", "", "Auswahl löschen (Entf)");
        let b_ripple = super::icon_button("ripple", "Lücke schließen", "Auswahl oder markierten Bereich löschen und alles Folgende nachrücken (Umschalt+Entf)");
        let b_fade = super::icon_button("ellipse", "Überblenden", "Ausgewählten Clip weich in den vorherigen überblenden");
        let b_track = super::icon_button("plus", "Spur", "Neue Spur hinzufügen");
        let b_zout = super::icon_button("minus", "", "Zeitleiste verkleinern");
        let b_zin = super::icon_button("plus", "", "Zeitleiste vergrößern");
        let b_zfit = super::icon_button("fit", "", "Ganzes Projekt zeigen");
        for b in [&b_split, &b_del, &b_ripple, &b_fade, &b_track, &b_zout, &b_zin, &b_zfit] { b.add_css_class("ws-flat"); }
        tl_bar.append(&b_split);
        tl_bar.append(&b_del);
        tl_bar.append(&b_ripple);
        tl_bar.append(&b_fade);
        tl_bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        tl_bar.append(&b_track);
        let range_hint = label("Blenden: Griffe oben am Clip ziehen · Bewegung: gelben Balken ziehen", "ws-dim ws-small");
        range_hint.set_hexpand(true);
        range_hint.set_xalign(0.5);
        tl_bar.append(&range_hint);
        tl_bar.append(&b_zout);
        tl_bar.append(&b_zin);
        tl_bar.append(&b_zfit);
        let tl_area = gtk::DrawingArea::new();
        tl_area.set_hexpand(true);
        tl_area.set_vexpand(true);
        tl_area.set_focusable(true);
        let tl_scrollbar = gtk::Scrollbar::new(gtk::Orientation::Horizontal, Some(&gtk::Adjustment::new(0.0, 0.0, 60.0, 1.0, 10.0, 30.0)));
        let bottom = vbox(0);
        bottom.append(&tl_bar);
        bottom.append(&tl_area);
        bottom.append(&tl_scrollbar);
        bottom.set_size_request(-1, 300);

        let paned = gtk::Paned::new(gtk::Orientation::Vertical);
        paned.set_start_child(Some(&mid));
        paned.set_end_child(Some(&bottom));
        paned.set_resize_start_child(true);
        paned.set_resize_end_child(false);
        paned.set_shrink_start_child(false);
        paned.set_shrink_end_child(false);
        paned.set_vexpand(true);
        root.append(&paned);

        let ed = Rc::new(Editor {
            app: Rc::downgrade(app),
            root,
            st: RefCell::new(State {
                project, path, dirty: false, sel: vec![], playhead: 0.0, undo: vec![], redo: vec![], tool: Tool::Select, view_zoom: 0.0, view_pan: (0.0, 0.0),
                tl_pps: 60.0, tl_scroll: 0.0, tl_vscroll: 0.0, range: None, range_tracks: vec![], tl_cut: false, cut_all: true, cut_hover: None, clipboard: vec![], canvas_drag: None, tl_drag: None, guides: vec![],
            }),
            engine,
            canvas: canvas_area,
            timeline: tl_area,
            tl_scrollbar,
            time_label,
            play_icon,
            play_btn,
            props: RefCell::new(None),
            media_box,
            note,
            note_rev,
            tool_btns: RefCell::new(vec![t_select.clone(), t_hand.clone(), t_crop.clone()]),
            cut_btns: RefCell::new(vec![b_cut.clone(), b_cut_all.clone()]),
            frame_cache: RefCell::new((0, None)),
            thumbs: RefCell::new(HashMap::new()),
            waves: RefCell::new(HashMap::new()),
            bg: Arc::new(Mutex::new(Vec::new())),
            bg_pending: RefCell::new(HashSet::new()),
            last_edit: RefCell::new((String::new(), std::time::Instant::now())),
            last_serial: Cell::new(0),
            was_playing: Cell::new(false),
            last_mark: Cell::new((-1, -1)),
            last_hint: Cell::new(0),
            fps_hint,
            last_autosave: Cell::new(std::time::Instant::now()),
            media_sig: RefCell::new(String::new()),
            updating_scroll: Cell::new(false),
        });

        let p = props::Props::new(&ed);
        right.set_child(Some(&p.root));
        *ed.props.borrow_mut() = Some(p);
        canvas::setup(&ed);
        timeline::setup(&ed);

        // ---- Signale
        macro_rules! on {
            ($w:expr, $ed:ident => $body:expr) => {{
                let weak = Rc::downgrade(&ed);
                $w.connect_clicked(move |_| { if let Some($ed) = weak.upgrade() { $body } });
            }};
        }
        {
            let a = app.clone();
            b_home.connect_clicked(move |_| a.show_start());
            let a = app.clone();
            b_rec.connect_clicked(move |_| a.open_recorder());
            let a = app.clone();
            b_import.connect_clicked(move |_| a.import_dialog());
        }
        on!(b_save, e => { e.save(); e.show_note("Projekt gespeichert."); });
        for (b, action) in action_btns.into_iter().chain(tile_btns) {
            let weak = Rc::downgrade(&ed);
            b.connect_clicked(move |_| { if let Some(e) = weak.upgrade() { e.quick_action(action); } });
        }
        on!(b_text, e => {
            e.add_annotation(Kind::Text, Shape::Rounded, false);
            let p = e.props.borrow().clone();
            if let Some(p) = p { glib::timeout_add_local_once(std::time::Duration::from_millis(80), move || p.focus_text()); }
        });
        on!(b_undo, e => e.undo());
        on!(b_redo, e => e.redo());
        on!(b_export, e => export::open(&e));
        on!(note_close, e => e.note_rev.set_reveal_child(false));
        on!(ed.play_btn, e => e.toggle_play());
        on!(b_start, e => e.seek(0.0));
        on!(b_end, e => { let d = e.st.borrow().project.duration(); e.seek(d); });
        on!(b_prev, e => e.step(-1));
        on!(b_next, e => e.step(1));
        {
            let weak = Rc::downgrade(&ed);
            b_cut.connect_toggled(move |b| { if let Some(e) = weak.upgrade() { e.set_cut_tool(b.is_active()); } });
            let weak = Rc::downgrade(&ed);
            b_cut_all.connect_toggled(move |b| { if let Some(e) = weak.upgrade() { e.st.borrow_mut().cut_all = b.is_active(); e.timeline.queue_draw(); } });
        }
        on!(b_split, e => e.split());
        on!(b_del, e => e.delete(false));
        on!(b_ripple, e => e.delete(true));
        on!(b_fade, e => e.crossfade_selected());
        on!(b_track, e => e.edit(|p| { let n = p.tracks.len() + 1; p.add_track(&format!("Spur {n}")); }));
        on!(b_zin, e => timeline::zoom(&e, 1.4, None));
        on!(b_zout, e => timeline::zoom(&e, 1.0 / 1.4, None));
        on!(b_zfit, e => timeline::zoom_fit(&e));
        for (i, t) in [t_select, t_hand, t_crop].iter().enumerate() {
            let weak = Rc::downgrade(&ed);
            t.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() { e.set_tool([Tool::Select, Tool::Hand, Tool::Crop][i]); }
            });
        }
        {
            let weak = Rc::downgrade(&ed);
            zoom.connect_selected_notify(move |d| {
                if let Some(e) = weak.upgrade() {
                    let z = [0.0, 0.25, 0.5, 1.0, 2.0][d.selected().min(4) as usize];
                    let mut st = e.st.borrow_mut();
                    st.view_zoom = z;
                    st.view_pan = (0.0, 0.0);
                    drop(st);
                    e.canvas.queue_draw();
                }
            });
        }
        for (b, kind, shape, icon) in ann_btns {
            let weak = Rc::downgrade(&ed);
            b.connect_clicked(move |_| {
                if let Some(e) = weak.upgrade() { e.add_annotation(kind, shape, icon == "line"); }
            });
        }
        // Dateien auf die Medienliste ziehen
        {
            let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
            let weak = Rc::downgrade(&ed);
            drop.connect_drop(move |_, value, _, _| {
                let Some(e) = weak.upgrade() else { return false };
                let Ok(list) = value.get::<gdk::FileList>() else { return false };
                let paths: Vec<String> = list.files().iter().filter_map(|f| f.path()).map(|p| p.to_string_lossy().to_string()).collect();
                e.import_files(&paths, false);
                true
            });
            left.add_controller(drop);
        }
        // Tastatur
        {
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            let weak = Rc::downgrade(&ed);
            keys.connect_key_pressed(move |_, key, _, state| {
                let Some(e) = weak.upgrade() else { return glib::Propagation::Proceed };
                if e.key_pressed(key, state) { glib::Propagation::Stop } else { glib::Propagation::Proceed }
            });
            ed.root.add_controller(keys);
        }
        // Takt: Vorschau, Abspielkopf, Hintergrundergebnisse
        {
            let weak = Rc::downgrade(&ed);
            ed.canvas.add_tick_callback(move |_, _| {
                let Some(e) = weak.upgrade() else { return glib::ControlFlow::Break };
                e.tick();
                glib::ControlFlow::Continue
            });
        }
        {
            let weak = Rc::downgrade(&ed);
            ed.tl_scrollbar.adjustment().connect_value_changed(move |a| {
                if let Some(e) = weak.upgrade() {
                    if e.updating_scroll.get() { return; }
                    e.st.borrow_mut().tl_scroll = a.value();
                    e.timeline.queue_draw();
                }
            });
        }
        ed.refresh_media();
        ed.refresh_props();
        ed.update_time();
        {
            // Zeitleiste einpassen, sobald sie ihre echte Breite hat (direkt nach dem Öffnen ist sie noch 0)
            let weak = Rc::downgrade(&ed);
            let tries = Cell::new(0u32);
            glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
                let Some(e) = weak.upgrade() else { return glib::ControlFlow::Break };
                tries.set(tries.get() + 1);
                if e.timeline.width() < 300 && tries.get() < 60 { return glib::ControlFlow::Continue; }
                timeline::zoom_fit(&e);
                glib::ControlFlow::Break
            });
        }
        Ok(ed)
    }

    // ---------------------------------------------------------------------------------------
    // Grundfunktionen

    pub fn updating_scroll_set(&self, v: bool) { self.updating_scroll.set(v); }

    pub fn window(&self) -> Option<gtk::ApplicationWindow> { self.app.upgrade().map(|a| a.window.clone()) }

    pub fn show_note(&self, text: &str) {
        self.note.set_text(text);
        self.note_rev.set_reveal_child(true);
        if text.len() < 60 {
            let rev = self.note_rev.clone();
            let note = self.note.clone();
            let t = text.to_string();
            glib::timeout_add_seconds_local_once(4, move || { if note.text() == t { rev.set_reveal_child(false); } });
        }
    }

    fn push_undo(&self) {
        let mut st = self.st.borrow_mut();
        let snap = st.project.clone();
        st.undo.push(snap);
        if st.undo.len() > 100 { st.undo.remove(0); }
        st.redo.clear();
    }

    /// Änderung mit Rückgängig-Schritt.
    pub fn edit(&self, f: impl FnOnce(&mut Project)) {
        self.push_undo();
        f(&mut self.st.borrow_mut().project);
        *self.last_edit.borrow_mut() = (String::new(), std::time::Instant::now());
        self.changed();
    }

    /// Wie `edit`, fasst aber schnelle Folgen derselben Änderung (Schieberegler) zu einem Schritt zusammen.
    pub fn edit_coalesced(&self, key: &str, f: impl FnOnce(&mut Project)) {
        let fresh = {
            let le = self.last_edit.borrow();
            le.0 != key || le.1.elapsed().as_secs_f64() > 1.0
        };
        if fresh { self.push_undo(); }
        *self.last_edit.borrow_mut() = (key.to_string(), std::time::Instant::now());
        f(&mut self.st.borrow_mut().project);
        self.changed();
    }

    /// Für Ziehvorgänge: einmal sichern, dann mit `modify` laufend ändern.
    pub fn begin_change(&self) {
        self.push_undo();
        *self.last_edit.borrow_mut() = (String::new(), std::time::Instant::now());
    }

    /// Änderung ohne Rückgängig-Schritt; `sync` gleicht die Vorschau sofort ab.
    pub fn modify(&self, sync: bool, f: impl FnOnce(&mut Project)) {
        {
            let mut st = self.st.borrow_mut();
            f(&mut st.project);
            st.dirty = true;
        }
        if sync { self.sync_engine(); }
        self.canvas.queue_draw();
        self.timeline.queue_draw();
    }

    fn sync_engine(&self) {
        // Neue Medien zuerst laden, solange nichts ausgeliehen ist: GES lässt dabei die Hauptschleife laufen.
        let t0 = std::time::Instant::now();
        let uris = { let st = self.st.borrow(); crate::engine::project_uris(&st.project) };
        let t1 = t0.elapsed().as_millis();
        for u in uris { crate::engine::preload_asset(&u); }
        let t2 = t0.elapsed().as_millis();
        let st = self.st.borrow();
        if let Ok(mut e) = self.engine.try_borrow_mut() { e.sync(&st.project); }
        if std::env::var_os("WS_DEBUG").is_some() { eprintln!("sync: Bilder {t1} ms, Laden {} ms, Abgleich {} ms", t2 - t1, t0.elapsed().as_millis() - t2); }
    }

    /// Nach jeder Änderung am Projekt.
    pub fn changed(&self) {
        {
            let mut st = self.st.borrow_mut();
            st.dirty = true;
            for c in st.project.clips.iter_mut() { crate::annot::fit_text_height(c); }
            let ids: HashSet<u64> = st.project.clips.iter().map(|c| c.id).collect();
            st.sel.retain(|id| ids.contains(id));
            let d = st.project.duration();
            if st.playhead > d { st.playhead = d; }
        }
        self.ensure_audio_profiles();
        self.sync_engine();
        let (ph, playing) = (self.st.borrow().playhead, self.engine.borrow().is_playing());
        if !playing && (self.engine.borrow().position() - ph).abs() > 1e-6 { self.engine.borrow_mut().seek(ph); }
        self.canvas.queue_draw();
        self.timeline.queue_draw();
        self.refresh_media();
        self.refresh_props();
        self.update_time();
        timeline::update_scrollbar(self);
    }

    pub fn refresh_props(&self) {
        let p = self.props.borrow().clone();
        let t0 = std::time::Instant::now();
        if let Some(p) = p { p.refresh(self); }
        if std::env::var_os("WS_DEBUG").is_some() && t0.elapsed().as_millis() > 5 { eprintln!("refresh_props: {} ms", t0.elapsed().as_millis()); }
    }

    pub fn undo(&self) {
        {
            let mut st = self.st.borrow_mut();
            let Some(prev) = st.undo.pop() else { return };
            let cur = std::mem::replace(&mut st.project, prev);
            st.redo.push(cur);
        }
        self.changed();
    }
    pub fn redo(&self) {
        {
            let mut st = self.st.borrow_mut();
            let Some(next) = st.redo.pop() else { return };
            let cur = std::mem::replace(&mut st.project, next);
            st.undo.push(cur);
        }
        self.changed();
    }

    pub fn save(&self) {
        let mut st = self.st.borrow_mut();
        if let Err(e) = st.project.save(&st.path) {
            eprintln!("ws-video: Speichern fehlgeschlagen: {e}");
        } else {
            st.dirty = false;
        }
    }

    pub fn pause(&self) {
        self.engine.borrow_mut().pause();
        self.play_icon.set("play");
        if let Some(c) = self.play_btn.child() { c.queue_draw(); }
    }

    pub fn shutdown(&self) {
        self.engine.borrow_mut().shutdown();
    }

    pub fn toggle_play(&self) {
        let playing = self.engine.borrow().is_playing();
        if playing {
            self.pause();
            let pos = self.engine.borrow().position();
            self.st.borrow_mut().playhead = pos;
            self.refresh_props();
        } else {
            let ph = self.st.borrow().playhead;
            let mut e = self.engine.borrow_mut();
            if (e.position() - ph).abs() > 0.02 { e.seek(ph); }
            e.play();
            if e.is_playing() { self.play_icon.set("pause"); }
            drop(e);
            if let Some(c) = self.play_btn.child() { c.queue_draw(); }
        }
    }

    pub fn seek(&self, t: f64) {
        let (t, fps) = {
            let st = self.st.borrow();
            (t.clamp(0.0, st.project.duration().max(0.0)), st.project.fps)
        };
        let t = ops::snap_frame(t, fps);
        self.st.borrow_mut().playhead = t;
        self.engine.borrow_mut().seek(t);
        self.canvas.queue_draw();
        self.timeline.queue_draw();
        self.update_time();
        if !self.engine.borrow().is_playing() { self.refresh_props(); }
    }

    pub fn step(&self, frames: i32) {
        if self.engine.borrow().is_playing() { self.pause(); }
        let (ph, fps) = { let st = self.st.borrow(); (st.playhead, st.project.fps.max(1)) };
        self.seek(ph + frames as f64 / fps as f64);
    }

    pub fn update_time(&self) {
        let st = self.st.borrow();
        self.time_label.set_text(&format!("{} / {}", format_time(st.playhead, st.project.fps), format_time(st.project.duration(), st.project.fps)));
    }

    fn tick(&self) {
        let (playing, pos, serial, ended, reduced) = {
            let Ok(mut e) = self.engine.try_borrow_mut() else { return };
            e.tick();
            (e.is_playing(), e.position(), e.frame_serial(), e.ended, e.reduced_fps())
        };
        let hint = reduced.map(|f| (f * 10.0).round() as u32).unwrap_or(0);
        if hint != self.last_hint.replace(hint) {
            self.fps_hint.set_text(&match reduced { Some(f) => format!("Vorschau {} Bilder/s", format!("{f:.1}").trim_end_matches(".0").replace('.', ",")), None => String::new() });
        }
        if playing {
            self.st.borrow_mut().playhead = pos;
            timeline::follow_playhead(self);
            // Die Zeitleiste nur neu zeichnen, wenn die Abspielmarke wenigstens einen Bildpunkt weitergerückt ist.
            let mark = { let st = self.st.borrow(); ((pos * st.tl_pps).round() as i64, (st.tl_scroll * st.tl_pps).round() as i64) };
            if mark != self.last_mark.replace(mark) { self.timeline.queue_draw(); }
            self.update_time();
        }
        if self.was_playing.get() && !playing {
            // Wiedergabe ist von selbst zu Ende gegangen
            if ended { self.st.borrow_mut().playhead = pos; }
            self.play_icon.set("play");
            if let Some(c) = self.play_btn.child() { c.queue_draw(); }
            self.timeline.queue_draw();
            self.update_time();
        }
        self.was_playing.set(playing);
        if serial != self.last_serial.get() {
            self.last_serial.set(serial);
            self.canvas.queue_draw();
        }
        // Ergebnisse aus Hintergrund-Threads (Vorschaubilder, Wellenformen)
        let results: Vec<BgResult> = std::mem::take(&mut *self.bg.lock().unwrap());
        if !results.is_empty() {
            for r in results {
                match r {
                    BgResult::Thumb(path, f) => { self.thumbs.borrow_mut().insert(path, f.and_then(|f| super::frame_surface(&f))); }
                    BgResult::Wave(path, w) => { self.waves.borrow_mut().insert(path, Rc::new(w.unwrap_or_default())); }
                    BgResult::Audio(path, prof) => self.audio_measured(path, prof),
                }
            }
            self.timeline.queue_draw();
            *self.media_sig.borrow_mut() = String::new();
            self.refresh_media();
        }
        // regelmäßig sichern
        if self.st.borrow().dirty && self.last_autosave.get().elapsed().as_secs() >= 5 {
            self.last_autosave.set(std::time::Instant::now());
            self.save();
        }
    }

    /// Vorschaubild anfordern (lädt im Hintergrund).
    pub fn request_thumb(&self, path: &str) {
        if path.is_empty() || self.thumbs.borrow().contains_key(path) { return; }
        let key = format!("t:{path}");
        if !self.bg_pending.borrow_mut().insert(key) { return; }
        let (bg, p) = (self.bg.clone(), path.to_string());
        std::thread::spawn(move || {
            let f = engine::thumbnail(&p, 0.5, 96).or_else(|| engine::thumbnail(&p, 0.0, 96));
            bg.lock().unwrap().push(BgResult::Thumb(p, f));
        });
    }
    pub fn request_wave(&self, path: &str) {
        if path.is_empty() || self.waves.borrow().contains_key(path) { return; }
        let key = format!("w:{path}");
        if !self.bg_pending.borrow_mut().insert(key) { return; }
        let (bg, p) = (self.bg.clone(), path.to_string());
        std::thread::spawn(move || {
            let w = engine::waveform(&p);
            bg.lock().unwrap().push(BgResult::Wave(p, w));
        });
    }

    // ---------------------------------------------------------------------------------------
    // Tonbearbeitung

    /// Tonbearbeitung ändern. Wirkt auf die ausgewählten Clips mit Ton und auf alle anderen Teile derselben
    /// Aufnahme. Ist kein Clip mit Ton ausgewählt, wirkt sie auf alle Clips mit Ton im Projekt.
    pub fn audio_change(&self, key: &str, f: impl Fn(&mut crate::model::AudioFx)) -> bool {
        let (paths, all) = {
            let st = self.st.borrow();
            let with = |only_sel: bool| -> std::collections::BTreeSet<String> {
                st.project.clips.iter().filter(|c| c.carries_audio() && (!only_sel || st.sel.contains(&c.id))).map(|c| c.path.clone()).collect()
            };
            let sel = with(true);
            if sel.is_empty() { (with(false), true) } else { (sel, false) }
        };
        if paths.is_empty() { self.show_note("In diesem Projekt gibt es noch keinen Clip mit Ton."); return false; }
        let change = |p: &mut Project| { for c in p.clips.iter_mut().filter(|c| c.carries_audio() && paths.contains(&c.path)) { f(&mut c.audio); } };
        if key.is_empty() { self.edit(change); } else { self.edit_coalesced(key, change); }
        if all && key.is_empty() { self.show_note("Angewendet auf alle Clips mit Ton."); }
        true
    }

    /// Schnellaktionen der Seite „Ton“.
    fn audio_action(&self, action: &str) {
        // An oder aus richtet sich nach dem ersten betroffenen Clip
        let cur = {
            let st = self.st.borrow();
            let pick = |only_sel: bool| st.project.clips.iter().find(|c| c.carries_audio() && (!only_sel || st.sel.contains(&c.id))).map(|c| c.audio.clone());
            pick(true).or_else(|| pick(false)).unwrap_or_default()
        };
        match action {
            "a_improve" => { self.audio_change("", |a| *a = crate::model::AudioFx::improved()); }
            "a_denoise" => { let v = if cur.denoise > 0.001 { 0.0 } else { 0.6 }; self.audio_change("", move |a| a.denoise = v); }
            "a_lowcut" => { let v = !cur.lowcut; self.audio_change("", move |a| a.lowcut = v); }
            "a_compress" => { let v = if cur.compress > 0.001 { 0.0 } else { 0.4 }; self.audio_change("", move |a| a.compress = v); }
            "a_normalize" => { let v = !cur.normalize; self.audio_change("", move |a| a.normalize = v); }
            "a_voice" => { let v = !cur.voice; self.audio_change("", move |a| a.voice = v); }
            "a_gate" => { let v = if cur.gate > 0.001 { 0.0 } else { 0.5 }; self.audio_change("", move |a| a.gate = v); }
            _ => { self.audio_change("", |a| *a = Default::default()); }
        }
    }

    /// Fehlende Messungen (Rauschprofil, Lautstärke) im Hintergrund anstoßen.
    fn ensure_audio_profiles(&self) {
        let need: std::collections::BTreeSet<String> = {
            let st = self.st.borrow();
            st.project.clips.iter().filter(|c| c.carries_audio() && c.audio.needs_profile() && !st.project.audio_profiles.contains_key(&c.path)).map(|c| c.path.clone()).collect()
        };
        for path in need {
            if !self.bg_pending.borrow_mut().insert(format!("a:{path}")) { continue; }
            let bg = self.bg.clone();
            std::thread::spawn(move || {
                let prof = engine::analyse_audio(&path);
                bg.lock().unwrap().push(BgResult::Audio(path, prof));
            });
        }
    }

    fn audio_measured(&self, path: String, prof: Option<crate::audiofx::Profile>) {
        self.bg_pending.borrow_mut().remove(&format!("a:{path}"));
        let ok = prof.is_some();
        let prof = prof.unwrap_or_default();
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let note = if !ok { format!("Der Ton von „{name}“ ließ sich nicht messen.") }
            else if prof.noise.is_empty() { format!("In „{name}“ ist kein Rauschen messbar.") }
            else if !prof.steady { format!("In „{name}“ gibt es kein gleichmäßiges Rauschen. Es wird nur vorsichtig entrauscht.") }
            else { format!("Ton gemessen: Grundrauschen bei {:.0} dB.", prof.floor) };
        // Die Messung gehört zur Datei, nicht zu einem Bearbeitungsschritt: auch in die Rückgängig-Stände eintragen.
        {
            let mut guard = self.st.borrow_mut();
            let st = &mut *guard;
            for p in st.undo.iter_mut().chain(st.redo.iter_mut()) { p.audio_profiles.insert(path.clone(), prof.clone()); }
        }
        self.modify(true, |p| { p.audio_profiles.insert(path, prof); });
        self.refresh_props();
        self.show_note(&note);
    }

    /// Kurzer Text zum Stand der Tonbearbeitung eines Clips (rechte Leiste).
    pub fn audio_status(&self, c: &Clip) -> String {
        let st = self.st.borrow();
        let parts = st.project.clips.iter().filter(|o| o.carries_audio() && o.path == c.path).count();
        let scope = if parts > 1 { format!(" Gilt für alle {parts} Teile dieser Aufnahme.") } else { String::new() };
        if !c.audio.active() { return "„Ton verbessern“ entfernt das Rauschen und gleicht die Lautstärke an.".to_string() + &scope; }
        match st.project.audio_profiles.get(&c.path) {
            None if c.audio.needs_profile() => "Der Ton wird gemessen …".to_string(),
            None => scope.trim().to_string(),
            Some(p) => {
                let noise = if p.noise.is_empty() { "kein Rauschen messbar".to_string() } else if !p.steady { format!("Grundrauschen {:.0} dB, nicht gleichmäßig, es wird nur vorsichtig entrauscht", p.floor) } else { format!("Grundrauschen {:.0} dB", p.floor) };
                format!("Gemessen: {noise}, Lautstärke {:.0} dB.{scope}", p.loud)
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Auswahl und Werkzeuge

    pub fn select(&self, id: Option<u64>, toggle: bool) {
        {
            let mut st = self.st.borrow_mut();
            match id {
                None => st.sel.clear(),
                Some(id) => {
                    if toggle {
                        if let Some(i) = st.sel.iter().position(|x| *x == id) { st.sel.remove(i); } else { st.sel.push(id); }
                    } else if !st.sel.contains(&id) || st.sel.len() > 1 {
                        st.sel = vec![id];
                    }
                }
            }
        }
        self.canvas.queue_draw();
        self.timeline.queue_draw();
        self.refresh_props();
    }

    pub fn selected_clip(&self) -> Option<Clip> {
        let st = self.st.borrow();
        st.sel.first().and_then(|id| st.project.clip(*id)).cloned()
    }

    pub fn set_tool(&self, tool: Tool) {
        self.st.borrow_mut().tool = tool;
        let btns = self.tool_btns.borrow();
        for (i, t) in [Tool::Select, Tool::Hand, Tool::Crop].iter().enumerate() {
            if btns[i].is_active() != (*t == tool) { btns[i].set_active(*t == tool); }
        }
        self.canvas.queue_draw();
    }

    // ---------------------------------------------------------------------------------------
    // Bearbeiten

    pub fn split(&self) {
        let (ids, t) = {
            let st = self.st.borrow();
            let t = st.playhead;
            let under = |c: &&Clip| c.start < t - 1e-6 && c.end() > t + 1e-6;
            let locked: Vec<u64> = st.project.tracks.iter().filter(|t| t.locked).map(|t| t.id).collect();
            let mut ids: Vec<u64> = st.project.clips.iter().filter(|c| st.sel.contains(&c.id)).filter(under).map(|c| c.id).collect();
            if ids.is_empty() { ids = st.project.clips.iter().filter(|c| !locked.contains(&c.track)).filter(under).map(|c| c.id).collect(); }
            (ids, t)
        };
        if ids.is_empty() { self.show_note("Am Abspielkopf liegt kein Clip, der geteilt werden kann."); return; }
        self.edit(|p| { for id in &ids { ops::split_clip(p, *id, t); } });
    }

    /// Schere ein- oder ausschalten.
    pub fn set_cut_tool(&self, on: bool) {
        let was = {
            let mut st = self.st.borrow_mut();
            let was = st.tl_cut;
            st.tl_cut = on;
            if !on { st.cut_hover = None; }
            was
        };
        {
            let btns = self.cut_btns.borrow();
            if let Some(b) = btns.first() { if b.is_active() != on { b.set_active(on); } }
            if let Some(b) = btns.get(1) { b.set_visible(on); }
        }
        self.timeline.set_cursor_from_name(Some(if on { "crosshair" } else { "default" }));
        self.timeline.queue_draw();
        if on && !was {
            self.show_note("Schere: Klick schneidet an der roten Linie. Ziehen markiert einen Bereich. „Lücke schließen“ nimmt ihn heraus und fügt zusammen, Entf nimmt ihn nur heraus. Esc beendet die Schere.");
        }
    }

    /// An der Zeit `t` schneiden. `tracks` leer = alle nicht gesperrten Spuren.
    pub fn cut_at(&self, t: f64, tracks: &[u64]) {
        let ids: Vec<u64> = {
            let st = self.st.borrow();
            let locked: Vec<u64> = st.project.tracks.iter().filter(|t| t.locked).map(|t| t.id).collect();
            st.project.clips.iter()
                .filter(|c| !locked.contains(&c.track) && (tracks.is_empty() || tracks.contains(&c.track)))
                .filter(|c| c.start < t - 1e-6 && c.end() > t + 1e-6)
                .map(|c| c.id).collect()
        };
        if ids.is_empty() { self.show_note("An dieser Stelle liegt kein Clip, der geschnitten werden kann."); return; }
        self.edit(|p| { for id in &ids { ops::split_clip(p, *id, t); } });
    }

    pub fn delete(&self, ripple: bool) {
        let (sel, range, only, cut) = { let st = self.st.borrow(); (st.sel.clone(), st.range, st.range_tracks.clone(), st.tl_cut) };
        if let Some((a, b)) = range {
            // Ein markierter Bereich geht vor: mit `ripple` herausnehmen und zusammenfügen, sonst nur herausnehmen.
            if ripple || sel.is_empty() || cut {
                self.edit(|p| ops::cut_range(p, a, b, &only, ripple));
                { let mut st = self.st.borrow_mut(); st.range = None; st.range_tracks.clear(); }
                if ripple { self.seek(a.min(b)); }
                self.timeline.queue_draw();
                return;
            }
        }
        if sel.is_empty() {
            if ripple { self.show_note("Erst einen Clip auswählen oder im Zeitlineal mit Umschalt + Ziehen einen Bereich markieren."); }
            return;
        }
        self.edit(|p| { ops::delete_clips(p, &sel, ripple); ops::prune_tracks(p); });
    }

    /// Ausgewählten Clip um eine halbe Sekunde in seinen Vorgänger schieben (weiche Überblendung).
    /// Schaltflächen der Kategorien links: wirken auf die ausgewählten Clips.
    pub fn quick_action(self: &Rc<Self>, action: &str) {
        if action == "pixelate" {
            // eigener Bereich, braucht keine Auswahl
            self.add_annotation(Kind::Blur, Shape::Rect, false);
            let id = self.st.borrow().sel.first().copied();
            self.edit(|p| { if let Some(c) = id.and_then(|id| p.clip_mut(id)) { c.pixelate = true; c.name = "Verpixeln".into(); } });
            self.show_note("Bereich angelegt. Zieh ihn in der Vorschau über die Stelle, die unkenntlich sein soll.");
            return;
        }
        // Tonbearbeitung geht auch ohne Auswahl: dann für allen Ton im Projekt
        if action.starts_with("a_") { self.audio_action(action); return; }
        if self.st.borrow().sel.is_empty() {
            self.show_note("Bitte zuerst einen Clip in der Vorschau oder in der Zeitleiste auswählen.");
            return;
        }
        match action {
            "xfade" => { self.crossfade_selected(); return; }
            "t_black" => { self.transition_selected(FadeKind::Black, 0.4); return; }
            "t_white" => { self.transition_selected(FadeKind::White, 0.4); return; }
            "t_flash" => { self.transition_selected(FadeKind::White, 0.15); return; }
            "t_zoom" => { self.transition_selected(FadeKind::Zoom, 0.4); return; }
            "t_whipl" => { self.transition_selected(FadeKind::WhipLeft, 0.35); return; }
            "t_whipr" => { self.transition_selected(FadeKind::WhipRight, 0.35); return; }
            "t_blur" => { self.transition_selected(FadeKind::Blur, 0.5); return; }
            "t_glitch" => { self.transition_selected(FadeKind::Glitch, 0.3); return; }
            "bgimage" => {
                let ok = { let st = self.st.borrow(); st.sel.iter().any(|id| st.project.clip(*id).map(|c| c.kind.is_media() && c.is_visual()).unwrap_or(false)) };
                if !ok { self.show_note("Wähle zuerst den Kamera-Clip aus."); return; }
                let p = self.props.borrow().clone();
                if let Some(p) = p { p.choose_bg(); }
                return;
            }
            "zoomin" | "zoomout" | "animcustom" => {
                let p = self.props.borrow().clone();
                if let Some(p) = p { p.add_anim(self, match action { "zoomin" => 0, "zoomout" => 1, _ => 2 }); }
                return;
            }
            _ => {}
        }
        let ids = self.st.borrow().sel.clone();
        let mut hit = false;
        let action = action.to_string();
        self.edit(|p| {
            let ph = p.height as f64;
            for id in &ids {
                let Some(c) = p.clip_mut(*id) else { continue };
                let media = c.kind.is_media() && c.is_visual();
                let d = c.duration;
                let mut ok = true;
                match action.as_str() {
                    "fadein" => { c.fade_in = 0.5f64.min(d / 2.0); c.fade_in_kind = FadeKind::Blend; }
                    "fadeout" => { c.fade_out = 0.5f64.min(d / 2.0); c.fade_out_kind = FadeKind::Blend; }
                    "fadeboth" => { c.fade_in = 0.5f64.min(d / 2.0); c.fade_out = 0.5f64.min(d / 2.0); c.fade_in_kind = FadeKind::Blend; c.fade_out_kind = FadeKind::Blend; }
                    "rollin" if c.is_visual() => { c.fade_in = 0.6f64.min(d / 2.0); c.fade_in_kind = FadeKind::Roll; }
                    "rollout" if c.is_visual() => { c.fade_out = 0.6f64.min(d / 2.0); c.fade_out_kind = FadeKind::Roll; }
                    "rollboth" if c.is_visual() => { c.fade_in = 0.6f64.min(d / 2.0); c.fade_out = 0.6f64.min(d / 2.0); c.fade_in_kind = FadeKind::Roll; c.fade_out_kind = FadeKind::Roll; }
                    "fadenone" => { c.fade_in = 0.0; c.fade_out = 0.0; c.fade_in_kind = FadeKind::Blend; c.fade_out_kind = FadeKind::Blend; }
                    "animnone" => {
                        c.anims.clear();
                        if c.fade_in_kind.is_motion() { c.fade_in = 0.0; c.fade_in_kind = FadeKind::Blend; }
                        if c.fade_out_kind.is_motion() { c.fade_out = 0.0; c.fade_out_kind = FadeKind::Blend; }
                    }
                    a if (a.starts_with("slidein_") || a == "popin") && c.is_visual() => {
                        c.fade_in = 0.5f64.min(d / 2.0);
                        c.fade_in_kind = match a { "slidein_l" => FadeKind::SlideLeft, "slidein_r" => FadeKind::SlideRight, "slidein_u" => FadeKind::SlideUp, "slidein_d" => FadeKind::SlideDown, _ => FadeKind::Pop };
                    }
                    a if (a.starts_with("slideout_") || a == "popout") && c.is_visual() => {
                        c.fade_out = (if a == "popout" { 0.4f64 } else { 0.5 }).min(d / 2.0);
                        c.fade_out_kind = match a { "slideout_l" => FadeKind::SlideLeft, "slideout_r" => FadeKind::SlideRight, "slideout_u" => FadeKind::SlideUp, "slideout_d" => FadeKind::SlideDown, _ => FadeKind::Pop };
                    }
                    "c_bright" if c.is_visual() => c.brightness = (c.brightness + 0.15).min(1.0),
                    "c_dark" if c.is_visual() => c.brightness = (c.brightness - 0.15).max(-1.0),
                    "c_contrast" if c.is_visual() => c.contrast = (c.contrast + 0.15).min(1.0),
                    "c_sat" if c.is_visual() => c.saturation = (c.saturation.max(0.0) + 0.2).min(1.0),
                    "c_bw" if c.is_visual() => c.saturation = if c.saturation <= -0.99 { 0.0 } else { -1.0 },
                    "c_warm" if c.is_visual() => c.temperature = (c.temperature + 0.2).min(1.0),
                    "c_cool" if c.is_visual() => c.temperature = (c.temperature - 0.2).max(-1.0),
                    "c_reset" if c.is_visual() => { c.brightness = 0.0; c.contrast = 0.0; c.saturation = 0.0; c.temperature = 0.0; }
                    "fliph" if c.is_visual() => c.flip_h = !c.flip_h,
                    "flipv" if c.is_visual() => c.flip_v = !c.flip_v,
                    "vignette" if c.is_visual() => c.vignette = if c.vignette > 0.01 { 0.0 } else { 0.6 },
                    "circle" if media => {
                        let side = c.w.min(c.h);
                        let (sw, sh) = (c.src_w.max(1) as f64, c.src_h.max(1) as f64);
                        c.crop = if sw > sh { let k = (1.0 - sh / sw) / 2.0; [k, 0.0, k, 0.0] } else { let k = (1.0 - sw / sh) / 2.0; [0.0, k, 0.0, k] };
                        c.x += (c.w - side) / 2.0;
                        c.y += (c.h - side) / 2.0;
                        c.w = side;
                        c.h = side;
                        c.shape = Shape::Ellipse;
                    }
                    "rounded" if media => c.shape = Shape::Rounded,
                    "rectshape" if media => c.shape = Shape::Rect,
                    "border" if media => c.border = if c.border > 0.0 { 0.0 } else { (ph * 0.005).round().max(3.0) },
                    "shadow" if c.is_visual() => c.shadow = !c.shadow,
                    "chroma" if media => c.chroma = !c.chroma,
                    "bgremove" if media => c.bg_mode = if c.bg_mode == BgMode::Transparent { BgMode::Off } else { BgMode::Transparent },
                    "bgblur" if media => c.bg_mode = if c.bg_mode == BgMode::Blur { BgMode::Off } else { BgMode::Blur },
                    "opacity" if c.is_visual() => c.opacity = if c.opacity < 0.99 { 1.0 } else { 0.5 },
                    "mute" if c.carries_audio() => c.muted = !c.muted,
                    "louder" if c.carries_audio() => c.volume = (c.volume + 0.1).min(2.0),
                    "quieter" if c.carries_audio() => c.volume = (c.volume - 0.1).max(0.0),
                    "afadein" if c.carries_audio() => c.fade_in = 1.0f64.min(d / 2.0),
                    "afadeout" if c.carries_audio() => c.fade_out = 1.0f64.min(d / 2.0),
                    a if a.starts_with("cur") => match c.cursor.as_mut() {
                        Some(k) => match a {
                            "curshow" => k.show = !k.show,
                            "curhl" => k.highlight = !k.highlight,
                            "curclicks" => k.clicks = !k.clicks,
                            "curbig" => k.scale = (k.scale * 1.25).min(5.0),
                            "cursmall" => k.scale = (k.scale / 1.25).max(0.5),
                            "cursmooth" => k.smooth = if k.smooth > 0.0 { 0.0 } else { 0.5 },
                            _ => ok = false,
                        },
                        None => ok = false,
                    },
                    _ => ok = false,
                }
                hit |= ok;
            }
        });
        if !hit { self.show_note("Das passt nicht zum ausgewählten Clip. Wähle zum Beispiel die Kamera, die Bildschirmaufnahme oder einen Clip mit Ton."); }
    }

    /// Übergang zwischen zwei Clips: am Anfang des ausgewählten Clips und am Ende des Clips direkt davor.
    /// Liegt davor nichts, wirkt er nur am Anfang.
    pub fn transition_selected(&self, kind: FadeKind, len: f64) {
        let ids = self.st.borrow().sel.clone();
        let (mut done, mut pairs) = (0, 0);
        self.edit(|p| {
            for id in &ids {
                let Some(c) = p.clip(*id).cloned() else { continue };
                if !c.is_visual() || c.kind == Kind::Blur { continue; }
                let prev = p.clips.iter().filter(|o| o.id != c.id && o.track == c.track && o.is_visual() && (o.end() - c.start).abs() < 0.05).map(|o| o.id).next();
                if let Some(m) = p.clip_mut(c.id) { m.fade_in = len.min(m.duration / 2.0); m.fade_in_kind = kind; done += 1; }
                if let Some(m) = prev.and_then(|pid| p.clip_mut(pid)) { m.fade_out = len.min(m.duration / 2.0); m.fade_out_kind = kind; pairs += 1; }
            }
        });
        if done == 0 { self.show_note("Das passt nicht zum ausgewählten Clip. Wähle einen Video-, Bild- oder Text-Clip."); }
        else if pairs == 0 { self.show_note("Davor liegt kein Clip direkt an. Der Übergang wirkt deshalb nur am Anfang dieses Clips."); }
    }

    pub fn crossfade_selected(&self) {
        let Some(c) = self.selected_clip() else { self.show_note("Erst den Clip auswählen, der weich aus dem vorherigen entstehen soll."); return };
        let prev = {
            let st = self.st.borrow();
            st.project.clips_on(c.track).into_iter().filter(|o| o.id != c.id && o.start < c.start).last().cloned()
        };
        let Some(prev) = prev else { self.show_note("Davor liegt auf dieser Spur kein Clip. Zum Einblenden rechts „Einblenden“ einstellen."); return };
        let gap = c.start - prev.end();
        if gap > 0.05 { self.show_note("Die Clips müssen direkt aneinander liegen. Schiebe den Clip an den vorherigen heran."); return; }
        let ov = 0.5f64.min(prev.duration / 2.0).min(c.duration / 2.0);
        let shift = ov + gap;
        let mut ok = true;
        self.edit(|p| {
            let later: Vec<u64> = p.clips.iter().filter(|o| o.track == c.track && o.start >= c.start - 1e-6).map(|o| o.id).collect();
            for id in later { if let Some(o) = p.clip_mut(id) { o.start = (o.start - shift).max(0.0); } }
            if let Some(n) = p.clip(c.id) { ok = p.placement_ok(n); }
        });
        if !ok { self.undo(); self.show_note("Hier ist keine Überblendung möglich."); }
    }

    pub fn add_annotation(&self, kind: Kind, shape: Shape, line: bool) {
        let t = self.st.borrow().playhead;
        let mut new_id = 0;
        self.edit(|p| {
            new_id = ops::add_annotation(p, kind, if kind == Kind::Arrow { Shape::Rect } else { shape }, t);
            if line { if let Some(c) = p.clip_mut(new_id) { c.arrow_head = false; c.name = "Linie".into(); } }
        });
        self.select(Some(new_id), false);
        self.set_tool(Tool::Select);
        if self.st.borrow().project.duration() > 0.0 { let ph = self.st.borrow().playhead; self.seek(ph); }
    }

    /// Dateien importieren; mit `place` kommen sie zusätzlich am Abspielkopf auf die Zeitleiste.
    /// Ordner des Projekts: Hier liegen die Projektdatei, die Aufnahmen und unter `medien/` alles Importierte.
    pub fn project_dir(&self) -> PathBuf {
        self.st.borrow().path.parent().map(|d| d.to_path_buf()).unwrap_or_default()
    }

    /// Datei in den Projektordner übernehmen (nach `medien/`), falls sie nicht schon dort liegt.
    /// Liefert den Pfad, mit dem das Projekt weiterarbeitet.
    pub fn adopt(&self, path: &str) -> String {
        let dir = self.project_dir();
        let src = std::path::Path::new(path);
        if dir.as_os_str().is_empty() || !src.is_file() { return path.to_string(); }
        let inside = std::fs::canonicalize(src).ok().zip(std::fs::canonicalize(&dir).ok()).map(|(s, d)| s.starts_with(d)).unwrap_or(false);
        if inside || src.starts_with(&dir) { return path.to_string(); }
        match adopt_file(src, &dir.join(MEDIA_DIR)) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(e) => {
                eprintln!("ws-video: {path} konnte nicht in den Projektordner übernommen werden: {e}");
                path.to_string()
            }
        }
    }

    pub fn import_files(&self, paths: &[String], place: bool) -> Vec<usize> {
        let mut items = Vec::new();
        let mut errors = Vec::new();
        for path in paths {
            if path.ends_with(&format!(".{FILE_EXT}")) { continue; }
            // Importierte Dateien kommen mit in den Projektordner.
            let path = &self.adopt(path);
            match ops::media_item(path) {
                Ok(m) => items.push(m),
                Err(e) => errors.push(format!("{}: {e}", std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())),
            }
        }
        let mut idx = Vec::new();
        if !items.is_empty() {
            let t = self.st.borrow().playhead;
            let empty = self.st.borrow().project.clips.is_empty();
            self.edit(|p| {
                let mut at = t;
                for m in items {
                    // das erste Video bestimmt bei leerem Projekt die Projektgröße
                    if empty && p.clips.is_empty() && m.kind == Kind::Video && m.width >= 320 && m.height >= 240 {
                        p.width = m.width / 2 * 2;
                        p.height = m.height / 2 * 2;
                    }
                    let i = ops::add_media(p, m.clone());
                    idx.push(i);
                    if place || empty {
                        let id = ops::clip_from_media(p, &m, at, None);
                        if let Some(c) = p.clip(id) { at = c.end(); }
                    }
                }
            });
            if empty { let w = Rc::new(()); let _ = w; }
        }
        if !errors.is_empty() { self.show_note(&format!("Nicht importiert:\n{}", errors.join("\n"))); }
        idx
    }

    /// Medium aus der Medienliste an einer Zeit (und optional Spur) einfügen.
    pub fn place_media(&self, index: usize, t: f64, track: Option<u64>, pos: Option<(f64, f64)>) {
        let Some(m) = self.st.borrow().project.media.get(index).cloned() else { return };
        let mut new_id = 0;
        self.edit(|p| {
            new_id = ops::clip_from_media(p, &m, t, track);
            if let (Some((x, y)), Some(c)) = (pos, p.clip_mut(new_id)) {
                if c.is_visual() { c.x = (x - c.w / 2.0).round(); c.y = (y - c.h / 2.0).round(); }
            }
        });
        self.select(Some(new_id), false);
    }

    pub fn copy(&self, cut: bool) {
        let clips: Vec<Clip> = { let st = self.st.borrow(); st.project.clips.iter().filter(|c| st.sel.contains(&c.id)).cloned().collect() };
        if clips.is_empty() { return; }
        self.st.borrow_mut().clipboard = clips;
        if cut { self.delete(false); }
    }
    pub fn paste(&self) {
        let (clips, t) = { let st = self.st.borrow(); (st.clipboard.clone(), st.playhead) };
        if clips.is_empty() { return; }
        let first = clips.iter().map(|c| c.start).fold(f64::INFINITY, f64::min);
        let mut ids = Vec::new();
        self.edit(|p| {
            for mut c in clips {
                c.id = p.new_id();
                c.start = ops::snap_frame(t + (c.start - first), p.fps);
                let pref = if p.track(c.track).is_some() { Some(c.track) } else { None };
                c.track = p.free_track(c.start, c.duration, pref);
                ids.push(c.id);
                p.clips.push(c);
            }
        });
        self.st.borrow_mut().sel = ids;
        self.changed();
    }

    fn refresh_media(&self) {
        let sig = {
            let st = self.st.borrow();
            let thumbs = self.thumbs.borrow();
            st.project.media.iter().map(|m| format!("{}{}", m.path, thumbs.contains_key(&m.path))).collect::<Vec<_>>().join("|")
        };
        if *self.media_sig.borrow() == sig && self.media_box.first_child().is_some() { return; }
        *self.media_sig.borrow_mut() = sig;
        while let Some(c) = self.media_box.first_child() { self.media_box.remove(&c); }
        let media = self.st.borrow().project.media.clone();
        if media.is_empty() {
            let l = label("Noch keine Medien. Über „Importieren“ oder per Ziehen hinzufügen.", "ws-dim ws-small");
            l.set_wrap(true);
            l.set_margin_top(10);
            self.media_box.append(&l);
            return;
        }
        let Some(me) = self.app.upgrade().and_then(|a| a.editor.borrow().clone()) else {
            // der Editor wird gerade erst aufgebaut: gleich noch einmal
            *self.media_sig.borrow_mut() = String::new();
            return;
        };
        for (i, m) in media.iter().enumerate() {
            let row = hbox(8);
            row.add_css_class("ws-card");
            let thumb = gtk::DrawingArea::new();
            thumb.set_content_width(64);
            thumb.set_content_height(40);
            if m.has_video { self.request_thumb(&m.path); }
            let surf = self.thumbs.borrow().get(&m.path).cloned().flatten();
            let icon: &'static str = match m.kind { Kind::Audio => "note", Kind::Image => "image", _ => "film" };
            thumb.set_draw_func(move |w, cr, width, height| {
                super::rounded_rect(cr, 0.0, 0.0, width as f64, height as f64, 5.0);
                super::theme::set(cr, "feld");
                let _ = cr.fill_preserve();
                cr.clip();
                match &surf {
                    Some(s) => super::draw_surface(cr, s, 0.0, 0.0, width as f64, height as f64, true),
                    None => {
                        let c = w.color();
                        cr.translate((width as f64 - 20.0) / 2.0, (height as f64 - 20.0) / 2.0);
                        icons::draw(cr, icon, 20.0, (c.red() as f64, c.green() as f64, c.blue() as f64, 0.6));
                    }
                }
            });
            row.append(&thumb);
            let v = vbox(2);
            let n = label(&m.name, "ws-small");
            n.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            n.set_max_width_chars(16);
            v.append(&n);
            let info = match m.kind {
                Kind::Audio => format!("Ton · {}", format_dur(m.duration)),
                Kind::Image => format!("Bild · {} × {}", m.width, m.height),
                _ => format!("{} × {} · {}", m.width, m.height, format_dur(m.duration)),
            };
            v.append(&label(&info, "ws-dim ws-small"));
            v.set_hexpand(true);
            row.append(&v);
            let add = super::icon_button("plus", "", "Am Abspielkopf einfügen");
            add.add_css_class("ws-flat");
            add.set_valign(gtk::Align::Center);
            row.append(&add);
            {
                let weak = Rc::downgrade(&me);
                add.connect_clicked(move |_| { if let Some(e) = weak.upgrade() { let t = e.st.borrow().playhead; e.place_media(i, t, None, None); } });
            }
            {
                let click = gtk::GestureClick::new();
                let weak = Rc::downgrade(&me);
                click.connect_pressed(move |_, n, _, _| {
                    if n == 2 { if let Some(e) = weak.upgrade() { let t = e.st.borrow().playhead; e.place_media(i, t, None, None); } }
                });
                row.add_controller(click);
            }
            {
                let drag = gtk::DragSource::new();
                drag.set_actions(gdk::DragAction::COPY);
                drag.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&format!("wsmedia:{i}").to_value())));
                row.add_controller(drag);
            }
            self.media_box.append(&row);
        }
    }

    fn key_pressed(&self, key: gdk::Key, state: gdk::ModifierType) -> bool {
        // in Eingabefeldern gelten die normalen Tasten
        if std::env::var_os("WS_DEBUG").is_some() { eprintln!("Taste {:?}, Fokus {:?}", key.name(), self.window().and_then(|w| gtk::prelude::GtkWindowExt::focus(&w)).map(|w| w.type_().name().to_string())); }
        if let Some(w) = self.window().and_then(|w| gtk::prelude::GtkWindowExt::focus(&w)) {
            if w.is::<gtk::Text>() || w.is::<gtk::TextView>() || w.is::<gtk::Entry>() || w.is::<gtk::SpinButton>() { return false; }
        }
        let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let name = key.name().map(|s| s.to_string()).unwrap_or_default();
        match (ctrl, name.as_str()) {
            (false, "space") => self.toggle_play(),
            (false, "s") | (false, "S") => self.split(),
            (false, "x") | (false, "X") => { let on = !self.st.borrow().tl_cut; self.set_cut_tool(on); }
            (false, "Delete") | (false, "BackSpace") => self.delete(shift),
            (false, "Home") => self.seek(0.0),
            (false, "End") => { let d = self.st.borrow().project.duration(); self.seek(d); }
            (false, "v") | (false, "V") => self.set_tool(Tool::Select),
            (false, "h") | (false, "H") => self.set_tool(Tool::Hand),
            (false, "c") | (false, "C") => self.set_tool(Tool::Crop),
            (false, "i") | (false, "I") => {
                let mut st = self.st.borrow_mut();
                let ph = st.playhead;
                let end = st.range.map(|r| r.1).filter(|e| *e > ph).unwrap_or(st.project.duration());
                st.range = Some((ph, end));
                st.range_tracks.clear();
                drop(st);
                self.timeline.queue_draw();
            }
            (false, "o") | (false, "O") => {
                let mut st = self.st.borrow_mut();
                let ph = st.playhead;
                let start = st.range.map(|r| r.0).filter(|s| *s < ph).unwrap_or(0.0);
                st.range = Some((start, ph));
                st.range_tracks.clear();
                drop(st);
                self.timeline.queue_draw();
            }
            (false, "Escape") => {
                { let mut st = self.st.borrow_mut(); st.range = None; st.range_tracks.clear(); }
                if self.st.borrow().tl_cut { self.set_cut_tool(false); }
                self.select(None, false);
            }
            (false, "Left") | (false, "Right") | (false, "Up") | (false, "Down") => {
                // Pfeiltasten: ohne Auswahl bildweise springen, mit Auswahl den Clip verschieben
                let sel = self.selected_clip().filter(|c| c.is_visual());
                let canvas_focus = self.canvas.has_focus();
                match (sel, canvas_focus) {
                    (Some(c), true) => {
                        let d = if shift { 10.0 } else { 1.0 };
                        let (dx, dy) = match name.as_str() { "Left" => (-d, 0.0), "Right" => (d, 0.0), "Up" => (0.0, -d), _ => (0.0, d) };
                        let st_time = c.source_time(self.st.borrow().playhead);
                        self.edit_coalesced("nudge", |p| { if let Some(k) = p.clip_mut(c.id) { canvas::offset_rect(k, st_time, dx, dy); } });
                    }
                    _ => match name.as_str() { "Left" => self.step(if shift { -10 } else { -1 }), "Right" => self.step(if shift { 10 } else { 1 }), _ => return false },
                }
            }
            (true, "z") => self.undo(),
            (true, "Z") | (true, "y") => self.redo(),
            (true, "s") => { self.save(); self.show_note("Projekt gespeichert."); }
            (true, "c") => self.copy(false),
            (true, "x") => self.copy(true),
            (true, "v") => self.paste(),
            (true, "d") => {
                if let Some(c) = self.selected_clip() {
                    let mut nid = None;
                    self.edit(|p| nid = ops::duplicate_clip(p, c.id));
                    self.select(nid, false);
                }
            }
            (true, "a") => {
                let ids: Vec<u64> = self.st.borrow().project.clips.iter().map(|c| c.id).collect();
                self.st.borrow_mut().sel = ids;
                self.canvas.queue_draw();
                self.timeline.queue_draw();
                self.refresh_props();
            }
            (true, "e") => { if let Some(me) = self.app.upgrade().and_then(|a| a.editor.borrow().clone()) { export::open(&me); } }
            _ => return false,
        }
        true
    }

    /// Projekt unter neuem Namen sichern: Es entsteht ein neuer Projektordner, und alles, was im bisherigen
    /// Ordner zum Projekt gehört, kommt mit (große Dateien nach Möglichkeit als Verknüpfung, also ohne
    /// zweiten Platzverbrauch).
    pub fn save_as_dialog(self: &Rc<Self>) {
        let Some(win) = self.window() else { return };
        let start = self.project_dir().parent().map(|d| d.to_path_buf()).unwrap_or_else(crate::settings::videos_dir);
        let d = gtk::FileDialog::builder().title("Projekt speichern unter (es wird ein Ordner mit diesem Namen angelegt)").initial_name("Mein Projekt").initial_folder(&gio::File::for_path(start)).build();
        let me = self.clone();
        d.save(Some(&win), gio::Cancellable::NONE, move |res| {
            let Ok(f) = res else { return };
            let Some(chosen) = f.path() else { return };
            if let Err(e) = me.save_as(&chosen) {
                if let Some(w) = me.window() { super::message(&w, "Das Projekt konnte nicht gespeichert werden", &format!("{e}")); }
            }
        });
    }

    pub fn save_as(&self, chosen: &std::path::Path) -> anyhow::Result<PathBuf> {
        let chosen = if chosen.extension().map(|e| e == FILE_EXT).unwrap_or(false) { chosen.with_extension("") } else { chosen.to_path_buf() };
        let name = chosen.file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).ok_or_else(|| anyhow::anyhow!("Kein Name angegeben"))?;
        let new_dir = chosen.clone();
        let old_dir = self.project_dir();
        std::fs::create_dir_all(&new_dir)?;
        let file = new_dir.join(format!("{name}.{FILE_EXT}"));
        if new_dir != old_dir {
            let errors = std::cell::RefCell::new(Vec::new());
            let mut st = self.st.borrow_mut();
            st.project.map_paths(&|p| {
                let src = std::path::Path::new(p);
                let Ok(rel) = src.strip_prefix(&old_dir) else { return p.to_string() };
                if !src.is_file() { return p.to_string(); }
                let target = new_dir.join(rel);
                let tdir = target.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| new_dir.clone());
                match adopt_file(src, &tdir) {
                    Ok(t) => t.to_string_lossy().to_string(),
                    Err(e) => { errors.borrow_mut().push(format!("{}: {e}", src.display())); p.to_string() }
                }
            });
            let errors = errors.into_inner();
            if !errors.is_empty() { anyhow::bail!("Nicht alle Dateien ließen sich übernehmen:\n{}", errors.join("\n")); }
        }
        self.st.borrow_mut().path = file.clone();
        self.save();
        *self.media_sig.borrow_mut() = String::new();
        self.sync_engine();
        self.refresh_media();
        if let Some(app) = self.app.upgrade() {
            app.settings.borrow_mut().add_recent(&file.to_string_lossy());
            app.window.set_title(Some(&format!("{name} – WS.Video")));
        }
        self.show_note(&format!("Projekt gespeichert in {}", new_dir.display()));
        Ok(file)
    }
}
