//! Anwendungszustand: Hauptfenster, Startseite, Editor und Aufnahme.
use super::{editor, recorder, start};
use crate::model::Project;
use crate::recorder::session::RecResult;
use crate::settings::Settings;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub struct App {
    pub gtk_app: gtk::Application,
    pub window: gtk::ApplicationWindow,
    pub stack: gtk::Stack,
    pub settings: RefCell<Settings>,
    pub editor: RefCell<Option<Rc<editor::Editor>>>,
    pub recorder: RefCell<Option<Rc<recorder::RecorderUi>>>,
    pub start: RefCell<Option<start::StartPage>>,
    /// eben neu angelegtes, noch leeres Projekt
    fresh: RefCell<Option<PathBuf>>,
}

/// Projektdatei in einem Projektordner: bevorzugt die, die wie der Ordner heißt.
pub fn project_file_in(dir: &Path) -> Option<PathBuf> {
    let ext = crate::model::FILE_EXT;
    if let Some(name) = dir.file_name() {
        let f = dir.join(format!("{}.{ext}", name.to_string_lossy()));
        if f.is_file() { return Some(f); }
    }
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == ext).unwrap_or(false)).collect();
    all.sort();
    all.into_iter().next()
}

pub fn media_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.set_name(Some("Videos, Bilder und Ton"));
    for m in ["video/*", "image/*", "audio/*"] { f.add_mime_type(m); }
    for e in ["mkv", "mp4", "mov", "webm", "avi", "png", "jpg", "jpeg", "webp", "svg", "mp3", "wav", "ogg", "flac", "m4a", "mka", "opus"] { f.add_suffix(e); }
    f
}

impl App {
    pub fn new(gtk_app: &gtk::Application) -> Rc<App> {
        super::install_css();
        let window = gtk::ApplicationWindow::builder().application(gtk_app).title("WS.Video").default_width(1500).default_height(920).build();
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(120);
        window.set_child(Some(&stack));
        // eigene Kopfzeile statt der Titelleiste des Systems
        window.set_titlebar(Some(&super::header(&window)));
        let app = Rc::new(App {
            gtk_app: gtk_app.clone(),
            window,
            stack,
            settings: RefCell::new(Settings::load()),
            editor: RefCell::new(None),
            recorder: RefCell::new(None),
            start: RefCell::new(None),
            fresh: RefCell::new(None),
        });
        let sp = start::StartPage::new(&app);
        app.stack.add_named(&sp.root, Some("start"));
        *app.start.borrow_mut() = Some(sp);
        {
            let a = Rc::downgrade(&app);
            app.window.connect_close_request(move |_| {
                if let Some(app) = a.upgrade() {
                    if let Some(r) = app.recorder.borrow().as_ref() { if r.is_recording() { r.stop_recording(); } }
                    app.close_editor();
                }
                glib::Propagation::Proceed
            });
        }
        app
    }

    pub fn show_start(self: &Rc<Self>) {
        if let Some(e) = self.editor.borrow().as_ref() { e.save(); e.pause(); }
        if let Some(s) = self.start.borrow().as_ref() { s.refresh(self); }
        self.stack.set_visible_child_name("start");
        self.window.set_title(Some("WS.Video"));
    }

    pub fn close_editor(&self) {
        if let Some(e) = self.editor.borrow_mut().take() {
            e.save();
            e.shutdown();
            self.stack.remove(&e.root);
            // Ein neues Projekt, in das nie etwas gelegt wurde, hinterlässt keinen leeren Ordner.
            let (path, empty) = { let st = e.st.borrow(); (st.path.clone(), st.project.clips.is_empty() && st.project.media.is_empty()) };
            let is_fresh = self.fresh.borrow().as_deref() == Some(path.as_path());
            if is_fresh {
                *self.fresh.borrow_mut() = None;
                if empty {
                    let _ = std::fs::remove_file(&path);
                    if let Some(d) = path.parent() { let _ = std::fs::remove_dir(d); }
                }
            }
        }
    }

    pub fn open_recorder(self: &Rc<Self>) {
        if let Some(e) = self.editor.borrow().as_ref() { e.pause(); }
        let existing = self.recorder.borrow().clone();
        let r = match existing {
            Some(r) => r,
            None => {
                let r = recorder::RecorderUi::new(self);
                *self.recorder.borrow_mut() = Some(r.clone());
                r
            }
        };
        r.present();
    }

    fn default_project_path() -> PathBuf {
        let now = glib::DateTime::now_local().ok().and_then(|d| d.format("%Y-%m-%d_%H-%M-%S").ok()).map(|s| s.to_string()).unwrap_or_else(|| "neu".into());
        // jedes Projekt bekommt seinen eigenen Ordner
        crate::settings::videos_dir().join(format!("Projekt_{now}")).join(format!("Projekt_{now}.{}", crate::model::FILE_EXT))
    }

    pub fn new_project(self: &Rc<Self>) {
        let path = Self::default_project_path();
        *self.fresh.borrow_mut() = Some(path.clone());
        self.open_editor(Project::default(), path);
    }

    pub fn open_editor(self: &Rc<Self>, project: Project, path: PathBuf) {
        self.close_editor();
        let _ = project.save(&path);
        self.settings.borrow_mut().add_recent(&path.to_string_lossy());
        match editor::Editor::new(self, project, path.clone()) {
            Ok(ed) => {
                self.stack.add_named(&ed.root, Some("editor"));
                self.stack.set_visible_child_name("editor");
                self.window.set_title(Some(&format!("{} – WS.Video", path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())));
                *self.editor.borrow_mut() = Some(ed);
            }
            Err(e) => super::message(&self.window, "Der Editor konnte nicht gestartet werden", &format!("{e}")),
        }
    }

    pub fn open_project(self: &Rc<Self>, path: &Path) {
        // Auch der Projektordner selbst lässt sich öffnen.
        let found;
        let path = if path.is_dir() {
            match project_file_in(path) {
                Some(f) => { found = f; found.as_path() }
                None => { super::message(&self.window, "In diesem Ordner liegt kein Projekt", &format!("{}", path.display())); return; }
            }
        } else { path };
        match Project::load(path) {
            Ok(p) => self.open_editor(p, path.to_path_buf()),
            Err(e) => super::message(&self.window, "Das Projekt konnte nicht geöffnet werden", &format!("{}\n{e}", path.display())),
        }
    }

    pub fn open_project_dialog(self: &Rc<Self>) {
        let f = gtk::FileFilter::new();
        f.set_name(Some("WS.Video-Projekte"));
        f.add_suffix(crate::model::FILE_EXT);
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&f);
        let d = gtk::FileDialog::builder().title("Projekt öffnen").filters(&filters).initial_folder(&gio::File::for_path(crate::settings::videos_dir())).build();
        let app = self.clone();
        d.open(Some(&self.window), gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res { if let Some(p) = file.path() { app.open_project(&p); } }
        });
    }

    /// Dateien wählen und in das aktuelle (oder ein neues) Projekt importieren.
    pub fn import_dialog(self: &Rc<Self>) {
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&media_filter());
        let d = gtk::FileDialog::builder().title("Medien importieren").filters(&filters).build();
        let app = self.clone();
        d.open_multiple(Some(&self.window), gio::Cancellable::NONE, move |res| {
            let Ok(list) = res else { return };
            let mut paths = Vec::new();
            for i in 0..list.n_items() {
                if let Some(f) = list.item(i).and_then(|o| o.downcast::<gio::File>().ok()) { if let Some(p) = f.path() { paths.push(p.to_string_lossy().to_string()); } }
            }
            if paths.is_empty() { return; }
            if app.editor.borrow().is_none() { app.new_project(); }
            let ed = app.editor.borrow().clone();
            if let Some(ed) = ed {
                app.stack.set_visible_child_name("editor");
                ed.import_files(&paths, false);
            }
        });
    }

    /// Mediendateien (z. B. von der Kommandozeile) in das aktuelle oder ein neues Projekt legen.
    pub fn open_media(self: &Rc<Self>, paths: &[String]) {
        if self.editor.borrow().is_none() { self.new_project(); }
        let ed = self.editor.borrow().clone();
        if let Some(ed) = ed {
            self.stack.set_visible_child_name("editor");
            ed.import_files(paths, true);
        }
    }

    pub fn recording_finished(self: &Rc<Self>, r: RecResult) {
        let fps = self.settings.borrow().rec_fps.max(30);
        if r.screen.is_none() && r.camera.is_none() && r.mic.is_none() && r.system.is_none() {
            super::message(&self.window, "Die Aufnahme enthält keine Daten", &r.notes.join("\n"));
            return;
        }
        let cam_bg = { let s = self.settings.borrow(); (crate::model::BgMode::from_index(s.cam_bg_mode), s.cam_bg_image.clone()) };
        let mut project = crate::ops::project_from_recording(&r, fps, cam_bg);
        // Das Hintergrundbild der Kamera gehört mit in den Ordner der Aufnahme.
        let media_dir = r.dir.join(crate::model::MEDIA_DIR);
        for c in &mut project.clips {
            if !c.bg_image.is_empty() {
                if let Ok(p) = crate::model::adopt_file(Path::new(&c.bg_image), &media_dir) { c.bg_image = p.to_string_lossy().to_string(); }
            }
        }
        let name = r.dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Aufnahme".into());
        let path = r.dir.join(format!("{name}.{}", crate::model::FILE_EXT));
        self.open_editor(project, path);
        if !r.notes.is_empty() {
            if let Some(ed) = self.editor.borrow().as_ref() { ed.show_note(&r.notes.join("\n")); }
        }
    }

    /// Befehle von der Kommandozeile (auch aus einer zweiten Instanz, z. B. per Tastenkürzel).
    pub fn command(self: &Rc<Self>, cmd: &str) {
        match cmd {
            "stop" => { if let Some(r) = self.recorder.borrow().clone() { r.stop_recording(); } }
            "pause" => { if let Some(r) = self.recorder.borrow().as_ref() { r.toggle_pause(); } }
            "record" => self.open_recorder(),
            "start" | "toggle" => {
                let existing = self.recorder.borrow().clone();
                let r = match existing {
                    Some(r) => r,
                    None => {
                        if let Some(e) = self.editor.borrow().as_ref() { e.pause(); }
                        let r = recorder::RecorderUi::new(self);
                        *self.recorder.borrow_mut() = Some(r.clone());
                        r
                    }
                };
                if r.is_recording() { if cmd == "toggle" { r.stop_recording(); } } else { r.start_now(); }
            }
            _ => {}
        }
    }
}
