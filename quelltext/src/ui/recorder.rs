//! Aufnahme-Fenster mit Quellen-Karten und die kleine Steuerleiste während der Aufnahme.
use super::{hbox, icons, label, vbox, App};
use crate::engine::{self, Frame};
use crate::recorder::devices::{self, Dev};
use crate::recorder::portal::ScreenSource;
use crate::recorder::session::{self, RecOptions, Recording, ScreenInput};
use crate::recorder::hypr;
use gst::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

/// Schlüssel der Auswahl „Größe“ in den Einstellungen, in der Reihenfolge der Liste
const SIZES: [&str; 4] = ["auto", "original", "1440", "1080"];

fn fake() -> bool { std::env::var_os("WS_FAKE").is_some() }

/// Kleine Vorschau-Pipeline (Kamerabild, Bildschirm-Vorschau oder Pegel).
struct MiniPipe {
    pipeline: gst::Element,
    frame: Arc<Mutex<Option<Frame>>>,
    level: f32,
    failed: bool,
    _fd: Option<std::os::fd::OwnedFd>,
}

/// Bild auf höchstens `max_h` Zeilen verkleinern (für die Vorschau, einfache Abtastung mit 2x2-Mittel).
fn shrink(f: Frame, max_h: i32) -> Frame {
    if f.h <= max_h || f.w < 2 || f.h < 2 { return f; }
    let k = f.h as f64 / max_h as f64;
    let (nw, nh) = (((f.w as f64 / k).round() as i32).max(1), max_h);
    let stride = nw * 4;
    let mut data = vec![255u8; (stride * nh) as usize];
    let half = ((k / 2.0) as i32).max(1);
    for y in 0..nh {
        let sy = ((y as f64 + 0.25) * k) as i32;
        let sy2 = (sy + half).min(f.h - 1);
        for x in 0..nw {
            let sx = ((x as f64 + 0.25) * k) as i32;
            let sx2 = (sx + half).min(f.w - 1);
            let o = (y * stride + x * 4) as usize;
            for c in 0..3usize {
                let g = |yy: i32, xx: i32| f.data[(yy.min(f.h - 1) * f.stride + xx.min(f.w - 1) * 4) as usize + c] as u32;
                data[o + c] = ((g(sy, sx) + g(sy, sx2) + g(sy2, sx) + g(sy2, sx2)) / 4) as u8;
            }
        }
    }
    Frame { w: nw, h: nh, stride, data }
}

impl MiniPipe {
    fn video(src: &str, fd: Option<std::os::fd::OwnedFd>) -> Option<MiniPipe> {
        // Bildschirmquellen (mit fd) werden in voller Größe abgeholt und erst hier verkleinert:
        // Das Skalieren in der Pipeline lieferte unter Hyprland mit manchen Monitoren nur einen schmalen Streifen.
        let native = fd.is_some();
        let desc = if native {
            format!("{src} ! videorate drop-only=true max-rate=8 ! videoconvert ! video/x-raw,format=BGRx ! appsink name=sink drop=true max-buffers=1 sync=false")
        } else {
            format!("{src} ! videoconvert ! videoscale ! video/x-raw,format=BGRx,height=200,pixel-aspect-ratio=1/1 ! appsink name=sink drop=true max-buffers=1 sync=false")
        };
        let pipeline = gst::parse::launch(&desc).ok()?;
        let frame: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
        let sink = pipeline.clone().downcast::<gst::Bin>().ok()?.by_name("sink")?.downcast::<gst_app::AppSink>().ok()?;
        let slot = frame.clone();
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |s| {
                    if let Ok(sample) = s.pull_sample() {
                        if let Some(f) = engine::sample_to_frame(&sample) { *slot.lock().unwrap() = Some(if native { shrink(f, 240) } else { f }); }
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        let failed = pipeline.set_state(gst::State::Playing).is_err();
        Some(MiniPipe { pipeline, frame, level: 0.0, failed, _fd: fd })
    }
    fn audio(src: &str) -> Option<MiniPipe> {
        let desc = format!("{src} ! audioconvert ! level interval=60000000 post-messages=true ! fakesink sync=false");
        let pipeline = gst::parse::launch(&desc).ok()?;
        let failed = pipeline.set_state(gst::State::Playing).is_err();
        Some(MiniPipe { pipeline, frame: Arc::new(Mutex::new(None)), level: 0.0, failed, _fd: None })
    }
    fn poll(&mut self) {
        let Some(bus) = self.pipeline.bus() else { return };
        while let Some(msg) = bus.pop() {
            match msg.view() {
                gst::MessageView::Error(_) => self.failed = true,
                gst::MessageView::Element(el) => {
                    if let Some(s) = el.structure() {
                        if s.name() == "level" {
                            if let Ok(arr) = s.get::<glib::ValueArray>("peak") {
                                let mut peak: f64 = -100.0;
                                for v in arr.iter() { if let Ok(db) = v.get::<f64>() { peak = peak.max(db); } }
                                self.level = (10f64.powf(peak / 20.0)).clamp(0.0, 1.0) as f32;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
impl Drop for MiniPipe {
    fn drop(&mut self) { let _ = self.pipeline.set_state(gst::State::Null); }
}

#[derive(Default)]
struct Previews {
    screen: Option<MiniPipe>,
    camera: Option<MiniPipe>,
    mic: Option<MiniPipe>,
    system: Option<MiniPipe>,
}

struct Card {
    frame: gtk::Box,
    switch: gtk::Switch,
    body: gtk::Box,
}

fn card(icon: &'static str, title: &str) -> Card {
    let frame = vbox(8);
    frame.add_css_class("ws-card");
    frame.set_hexpand(true);
    let head = hbox(8);
    head.append(&icons::widget(icon, 20));
    let l = label(title, "ws-h");
    l.set_hexpand(true);
    head.append(&l);
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    head.append(&switch);
    frame.append(&head);
    let body = vbox(8);
    frame.append(&body);
    Card { frame, switch, body }
}

fn preview_area(slot: &Rc<RefCell<Option<gtk::cairo::ImageSurface>>>, hint: &'static str) -> gtk::DrawingArea {
    let a = gtk::DrawingArea::new();
    a.set_content_height(120);
    a.set_hexpand(true);
    let slot = slot.clone();
    a.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        super::rounded_rect(cr, 0.0, 0.0, w, h, 8.0);
        super::theme::set(cr, "feld");
        let _ = cr.fill_preserve();
        cr.clip();
        match slot.borrow().as_ref() {
            Some(s) => super::draw_surface(cr, s, 0.0, 0.0, w, h, false),
            None => { super::draw_text(cr, hint, 12.0, h / 2.0 - 8.0, 12.0, (0.55, 0.56, 0.61, 1.0), false, w - 24.0); }
        }
    });
    a
}

pub struct RecorderUi {
    app: Weak<App>,
    pub window: gtk::Window,
    sw_screen: gtk::Switch,
    sw_cam: gtk::Switch,
    sw_mic: gtk::Switch,
    sw_sys: gtk::Switch,
    dd_cam: gtk::DropDown,
    dd_mic: gtk::DropDown,
    dd_sys: gtk::DropDown,
    dd_fps: gtk::DropDown,
    dd_size: gtk::DropDown,
    cb_cursor: gtk::CheckButton,
    cb_gpu: gtk::CheckButton,
    cb_countdown: gtk::CheckButton,
    cams: RefCell<Vec<Dev>>,
    mics: RefCell<Vec<Dev>>,
    outs: RefCell<Vec<Dev>>,
    screen_label: gtk::Label,
    screen_area: gtk::DrawingArea,
    cam_area: gtk::DrawingArea,
    screen_surf: Rc<RefCell<Option<gtk::cairo::ImageSurface>>>,
    cam_surf: Rc<RefCell<Option<gtk::cairo::ImageSurface>>>,
    dd_bg: gtk::DropDown,
    mic_level: (gtk::DrawingArea, Rc<Cell<f32>>),
    sys_level: (gtk::DrawingArea, Rc<Cell<f32>>),
    status: gtk::Label,
    rec_btn: gtk::Button,
    source: RefCell<Option<Rc<ScreenSource>>>,
    /// zweite Quelle für „aktivem Bildschirm folgen“
    source2: RefCell<Option<Rc<ScreenSource>>>,
    cb_follow: gtk::CheckButton,
    follow_box: gtk::Box,
    mon_row: gtk::Box,
    screen2_label: gtk::Label,
    dd_mon1: gtk::DropDown,
    dd_mon2: gtk::DropDown,
    mon_names: RefCell<Vec<String>>,
    previews: RefCell<Previews>,
    recording: RefCell<Option<Recording>>,
    bar: RefCell<Option<ControlBar>>,
    busy: Cell<bool>,
    loading: Cell<bool>,
    last_opts: RefCell<Option<RecOptions>>,
}

struct ControlBar {
    window: gtk::Window,
    time: gtk::Label,
    pause: gtk::Button,
    pause_icon: Rc<Cell<&'static str>>,
    cam_area: gtk::DrawingArea,
    cam_surf: Rc<RefCell<Option<gtk::cairo::ImageSurface>>>,
    mic: (gtk::DrawingArea, Rc<Cell<f32>>),
    sys: (gtk::DrawingArea, Rc<Cell<f32>>),
    hint: gtk::Label,
}

impl RecorderUi {
    pub fn new(app: &Rc<App>) -> Rc<RecorderUi> {
        let window = gtk::Window::builder().title("WS.Video – Neue Aufnahme").default_width(780).resizable(false).build();
        window.set_transient_for(Some(&app.window));
        window.set_hide_on_close(true);
        let root = vbox(14);
        root.set_margin_top(18);
        root.set_margin_bottom(18);
        root.set_margin_start(18);
        root.set_margin_end(18);
        root.append(&label("Neue Aufnahme", "ws-title"));
        root.append(&label("Quellen einschalten, Gerät wählen und aufnehmen. Jede Quelle landet auf einer eigenen Spur.", "ws-dim"));

        let grid = gtk::Grid::new();
        grid.set_row_spacing(12);
        grid.set_column_spacing(12);
        grid.set_column_homogeneous(true);

        // Bildschirm
        let c_screen = card("screen", "Bildschirm");
        let screen_surf = Rc::new(RefCell::new(None));
        let screen_area = preview_area(&screen_surf, "Noch keine Quelle gewählt");
        let screen_label = label("Monitor, Fenster oder Bereich", "ws-dim ws-small");
        let choose = gtk::Button::with_label("Quelle wählen …");
        c_screen.body.append(&screen_area);
        let row = hbox(8);
        screen_label.set_hexpand(true);
        row.append(&screen_label);
        row.append(&choose);
        c_screen.body.append(&row);
        // Zuordnung zum Monitor und „aktivem Bildschirm folgen“ (nur unter Hyprland)
        let mon_row = hbox(8);
        mon_row.append(&label("Das ist Monitor", "ws-dim ws-small"));
        let dd_mon1 = gtk::DropDown::from_strings(&["Automatisch"]);
        dd_mon1.set_hexpand(true);
        dd_mon1.set_tooltip_text(Some("Welcher Monitor hier aufgenommen wird. Davon hängt ab, wo der aufgezeichnete Mauszeiger im Bild sitzt. Bei zwei gleich großen Monitoren bitte von Hand wählen."));
        mon_row.append(&dd_mon1);
        c_screen.body.append(&mon_row);
        let cb_follow = gtk::CheckButton::with_label("Aktivem Bildschirm folgen");
        cb_follow.set_tooltip_text(Some("Nimmt zwei Monitore auf und zeigt im Video immer den, auf dem der Mauszeiger gerade ist. Steht der Zeiger auf einem dritten Monitor (zum Beispiel dem mit WS.Video), bleibt das Bild beim letzten."));
        c_screen.body.append(&cb_follow);
        let follow_box = vbox(6);
        let row2 = hbox(8);
        let screen2_label = label("Zweiter Bildschirm: noch nicht gewählt", "ws-dim ws-small");
        screen2_label.set_hexpand(true);
        let choose2 = gtk::Button::with_label("Zweiten wählen …");
        row2.append(&screen2_label);
        row2.append(&choose2);
        follow_box.append(&row2);
        let row3 = hbox(8);
        row3.append(&label("Das ist Monitor", "ws-dim ws-small"));
        let dd_mon2 = gtk::DropDown::from_strings(&["Automatisch"]);
        dd_mon2.set_hexpand(true);
        row3.append(&dd_mon2);
        follow_box.append(&row3);
        follow_box.set_visible(false);
        c_screen.body.append(&follow_box);
        grid.attach(&c_screen.frame, 0, 0, 1, 1);

        // Kamera
        let c_cam = card("camera", "Kamera");
        let cam_surf = Rc::new(RefCell::new(None));
        let cam_area = preview_area(&cam_surf, "Kamera ist aus");
        let dd_cam = gtk::DropDown::from_strings(&[]);
        c_cam.body.append(&cam_area);
        c_cam.body.append(&dd_cam);
        let bg_row = hbox(6);
        let dd_bg = gtk::DropDown::from_strings(&["Hintergrund: Original", "Hintergrund entfernen", "Hintergrund unscharf", "Eigenes Hintergrundbild"]);
        dd_bg.set_hexpand(true);
        dd_bg.set_tooltip_text(Some("Wird auf das Kamerabild angewendet. Die Aufnahme bleibt unverändert gespeichert, du kannst den Hintergrund im Editor jederzeit ändern."));
        let b_bg = gtk::Button::with_label("Bild …");
        b_bg.set_tooltip_text(Some("Hintergrundbild wählen (JPG oder PNG)"));
        bg_row.append(&dd_bg);
        bg_row.append(&b_bg);
        c_cam.body.append(&bg_row);
        grid.attach(&c_cam.frame, 1, 0, 1, 1);

        // Mikrofon
        let c_mic = card("mic", "Mikrofon");
        let dd_mic = gtk::DropDown::from_strings(&[]);
        let mic_level = super::level_bar();
        c_mic.body.append(&dd_mic);
        c_mic.body.append(&mic_level.0);
        grid.attach(&c_mic.frame, 0, 1, 1, 1);

        // Systemton
        let c_sys = card("speaker", "Systemton");
        let dd_sys = gtk::DropDown::from_strings(&[]);
        let sys_level = super::level_bar();
        c_sys.body.append(&dd_sys);
        c_sys.body.append(&sys_level.0);
        grid.attach(&c_sys.frame, 1, 1, 1, 1);
        root.append(&grid);

        // Optionen
        let opts = hbox(16);
        opts.append(&label("Bildrate", "ws-dim"));
        let dd_fps = gtk::DropDown::from_strings(&["30 Bilder/s", "60 Bilder/s"]);
        opts.append(&dd_fps);
        opts.append(&label("Größe", "ws-dim"));
        let dd_size = gtk::DropDown::from_strings(&["Automatisch", "Original", "höchstens 1440p", "höchstens 1080p"]);
        dd_size.set_tooltip_text(Some("Größe der Bildschirmaufnahme. Ein 4K-Bildschirm kostet beim Aufnehmen und später beim Bearbeiten ein Mehrfaches an Rechenzeit. „Automatisch“ nimmt in Originalgröße auf und verkleinert nur dann auf 1440p, wenn ohne Grafikkarte kodiert wird und der Rechner weniger als acht Prozessorkerne hat."));
        opts.append(&dd_size);
        let cb_cursor = gtk::CheckButton::with_label("Mauszeiger als Daten aufzeichnen");
        cb_cursor.set_tooltip_text(Some("Der Zeiger wird nicht ins Bild gebrannt, sondern getrennt gespeichert. So lassen sich später Größe, Glättung, Hervorhebung und Klick-Ringe einstellen (nur unter Hyprland)."));
        opts.append(&cb_cursor);
        let cb_gpu = gtk::CheckButton::with_label("Grafikkarte nutzen");
        cb_gpu.set_tooltip_text(Some("Video über die Grafikkarte kodieren (VAAPI oder NVENC). Spart viel Rechenzeit; wenn es nicht klappt, wird automatisch auf Software umgeschaltet."));
        opts.append(&cb_gpu);
        let cb_countdown = gtk::CheckButton::with_label("3 Sekunden Vorlauf");
        opts.append(&cb_countdown);
        root.append(&opts);

        let bottom = hbox(14);
        let status = label("", "ws-dim");
        status.set_wrap(true);
        status.set_hexpand(true);
        bottom.append(&status);
        let rec_btn = gtk::Button::new();
        let rc = hbox(8);
        rc.append(&icons::widget("record", 18));
        rc.append(&gtk::Label::new(Some("Aufnahme starten")));
        rec_btn.set_child(Some(&rc));
        rec_btn.add_css_class("ws-rec");
        bottom.append(&rec_btn);
        root.append(&bottom);
        window.set_child(Some(&root));

        let ui = Rc::new(RecorderUi {
            app: Rc::downgrade(app),
            window,
            sw_screen: c_screen.switch.clone(),
            sw_cam: c_cam.switch.clone(),
            sw_mic: c_mic.switch.clone(),
            sw_sys: c_sys.switch.clone(),
            dd_cam, dd_bg, dd_mic, dd_sys, dd_fps, dd_size, cb_cursor, cb_gpu, cb_countdown,
            cams: RefCell::new(vec![]), mics: RefCell::new(vec![]), outs: RefCell::new(vec![]),
            screen_label, screen_area, cam_area, screen_surf, cam_surf, mic_level, sys_level, status, rec_btn,
            source: RefCell::new(None), source2: RefCell::new(None), cb_follow, follow_box, mon_row, screen2_label, dd_mon1, dd_mon2, mon_names: RefCell::new(vec![]),
            previews: RefCell::new(Previews::default()), recording: RefCell::new(None), bar: RefCell::new(None),
            busy: Cell::new(false), loading: Cell::new(true), last_opts: RefCell::new(None),
        });

        // Einstellungen laden
        {
            let s = app.settings.borrow();
            ui.sw_screen.set_active(s.rec_screen);
            ui.sw_cam.set_active(s.rec_camera);
            ui.dd_bg.set_selected(s.cam_bg_mode.min(3));
            ui.sw_mic.set_active(s.rec_mic);
            ui.sw_sys.set_active(s.rec_system);
            ui.dd_fps.set_selected(if s.rec_fps >= 60 { 1 } else { 0 });
            ui.dd_size.set_selected(SIZES.iter().position(|k| *k == s.rec_size).unwrap_or(0) as u32);
            ui.cb_cursor.set_active(s.rec_cursor_data);
            ui.cb_gpu.set_active(s.rec_hardware);
            ui.cb_countdown.set_active(true);
        }
        if !hypr::available() && !fake() {
            ui.cb_cursor.set_active(false);
            ui.cb_cursor.set_sensitive(false);
            ui.mon_row.set_visible(false);
            ui.cb_follow.set_visible(false);
        } else {
            ui.fill_monitors();
            let follow = app.settings.borrow().rec_follow;
            ui.cb_follow.set_active(follow);
            ui.follow_box.set_visible(follow);
        }
        ui.reload_devices();
        ui.loading.set(false);

        // Signale
        {
            let w = Rc::downgrade(&ui);
            ui.dd_bg.connect_selected_notify(move |_| { if let Some(u) = w.upgrade() { u.sources_changed(); } });
            let w = Rc::downgrade(&ui);
            b_bg.connect_clicked(move |_| {
                let Some(u) = w.upgrade() else { return };
                let filter = gtk::FileFilter::new();
                filter.add_pixbuf_formats();
                filter.set_name(Some("Bilder"));
                let d = gtk::FileDialog::builder().title("Hintergrundbild wählen").default_filter(&filter).build();
                let w2 = Rc::downgrade(&u);
                d.open(Some(&u.window), gtk::gio::Cancellable::NONE, move |res| {
                    let (Ok(f), Some(u)) = (res, w2.upgrade()) else { return };
                    let Some(path) = f.path() else { return };
                    if let Some(app) = u.app.upgrade() { app.settings.borrow_mut().cam_bg_image = path.to_string_lossy().to_string(); }
                    if u.dd_bg.selected() == 3 { u.sources_changed(); } else { u.dd_bg.set_selected(3); }
                });
            });
        }
        for sw in [&ui.sw_screen, &ui.sw_cam, &ui.sw_mic, &ui.sw_sys] {
            let w = Rc::downgrade(&ui);
            sw.connect_active_notify(move |_| { if let Some(u) = w.upgrade() { u.sources_changed(); } });
        }
        for dd in [&ui.dd_cam, &ui.dd_mic, &ui.dd_sys] {
            let w = Rc::downgrade(&ui);
            dd.connect_selected_notify(move |_| { if let Some(u) = w.upgrade() { u.sources_changed(); } });
        }
        {
            let w = Rc::downgrade(&ui);
            ui.cb_cursor.connect_toggled(move |_| {
                // Der Zeiger-Modus wird bei der Quellenwahl festgelegt: Quelle neu wählen lassen
                if let Some(u) = w.upgrade() { if !u.loading.get() { u.drop_source(); } }
            });
        }
        {
            let w = Rc::downgrade(&ui);
            choose.connect_clicked(move |_| {
                if let Some(u) = w.upgrade() { super::spawn_local(async move { u.choose_source(false).await; }); }
            });
            let w = Rc::downgrade(&ui);
            choose2.connect_clicked(move |_| {
                if let Some(u) = w.upgrade() { super::spawn_local(async move { u.choose_second(false).await; }); }
            });
            let w = Rc::downgrade(&ui);
            ui.cb_follow.connect_toggled(move |b| {
                let Some(u) = w.upgrade() else { return };
                u.follow_box.set_visible(b.is_active());
                if !u.loading.get() { u.save_settings(); }
            });
            for dd in [&ui.dd_mon1, &ui.dd_mon2] {
                let w = Rc::downgrade(&ui);
                dd.connect_selected_notify(move |_| { if let Some(u) = w.upgrade() { if !u.loading.get() { u.save_settings(); } } });
            }
        }
        {
            let w = Rc::downgrade(&ui);
            ui.rec_btn.connect_clicked(move |_| {
                if let Some(u) = w.upgrade() { super::spawn_local(async move { u.start_recording().await; }); }
            });
        }
        {
            let w = Rc::downgrade(&ui);
            ui.window.connect_close_request(move |_| {
                if let Some(u) = w.upgrade() {
                    u.stop_previews();
                    u.drop_source();
                    if let Some(app) = u.app.upgrade() { app.window.present(); }
                }
                glib::Propagation::Proceed
            });
        }
        // Vorschau und Pegel regelmäßig auffrischen
        {
            let w = Rc::downgrade(&ui);
            glib::timeout_add_local(std::time::Duration::from_millis(60), move || {
                let Some(u) = w.upgrade() else { return glib::ControlFlow::Break };
                u.tick();
                glib::ControlFlow::Continue
            });
        }
        ui
    }

    pub fn present(self: &Rc<Self>) {
        self.reload_devices();
        self.window.present();
        self.restart_previews();
    }

    pub fn is_recording(&self) -> bool { self.recording.borrow().is_some() }

    /// Für Tastenkürzel: Aufnahme mit den letzten Einstellungen starten, ohne das Fenster zu zeigen.
    pub fn start_now(self: &Rc<Self>) {
        if self.is_recording() || self.busy.get() { return; }
        self.reload_devices();
        let u = self.clone();
        super::spawn_local(async move {
            u.clone().start_recording().await;
            // nichts gestartet (abgebrochen oder Fehler): Fenster zeigen, damit man sieht, woran es liegt
            if !u.is_recording() && !u.window.is_visible() { u.present(); }
        });
    }

    /// Monitore aus Hyprland in die beiden Auswahlfelder schreiben.
    fn fill_monitors(&self) {
        let mut mons = if fake() {
            vec![("TEST-1".to_string(), 1920u32, 1080u32, 0.0f64), ("TEST-2".to_string(), 1920, 1080, 1920.0)]
        } else {
            hypr::monitors().into_iter().map(|m| (m.name, m.pw, m.ph, m.x)).collect()
        };
        mons.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));
        let n = mons.len();
        let mut items = vec!["Automatisch".to_string()];
        for (i, (name, w, h, _)) in mons.iter().enumerate() {
            let place = if n < 2 { String::new() } else if i == 0 { " · links".into() } else if i == n - 1 { " · rechts".into() } else if n == 3 { " · Mitte".into() } else { format!(" · Nr. {} von links", i + 1) };
            items.push(format!("{name} · {w} × {h}{place}"));
        }
        let refs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
        let names: Vec<String> = mons.into_iter().map(|m| m.0).collect();
        let (want1, want2) = self.app.upgrade().map(|a| { let s = a.settings.borrow(); (s.rec_monitor1.clone(), s.rec_monitor2.clone()) }).unwrap_or_default();
        let was = self.loading.replace(true);
        for (dd, want) in [(&self.dd_mon1, want1), (&self.dd_mon2, want2)] {
            dd.set_model(Some(&gtk::StringList::new(&refs)));
            dd.set_selected(names.iter().position(|n| *n == want).map(|i| i as u32 + 1).unwrap_or(0));
        }
        self.loading.set(was);
        *self.mon_names.borrow_mut() = names;
    }

    fn monitor_choice(&self, dd: &gtk::DropDown) -> String {
        let i = dd.selected() as usize;
        if i == 0 { String::new() } else { self.mon_names.borrow().get(i - 1).cloned().unwrap_or_default() }
    }

    /// Monitore der beiden Quellen fürs Folgen. „Automatisch“ wird nach der Bildgröße aufgelöst; bleibt etwas
    /// offen oder doppelt, gibt es eine Fehlermeldung.
    fn follow_monitors(&self) -> Result<(String, String), String> {
        let mut a = self.monitor_choice(&self.dd_mon1);
        let mut b = self.monitor_choice(&self.dd_mon2);
        if a.is_empty() || b.is_empty() {
            let mons = hypr::monitors();
            let size = |s: &RefCell<Option<Rc<ScreenSource>>>| s.borrow().as_ref().and_then(|s| s.size);
            let fits = |sz: Option<(i32, i32)>, taken: &str| -> Vec<String> {
                mons.iter().filter(|m| m.name != taken && sz.map(|(w, h)| m.pw as i32 == w && m.ph as i32 == h).unwrap_or(false)).map(|m| m.name.clone()).collect()
            };
            if a.is_empty() { let f = fits(size(&self.source), &b); if f.len() == 1 { a = f[0].clone(); } }
            if b.is_empty() { let f = fits(size(&self.source2), &a); if f.len() == 1 { b = f[0].clone(); } }
        }
        if a.is_empty() || b.is_empty() { return Err("Bitte bei beiden Bildschirmen unter „Das ist Monitor“ den Monitor wählen. Automatisch geht nur, wenn die Größe eindeutig ist.".into()); }
        if a == b { return Err("Beide Bildschirme sind demselben Monitor zugeordnet. Bitte unter „Das ist Monitor“ zwei verschiedene wählen.".into()); }
        Ok((a, b))
    }

    fn set_status(&self, text: &str, error: bool) {
        self.status.set_text(text);
        self.status.remove_css_class("ws-err");
        self.status.remove_css_class("ws-dim");
        self.status.add_css_class(if error { "ws-err" } else { "ws-dim" });
    }

    fn reload_devices(&self) {
        let was = self.loading.replace(true);
        let s = self.app.upgrade().map(|a| a.settings.borrow().clone()).unwrap_or_default();
        let cams = if fake() { vec![Dev { id: "/dev/video0".into(), name: "Testkamera".into(), default: true }] } else { devices::cameras() };
        let (mics, outs) = if fake() { (vec![Dev { id: String::new(), name: "Testmikrofon".into(), default: true }], vec![Dev { id: "@DEFAULT_MONITOR@".into(), name: "Testausgabe".into(), default: true }]) } else { devices::audio_devices() };
        let fill = |dd: &gtk::DropDown, list: &[Dev], wanted: &str, empty: &str| {
            let names: Vec<&str> = if list.is_empty() { vec![empty] } else { list.iter().map(|d| d.name.as_str()).collect() };
            dd.set_model(Some(&gtk::StringList::new(&names)));
            let idx = list.iter().position(|d| d.id == wanted).unwrap_or(0);
            dd.set_selected(idx as u32);
            dd.set_sensitive(!list.is_empty());
        };
        fill(&self.dd_cam, &cams, &s.camera_device, "Keine Kamera gefunden");
        fill(&self.dd_mic, &mics, &s.mic_device, "Kein Mikrofon gefunden");
        fill(&self.dd_sys, &outs, &s.system_device, "Keine Ausgabe gefunden");
        if cams.is_empty() { self.sw_cam.set_active(false); self.sw_cam.set_sensitive(false); }
        *self.cams.borrow_mut() = cams;
        *self.mics.borrow_mut() = mics;
        *self.outs.borrow_mut() = outs;
        self.loading.set(was);
    }

    fn selected(list: &RefCell<Vec<Dev>>, dd: &gtk::DropDown) -> Option<String> {
        list.borrow().get(dd.selected() as usize).map(|d| d.id.clone())
    }

    fn options(&self) -> RecOptions {
        RecOptions {
            screen: self.sw_screen.is_active(),
            cursor_data: self.cb_cursor.is_active(),
            camera: if self.sw_cam.is_active() { Self::selected(&self.cams, &self.dd_cam) } else { None },
            mic: if self.sw_mic.is_active() { Self::selected(&self.mics, &self.dd_mic) } else { None },
            system: if self.sw_sys.is_active() { Self::selected(&self.outs, &self.dd_sys) } else { None },
            fps: if self.dd_fps.selected() == 1 { 60 } else { 30 },
            hardware: self.cb_gpu.is_active(),
            size: session::RecSize::from_setting(self.size_key()),
        }
    }

    fn size_key(&self) -> &'static str { SIZES.get(self.dd_size.selected() as usize).copied().unwrap_or("auto") }

    fn save_settings(&self) {
        let Some(app) = self.app.upgrade() else { return };
        let mut s = app.settings.borrow_mut();
        s.rec_screen = self.sw_screen.is_active();
        s.rec_camera = self.sw_cam.is_active();
        s.cam_bg_mode = self.dd_bg.selected().min(3);
        s.rec_mic = self.sw_mic.is_active();
        s.rec_system = self.sw_sys.is_active();
        if let Some(d) = Self::selected(&self.cams, &self.dd_cam) { s.camera_device = d; }
        if let Some(d) = Self::selected(&self.mics, &self.dd_mic) { s.mic_device = d; }
        if let Some(d) = Self::selected(&self.outs, &self.dd_sys) { s.system_device = d; }
        s.rec_fps = if self.dd_fps.selected() == 1 { 60 } else { 30 };
        s.rec_size = self.size_key().to_string();
        s.rec_cursor_data = self.cb_cursor.is_active();
        s.rec_hardware = self.cb_gpu.is_active();
        s.rec_follow = self.cb_follow.is_active();
        s.rec_monitor1 = self.monitor_choice(&self.dd_mon1);
        s.rec_monitor2 = self.monitor_choice(&self.dd_mon2);
        s.save();
    }

    fn sources_changed(self: &Rc<Self>) {
        if self.loading.get() || self.busy.get() { return; }
        self.save_settings();
        self.restart_previews();
    }

    fn stop_previews(&self) {
        *self.previews.borrow_mut() = Previews::default();
        self.mic_level.1.set(0.0);
        self.sys_level.1.set(0.0);
    }

    /// Kamerabild und Pegelanzeigen passend zu den Schaltern (neu) starten.
    fn restart_previews(self: &Rc<Self>) {
        if self.is_recording() || !self.window.is_visible() { return; }
        let o = self.options();
        let mut p = self.previews.borrow_mut();
        p.camera = None;
        p.mic = None;
        p.system = None;
        *self.cam_surf.borrow_mut() = None;
        if let Some(cam) = &o.camera {
            let src = if fake() {
                "videotestsrc is-live=true pattern=smpte ! video/x-raw,width=640,height=360".to_string()
            } else {
                match devices::camera_format(cam) {
                    Some((caps, true)) => format!("v4l2src device={cam} ! {caps} ! jpegdec"),
                    Some((caps, false)) => format!("v4l2src device={cam} ! {caps}"),
                    None => format!("v4l2src device={cam} ! decodebin"),
                }
            };
            // Vorschau mit dem gewählten Hintergrund (die Aufnahme selbst bleibt unverändert)
            let (bg_mode, bg_image) = self.app.upgrade().map(|a| { let s = a.settings.borrow(); (s.cam_bg_mode, s.cam_bg_image.clone()) }).unwrap_or((0, String::new()));
            let src = match bg_mode {
                0 => src,
                1 => format!("{src} ! videoconvert ! videoscale ! video/x-raw,format=BGRA,height=200 ! wsperson bg-mode=4"),
                3 if !bg_image.is_empty() => format!("{src} ! videoconvert ! videoscale ! video/x-raw,format=BGRA,height=200 ! wsperson bg-mode=3 bg-path=\"{}\"", bg_image.replace('\\', "\\\\").replace('"', "\\\"")),
                _ => format!("{src} ! videoconvert ! videoscale ! video/x-raw,format=BGRA,height=200 ! wsperson bg-mode=2"),
            };
            p.camera = MiniPipe::video(&src, None);
        }
        let audio_src = |dev: &str, fake_src: &str| {
            if fake() { fake_src.to_string() } else if dev.is_empty() { "pulsesrc client-name=WS.Video".to_string() } else { format!("pulsesrc device=\"{dev}\" client-name=WS.Video") }
        };
        if let Some(m) = &o.mic { p.mic = MiniPipe::audio(&audio_src(m, "audiotestsrc is-live=true wave=ticks")); }
        if let Some(s) = &o.system { p.system = MiniPipe::audio(&audio_src(s, "audiotestsrc is-live=true volume=0.05")); }
        drop(p);
        self.cam_area.queue_draw();
    }

    fn tick(&self) {
        if let Some(bar) = self.bar.borrow().as_ref() {
            // Aufnahme läuft: Steuerleiste versorgen
            let mut failed: Option<String> = None;
            if let Some(rec) = self.recording.borrow_mut().as_mut() {
                rec.poll();
                let t = rec.elapsed();
                bar.time.set_text(&crate::model::format_dur(t));
                let (m, s) = rec.levels();
                bar.mic.1.set(m);
                bar.sys.1.set(s);
                bar.mic.0.queue_draw();
                bar.sys.0.queue_draw();
                if rec.has_camera() {
                    if let Some(f) = rec.cam_frame() { *bar.cam_surf.borrow_mut() = super::frame_surface(&f); bar.cam_area.queue_draw(); }
                }
                if let Some(e) = &rec.error { failed = Some(e.clone()); }
                if let Some(n) = rec.active_screen() { if !rec.is_paused() && failed.is_none() { bar.hint.set_text(&format!("Zeigt Bildschirm {n}")); } }
            }
            if let Some(e) = failed {
                bar.hint.set_text(&format!("Fehler: {e}"));
            }
            return;
        }
        if !self.window.is_visible() { return; }
        let mut p = self.previews.borrow_mut();
        if let Some(c) = p.camera.as_mut() {
            c.poll();
            if let Some(f) = c.frame.lock().unwrap().take() { *self.cam_surf.borrow_mut() = super::frame_surface(&f); self.cam_area.queue_draw(); }
        }
        if let Some(c) = p.screen.as_mut() {
            c.poll();
            if let Some(f) = c.frame.lock().unwrap().take() { *self.screen_surf.borrow_mut() = super::frame_surface(&f); self.screen_area.queue_draw(); }
        }
        let lvl = |pipe: &mut Option<MiniPipe>, bar: &(gtk::DrawingArea, Rc<Cell<f32>>)| {
            let v = match pipe.as_mut() { Some(m) => { m.poll(); m.level } None => 0.0 };
            // langsam abfallen lassen
            let shown = bar.1.get();
            bar.1.set(if v > shown { v } else { shown * 0.85 });
            bar.0.queue_draw();
        };
        lvl(&mut p.mic, &self.mic_level);
        lvl(&mut p.system, &self.sys_level);
    }

    fn drop_source(&self) {
        self.previews.borrow_mut().screen = None;
        *self.screen_surf.borrow_mut() = None;
        self.screen_area.queue_draw();
        if let Some(s) = self.source.borrow_mut().take() {
            super::spawn_local(async move { s.close().await; });
        }
        if let Some(s) = self.source2.borrow_mut().take() {
            super::spawn_local(async move { s.close().await; });
        }
        self.screen_label.set_text("Monitor, Fenster oder Bereich");
        self.screen2_label.set_text("Zweiter Bildschirm: noch nicht gewählt");
    }

    /// Zweite Quelle für „aktivem Bildschirm folgen“ wählen.
    async fn choose_second(self: Rc<Self>, quiet: bool) -> bool {
        if fake() { self.screen2_label.set_text("Zweiter Bildschirm: Testbild"); return true; }
        if let Some(s) = self.source2.borrow_mut().take() { s.close().await; }
        let hide = self.cb_cursor.is_active() && hypr::available();
        let token = if quiet { self.app.upgrade().map(|a| a.settings.borrow().restore_token2.clone()).filter(|t| !t.is_empty()) } else { None };
        self.set_status("Bitte im Dialog des Systems den zweiten Bildschirm wählen …", false);
        match ScreenSource::open(hide, token).await {
            Ok(src) => {
                if let (Some(app), Some(t)) = (self.app.upgrade(), src.restore_token.clone()) {
                    let mut s = app.settings.borrow_mut();
                    s.restore_token2 = t;
                    s.save();
                }
                self.screen2_label.set_text(&format!("Zweiter Bildschirm: {}", src.describe()));
                *self.source2.borrow_mut() = Some(Rc::new(src));
                self.set_status("", false);
                true
            }
            Err(e) => { self.set_status(&format!("{e}"), true); false }
        }
    }

    /// Auswahl des Systems öffnen. `quiet` nutzt die gemerkte Auswahl, wenn das Portal sie zulässt.
    async fn choose_source(self: Rc<Self>, quiet: bool) -> bool {
        if fake() {
            self.screen_label.set_text("Testbild 1920 × 1080");
            return true;
        }
        self.drop_source();
        let hide = self.cb_cursor.is_active() && hypr::available();
        let token = if quiet { self.app.upgrade().map(|a| a.settings.borrow().restore_token.clone()).filter(|t| !t.is_empty()) } else { None };
        self.set_status("Bitte im Dialog des Systems den Bildschirm wählen …", false);
        match ScreenSource::open(hide, token).await {
            Ok(src) => {
                if let (Some(app), Some(t)) = (self.app.upgrade(), src.restore_token.clone()) {
                    let mut s = app.settings.borrow_mut();
                    s.restore_token = t;
                    s.save();
                }
                self.screen_label.set_text(&format!("Gewählt: {}", src.describe()));
                let src = Rc::new(src);
                *self.source.borrow_mut() = Some(src.clone());
                self.sw_screen.set_active(true);
                self.set_status("", false);
                // Live-Vorschau der gewählten Quelle
                if let Ok(fd) = src.fd().await {
                    use std::os::fd::AsRawFd;
                    let desc = format!("pipewiresrc fd={} path={} do-timestamp=true keepalive-time=1000", fd.as_raw_fd(), src.node);
                    self.previews.borrow_mut().screen = MiniPipe::video(&desc, Some(fd));
                }
                true
            }
            Err(e) => {
                self.set_status(&format!("{e}"), true);
                false
            }
        }
    }

    async fn start_recording(self: Rc<Self>) {
        if self.busy.get() || self.is_recording() { return; }
        let mut opts = self.options();
        if !opts.screen && opts.camera.is_none() && opts.mic.is_none() && opts.system.is_none() {
            self.set_status("Bitte mindestens eine Quelle einschalten.", true);
            return;
        }
        self.busy.set(true);
        self.rec_btn.set_sensitive(false);
        self.save_settings();
        if opts.screen && !fake() && self.source.borrow().is_none() {
            if !self.clone().choose_source(true).await {
                self.busy.set(false);
                self.rec_btn.set_sensitive(true);
                return;
            }
        }
        // Aktivem Bildschirm folgen: zweite Quelle öffnen und beide einem Monitor zuordnen
        let follow = opts.screen && !fake() && self.cb_follow.is_active() && self.cb_follow.is_visible();
        let mut mon_names = (self.monitor_choice(&self.dd_mon1), String::new());
        if follow {
            let mut err = None;
            if self.source2.borrow().is_none() && !self.clone().choose_second(true).await { err = Some(String::new()); }
            if err.is_none() { match self.follow_monitors() { Ok(m) => mon_names = m, Err(e) => err = Some(e) } }
            if let Some(e) = err {
                if !e.is_empty() { self.set_status(&e, true); }
                self.busy.set(false);
                self.rec_btn.set_sensitive(true);
                self.window.present();
                return;
            }
        }
        self.stop_previews();
        let Some(app) = self.app.upgrade() else { return };
        // Fenster ausblenden, Steuerleiste zeigen
        self.window.set_visible(false);
        app.window.set_visible(false);
        let bar = self.make_bar();
        {
            // Steuerleiste schwebend und auf allen Arbeitsumgebungen, möglichst auf einem Monitor, der nicht aufgenommen wird
            let mut recorded: Vec<String> = Vec::new();
            if opts.screen { for n in [&mon_names.0, &mon_names.1] { if !n.is_empty() { recorded.push(n.clone()); } } }
            place_bar(recorded);
        }
        if self.cb_countdown.is_active() {
            for n in (1..=3).rev() {
                bar.time.set_text(&format!("Start in {n}"));
                glib::timeout_future(std::time::Duration::from_millis(900)).await;
            }
        }
        bar.time.set_text("Starte …");
        *self.bar.borrow_mut() = Some(bar);

        let mut notes: Vec<String> = Vec::new();
        let mut atts = session::attempts(opts.hardware);
        let mut rec: Option<Recording> = None;
        let mut tries = 0;
        while !atts.is_empty() && tries < 7 {
            tries += 1;
            let att = atts[0].clone();
            let mut input: Vec<ScreenInput> = Vec::new();
            if opts.screen {
                let mut srcs = vec![(self.source.borrow().clone(), mon_names.0.clone())];
                if follow { srcs.push((self.source2.borrow().clone(), mon_names.1.clone())); }
                let mut failed = false;
                for (src, monitor) in srcs {
                    match src {
                        Some(s) => match s.fd().await {
                            Ok(fd) => input.push(ScreenInput { fd: Some(fd), node: s.node, size: s.size, position: s.position, cursor_hidden: s.cursor_hidden, monitor }),
                            Err(e) => { notes.push(format!("PipeWire-Verbindung fehlgeschlagen: {e}")); failed = true; break; }
                        },
                        None => input.push(ScreenInput { fd: None, node: 0, size: Some((1920, 1080)), position: None, cursor_hidden: true, monitor }),
                    }
                }
                if failed { break; }
            }
            match Recording::start(&opts, input, &att) {
                Ok(mut r) => {
                    let t0 = std::time::Instant::now();
                    let mut ok = false;
                    loop {
                        glib::timeout_future(std::time::Duration::from_millis(80)).await;
                        r.poll();
                        if r.error.is_some() { break; }
                        if r.has_data() { ok = true; break; }
                        if t0.elapsed().as_secs_f64() > 6.0 { ok = r.screen_has_data() && r.missing_sources().len() < 4; break; }
                    }
                    if ok && r.error.is_none() {
                        for m in r.missing_sources() { notes.push(format!("{m} liefert noch keine Daten.")); }
                        rec = Some(r);
                        break;
                    }
                    let err = r.error.clone().unwrap_or_else(|| "es kamen keine Daten an".to_string());
                    let source = r.error_source.clone();
                    let missing = r.missing_sources();
                    r.discard();
                    // Ursache eingrenzen: einzelne Quelle abschalten oder nächsten Encoder probieren
                    if source == "cam" || (source.is_empty() && missing == vec!["Kamera"]) {
                        notes.push(format!("Kamera abgeschaltet: {err}"));
                        opts.camera = None;
                    } else if source == "mic" || (source.is_empty() && missing == vec!["Mikrofon"]) {
                        notes.push(format!("Mikrofon abgeschaltet: {err}"));
                        opts.mic = None;
                    } else if source == "sys" || (source.is_empty() && missing == vec!["Systemton"]) {
                        notes.push(format!("Systemton abgeschaltet: {err}"));
                        opts.system = None;
                    } else {
                        notes.push(format!("Versuch mit {}{} fehlgeschlagen: {err}", att.encoder, if att.limit_rate { "" } else { " (ohne Bildraten-Vorgabe)" }));
                        atts.remove(0);
                    }
                    if !opts.screen && opts.camera.is_none() && opts.mic.is_none() && opts.system.is_none() { break; }
                }
                Err(e) => {
                    notes.push(format!("{e}"));
                    atts.remove(0);
                }
            }
        }
        self.busy.set(false);
        self.rec_btn.set_sensitive(true);
        match rec {
            Some(mut r) => {
                r.notes.extend(notes);
                if let Some(bar) = self.bar.borrow().as_ref() {
                    bar.cam_area.set_visible(r.has_camera());
                    bar.mic.0.set_visible(r.has_mic());
                    bar.sys.0.set_visible(r.has_system());
                    let gpu = if r.attempt.on_gpu { "Grafikkarte" } else { "Software" };
                    bar.hint.set_text(&format!("Encoder: {} ({gpu})", r.attempt.encoder));
                }
                *self.last_opts.borrow_mut() = Some(opts);
                *self.recording.borrow_mut() = Some(r);
            }
            None => {
                if let Some(bar) = self.bar.borrow_mut().take() { bar.window.destroy(); }
                app.window.set_visible(true);
                self.window.present();
                self.set_status(&format!("Die Aufnahme konnte nicht gestartet werden.\n{}", notes.join("\n")), true);
                self.restart_previews();
            }
        }
    }

    fn make_bar(self: &Rc<Self>) -> ControlBar {
        let window = gtk::Window::builder().title("WS.Video Aufnahme").resizable(false).decorated(false).build();
        let b = hbox(10);
        b.set_margin_top(10);
        b.set_margin_bottom(10);
        b.set_margin_start(14);
        b.set_margin_end(14);
        let dot = icons::widget("record", 16);
        dot.add_css_class("ws-recdot");
        b.append(&dot);
        let time = label("00:00", "ws-bigtime");
        time.set_width_chars(9);
        b.append(&time);
        let pause_icon = Rc::new(Cell::new("pause"));
        let pause = gtk::Button::new();
        pause.set_tooltip_text(Some("Pause / Weiter"));
        let pi = pause_icon.clone();
        let pa = gtk::DrawingArea::new();
        pa.set_content_width(18);
        pa.set_content_height(18);
        pa.set_draw_func(move |w, cr, width, _| {
            let c = w.color();
            icons::draw(cr, pi.get(), width as f64, (c.red() as f64, c.green() as f64, c.blue() as f64, 1.0));
        });
        pause.set_child(Some(&pa));
        b.append(&pause);
        let restart = super::icon_button("restart", "", "Verwerfen und neu beginnen");
        b.append(&restart);
        let stop = super::icon_button("stop", "Stopp", "Aufnahme beenden und im Editor öffnen");
        stop.add_css_class("ws-primary");
        b.append(&stop);
        let cam_surf = Rc::new(RefCell::new(None));
        let cam_area = gtk::DrawingArea::new();
        cam_area.set_content_width(96);
        cam_area.set_content_height(54);
        let cs: Rc<RefCell<Option<gtk::cairo::ImageSurface>>> = cam_surf.clone();
        cam_area.set_draw_func(move |_, cr, w, h| {
            super::rounded_rect(cr, 0.0, 0.0, w as f64, h as f64, 6.0);
            super::theme::set(cr, "feld");
            let _ = cr.fill_preserve();
            cr.clip();
            if let Some(s) = cs.borrow().as_ref() { super::draw_surface(cr, s, 0.0, 0.0, w as f64, h as f64, true); }
        });
        b.append(&cam_area);
        let levels = vbox(6);
        levels.set_valign(gtk::Align::Center);
        let mic = super::level_bar();
        let sys = super::level_bar();
        mic.0.set_tooltip_text(Some("Mikrofon"));
        sys.0.set_tooltip_text(Some("Systemton"));
        levels.append(&mic.0);
        levels.append(&sys.0);
        b.append(&levels);
        let outer = vbox(0);
        outer.append(&b);
        let hint = label("", "ws-dim ws-small");
        hint.set_margin_start(14);
        hint.set_margin_bottom(8);
        outer.append(&hint);
        // Das ganze Fenster ist Griff: an einer freien Stelle anfassen und ziehen verschiebt es.
        let handle = gtk::WindowHandle::builder().child(&outer).build();
        handle.set_tooltip_text(Some("Zum Verschieben an einer freien Stelle anfassen und ziehen"));
        window.set_child(Some(&handle));
        {
            let w = Rc::downgrade(self);
            pause.connect_clicked(move |_| { if let Some(u) = w.upgrade() { u.toggle_pause(); } });
        }
        {
            let w = Rc::downgrade(self);
            stop.connect_clicked(move |_| { if let Some(u) = w.upgrade() { u.stop_recording(); } });
        }
        {
            let w = Rc::downgrade(self);
            restart.connect_clicked(move |_| {
                if let Some(u) = w.upgrade() {
                    let rec = u.recording.borrow_mut().take();
                    if let Some(r) = rec { r.discard(); }
                    if let Some(bar) = u.bar.borrow_mut().take() { bar.window.destroy(); }
                    super::spawn_local(async move { u.start_recording().await; });
                }
            });
        }
        {
            let w = Rc::downgrade(self);
            window.connect_close_request(move |_| {
                if let Some(u) = w.upgrade() { if u.is_recording() { u.stop_recording(); } }
                glib::Propagation::Proceed
            });
        }
        window.present();
        ControlBar { window, time, pause, pause_icon, cam_area, cam_surf, mic, sys, hint }
    }

    pub fn toggle_pause(&self) {
        let rec = self.recording.borrow();
        let Some(r) = rec.as_ref() else { return };
        if r.is_paused() { r.resume(); } else { r.pause(); }
        if let Some(bar) = self.bar.borrow().as_ref() {
            bar.pause_icon.set(if r.is_paused() { "play" } else { "pause" });
            if let Some(c) = bar.pause.child() { c.queue_draw(); }
            bar.hint.set_text(if r.is_paused() { "Pausiert" } else { "" });
        }
    }

    pub fn stop_recording(self: &Rc<Self>) {
        let Some(rec) = self.recording.borrow_mut().take() else { return };
        if let Some(bar) = self.bar.borrow().as_ref() { bar.time.set_text("Speichere …"); }
        let result = rec.stop();
        if let Some(bar) = self.bar.borrow_mut().take() { bar.window.destroy(); }
        self.drop_source();
        if let Some(app) = self.app.upgrade() {
            app.window.set_visible(true);
            app.window.present();
            app.recording_finished(result);
        }
    }
}

/// Steuerleiste unter Hyprland einrichten: schwebend (sonst lässt sie sich nicht verschieben), auf allen
/// Arbeitsumgebungen sichtbar und möglichst auf einem Monitor, der nicht aufgenommen wird.
/// `recorded` = Namen der aufgenommenen Monitore (leer = unbekannt, dann bleibt die Leiste, wo sie ist).
fn place_bar(recorded: Vec<String>) {
    if !hypr::available() { return; }
    glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
        const TITLE: &str = "WS.Video Aufnahme";
        let sel = "title:^(WS.Video Aufnahme)$";
        let Some((floating, _, mon)) = hypr::client(TITLE) else { return };
        if !floating { let _ = hypr::request(&format!("dispatch setfloating {sel}")); }
        if !recorded.is_empty() {
            let mons = hypr::monitor_workspaces();
            let here = mons.iter().find(|m| m.1 == mon).map(|m| m.0.clone()).unwrap_or_default();
            if recorded.contains(&here) {
                if let Some(free) = mons.iter().find(|m| !recorded.contains(&m.0) && m.2 >= 0) {
                    let _ = hypr::request(&format!("dispatch movetoworkspacesilent {},{sel}", free.2));
                }
            }
        }
        // anheften schaltet um, deshalb nur, wenn es noch nicht angeheftet ist
        if let Some((_, pinned, _)) = hypr::client(TITLE) { if !pinned { let _ = hypr::request(&format!("dispatch pin {sel}")); } }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vorschau_verkleinern() {
        let (w, h) = (3840, 2160);
        let mut data = vec![0u8; (w * h * 4) as usize];
        // rechte Bildhälfte weiß
        for y in 0..h { for x in w / 2..w { let o = ((y * w + x) * 4) as usize; data[o] = 255; data[o + 1] = 255; data[o + 2] = 255; } }
        let f = shrink(Frame { w, h, stride: w * 4, data }, 240);
        assert_eq!((f.w, f.h), (427, 240));
        assert_eq!(f.data[(100 * f.stride + 10 * 4) as usize], 0);
        assert_eq!(f.data[(100 * f.stride + 400 * 4) as usize], 255);
    }
}
