//! Startseite: neue Aufnahme, Projekte, Kurzanleitung.
use super::{hbox, icons, label, vbox, App};
use gtk::prelude::*;
use std::rc::Rc;

pub struct StartPage {
    pub root: gtk::Box,
    recent: gtk::Box,
}

fn tile(icon: &'static str, title: &str, sub: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("ws-tile");
    let v = vbox(8);
    v.set_size_request(170, -1);
    let ic = icons::widget(icon, 34);
    ic.set_halign(gtk::Align::Start);
    v.append(&ic);
    v.append(&label(title, "ws-h"));
    let s = label(sub, "ws-dim ws-small");
    s.set_wrap(true);
    s.set_max_width_chars(22);
    v.append(&s);
    b.set_child(Some(&v));
    b
}

impl StartPage {
    pub fn new(app: &Rc<App>) -> StartPage {
        let root = hbox(0);
        root.add_css_class("ws-root");

        let side = vbox(10);
        side.add_css_class("ws-side");
        side.set_size_request(250, -1);
        side.set_margin_top(0);
        let logo = hbox(8);
        logo.set_margin_top(22);
        logo.set_margin_start(20);
        logo.append(&icons::widget("film", 20));
        logo.append(&label("WS.Video", "ws-logo"));
        side.append(&logo);
        let help = label(
            "So geht es\n\n1. Neue Aufnahme: Quellen einschalten und aufnehmen. Bildschirm, Kamera, Mikrofon und Systemton landen auf getrennten Spuren.\n\n2. Im Editor: Kamera verschieben und formen, schneiden, Text und Pfeile einfügen, zoomen.\n\n3. Exportieren: MP4 in HD bis 4K.",
            "ws-dim ws-small",
        );
        help.set_wrap(true);
        help.set_margin_start(20);
        help.set_margin_end(16);
        help.set_margin_top(18);
        help.set_max_width_chars(28);
        side.append(&help);
        let spacer = vbox(0);
        spacer.set_vexpand(true);
        side.append(&spacer);
        let ver = label(&format!("Version {}", env!("CARGO_PKG_VERSION")), "ws-dim ws-small");
        ver.set_margin_start(20);
        ver.set_margin_bottom(16);
        side.append(&ver);
        root.append(&side);

        let content = vbox(18);
        content.set_hexpand(true);
        content.set_margin_top(34);
        content.set_margin_start(40);
        content.set_margin_end(40);
        content.set_margin_bottom(30);
        content.append(&label("Was möchtest du tun?", "ws-title"));
        let tiles = hbox(14);
        let t_rec = tile("record", "Neue Aufnahme", "Bildschirm, Kamera und Ton aufnehmen");
        let t_new = tile("film", "Neues Projekt", "Leeres Projekt im Editor öffnen");
        let t_open = tile("folder", "Projekt öffnen", "Gespeichertes Projekt laden");
        let t_imp = tile("import", "Medien importieren", "Videos, Bilder oder Ton in ein Projekt holen");
        for t in [&t_rec, &t_new, &t_open, &t_imp] { tiles.append(t); }
        content.append(&tiles);
        {
            let a = app.clone();
            t_rec.connect_clicked(move |_| a.open_recorder());
            let a = app.clone();
            t_new.connect_clicked(move |_| a.new_project());
            let a = app.clone();
            t_open.connect_clicked(move |_| a.open_project_dialog());
            let a = app.clone();
            t_imp.connect_clicked(move |_| a.import_dialog());
        }
        content.append(&label("Zuletzt geöffnet", "ws-h"));
        let recent = vbox(4);
        let scroll = gtk::ScrolledWindow::builder().child(&recent).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        content.append(&scroll);
        root.append(&content);
        let sp = StartPage { root, recent };
        sp.refresh(app);
        sp
    }

    pub fn refresh(&self, app: &Rc<App>) {
        while let Some(c) = self.recent.first_child() { self.recent.remove(&c); }
        let list = app.settings.borrow().recent_existing();
        if list.is_empty() {
            self.recent.append(&label("Noch keine Projekte. Starte mit einer neuen Aufnahme.", "ws-dim"));
            return;
        }
        for path in list {
            let b = gtk::Button::new();
            b.add_css_class("ws-flat");
            let h = hbox(10);
            h.append(&icons::widget("film", 18));
            let p = std::path::Path::new(&path);
            let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let n = label(&name, "");
            h.append(&n);
            let dir = label(&p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default(), "ws-dim ws-small");
            dir.set_hexpand(true);
            dir.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            h.append(&dir);
            b.set_child(Some(&h));
            let a = app.clone();
            let pp = path.clone();
            b.connect_clicked(move |_| a.open_project(std::path::Path::new(&pp)));
            self.recent.append(&b);
        }
    }
}
