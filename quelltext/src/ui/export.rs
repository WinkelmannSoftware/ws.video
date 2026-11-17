//! Export-Dialog: Auflösung, Codec, Qualität, Fortschritt.
use super::editor::Editor;
use super::{hbox, label, vbox};
use crate::engine::{self, Codec, ExportJob, ExportOpts};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

const CODECS: [Codec; 3] = [Codec::H264, Codec::Hevc, Codec::Av1];

fn heights(project_h: u32) -> Vec<(u32, String)> {
    let mut v = vec![(project_h, format!("Projektgröße ({project_h}p)"))];
    for (h, n) in [(2160u32, "4K (2160p)"), (1440, "2K (1440p)"), (1080, "Full HD (1080p)"), (720, "HD (720p)")] {
        if h != project_h { v.push((h, n.to_string())); }
    }
    v
}

pub fn open(ed: &Rc<Editor>) {
    let Some(win) = ed.window() else { return };
    let Some(app) = ed.app.upgrade() else { return };
    ed.pause();
    ed.save();
    let (project, proj_path) = { let st = ed.st.borrow(); (st.project.clone(), st.path.clone()) };
    if project.duration() <= 0.0 {
        super::message(&win, "Das Projekt ist noch leer", "Lege zuerst eine Aufnahme oder ein Medium auf die Zeitleiste.");
        return;
    }
    let settings = app.settings.borrow().clone();

    let dlg = gtk::Window::builder().transient_for(&win).modal(true).title("Exportieren").default_width(520).resizable(false).build();
    let root = vbox(12);
    root.set_margin_top(18);
    root.set_margin_bottom(18);
    root.set_margin_start(20);
    root.set_margin_end(20);
    dlg.set_child(Some(&root));
    root.append(&label("Video exportieren", "ws-title"));

    let grid = gtk::Grid::builder().row_spacing(10).column_spacing(12).build();
    root.append(&grid);
    let hs = heights(project.height);
    let names: Vec<&str> = hs.iter().map(|h| h.1.as_str()).collect();
    let dd_size = gtk::DropDown::from_strings(&names);
    dd_size.set_hexpand(true);
    let codec_names: Vec<&str> = CODECS.iter().map(|c| c.label()).collect();
    let dd_codec = gtk::DropDown::from_strings(&codec_names);
    dd_codec.set_selected(settings.export_codec.min(2));
    let dd_quality = gtk::DropDown::from_strings(&["Gut (kleine Datei)", "Hoch (empfohlen)", "Sehr hoch (große Datei)"]);
    dd_quality.set_selected(settings.export_quality.min(2));
    let cb_gpu = gtk::CheckButton::with_label("Grafikkarte zum Kodieren verwenden (schneller, schont die CPU)");
    cb_gpu.set_active(settings.export_hardware);
    for (i, (t, w)) in [("Auflösung", dd_size.clone().upcast::<gtk::Widget>()), ("Format", dd_codec.clone().upcast()), ("Qualität", dd_quality.clone().upcast())].into_iter().enumerate() {
        grid.attach(&label(t, "ws-dim"), 0, i as i32, 1, 1);
        grid.attach(&w, 1, i as i32, 1, 1);
    }
    grid.attach(&cb_gpu, 0, 3, 2, 1);

    let info = label("", "ws-dim ws-small");
    info.set_wrap(true);
    root.append(&info);
    let progress = gtk::ProgressBar::new();
    progress.set_visible(false);
    root.append(&progress);
    let status = label("", "ws-small");
    status.set_wrap(true);
    status.set_visible(false);
    root.append(&status);

    let buttons = hbox(8);
    buttons.set_halign(gtk::Align::End);
    let b_folder = gtk::Button::with_label("Ordner öffnen");
    b_folder.set_visible(false);
    let b_close = gtk::Button::with_label("Schließen");
    let b_start = gtk::Button::with_label("Exportieren …");
    b_start.add_css_class("ws-primary");
    buttons.append(&b_folder);
    buttons.append(&b_close);
    buttons.append(&b_start);
    root.append(&buttons);

    // Hinweis: welcher Encoder würde verwendet, wie groß wird die Datei ungefähr
    let update_info = {
        let (info, dd_size, dd_codec, dd_quality, cb_gpu, hs, project) = (info.clone(), dd_size.clone(), dd_codec.clone(), dd_quality.clone(), cb_gpu.clone(), hs.clone(), project.clone());
        move || {
            let h = hs[(dd_size.selected() as usize).min(hs.len() - 1)].0;
            let codec = CODECS[dd_codec.selected().min(2) as usize];
            let (w, hh) = engine::Builder::out_size(&project, h as f64 / project.height.max(1) as f64);
            let kbps = engine::export_bitrate(w, hh, project.fps, codec, dd_quality.selected());
            let mb = (kbps as f64 + 160.0) * project.duration() / 8.0 / 1000.0;
            let enc = match engine::pick_encoder(codec, cb_gpu.is_active()) {
                Some((n, true)) => format!("Kodiert auf der Grafikkarte ({n})."),
                Some((n, false)) => format!("Kodiert auf der CPU ({n})."),
                None => "Für dieses Format ist auf diesem Rechner kein Encoder installiert. Bitte ein anderes Format wählen.".to_string(),
            };
            info.set_text(&format!("{w} × {hh}, {} Bilder/s, etwa {:.1} Mbit/s, ungefähr {:.0} MB. {enc}", project.fps, kbps as f64 / 1000.0, mb.max(1.0)));
        }
    };
    update_info();
    { let u = update_info.clone(); dd_size.connect_selected_notify(move |_| u()); }
    { let u = update_info.clone(); dd_codec.connect_selected_notify(move |_| u()); }
    { let u = update_info.clone(); dd_quality.connect_selected_notify(move |_| u()); }
    { let u = update_info.clone(); cb_gpu.connect_toggled(move |_| u()); }

    let job: Rc<RefCell<Option<ExportJob>>> = Rc::new(RefCell::new(None));
    let out_path: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    // Schließen bricht einen laufenden Export ab
    {
        let (job1, dlg2) = (job.clone(), dlg.clone());
        b_close.connect_clicked(move |_| {
            if let Some(j) = job1.borrow_mut().take() { j.cancel(); }
            dlg2.close();
        });
        let job = job.clone();
        dlg.connect_close_request(move |_| {
            if let Some(j) = job.borrow_mut().take() { j.cancel(); }
            glib::Propagation::Proceed
        });
    }
    {
        let out_path = out_path.clone();
        b_folder.connect_clicked(move |_| {
            let p = out_path.borrow().clone();
            if let Some(dir) = std::path::Path::new(&p).parent() {
                let _ = gio::AppInfo::launch_default_for_uri(&engine::path_uri(&dir.to_string_lossy()), gio::AppLaunchContext::NONE);
            }
        });
    }
    {
        let (dlg2, app2) = (dlg.clone(), app.clone());
        let widgets = (dd_size.clone(), dd_codec.clone(), dd_quality.clone(), cb_gpu.clone(), b_start.clone(), b_close.clone(), b_folder.clone(), progress.clone(), status.clone());
        b_start.connect_clicked(move |_| {
            let (dd_size, dd_codec, dd_quality, cb_gpu, b_start, b_close, b_folder, progress, status) = widgets.clone();
            let h = hs[(dd_size.selected() as usize).min(hs.len() - 1)].0;
            let codec = CODECS[dd_codec.selected().min(2) as usize];
            let (quality, hardware) = (dd_quality.selected(), cb_gpu.is_active());
            let stem = proj_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Video".into());
            let fd = gtk::FileDialog::builder().title("Video speichern").initial_name(format!("{stem}.mp4")).build();
            let dir = { let s = app2.settings.borrow(); if s.export_dir.is_empty() { crate::settings::videos_dir() } else { std::path::PathBuf::from(&s.export_dir) } };
            fd.set_initial_folder(Some(&gio::File::for_path(dir)));
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Video (MP4, MKV)"));
            filter.add_suffix("mp4");
            filter.add_suffix("mkv");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            fd.set_filters(Some(&filters));
            let (project, job, out_path, app3, dlg3) = (project.clone(), job.clone(), out_path.clone(), app2.clone(), dlg2.clone());
            fd.save(Some(&dlg2), gio::Cancellable::NONE, move |res| {
                let Ok(file) = res else { return };
                let Some(mut path) = file.path() else { return };
                let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                if ext != "mp4" && ext != "mkv" { path.set_extension("mp4"); }
                let path_s = path.to_string_lossy().to_string();
                {
                    let mut s = app3.settings.borrow_mut();
                    s.export_codec = CODECS.iter().position(|c| *c == codec).unwrap_or(0) as u32;
                    s.export_quality = quality;
                    s.export_hardware = hardware;
                    if let Some(d) = path.parent() { s.export_dir = d.to_string_lossy().to_string(); }
                    s.save();
                }
                *out_path.borrow_mut() = path_s.clone();
                status.set_visible(true);
                status.remove_css_class("ws-err");
                b_folder.set_visible(false);
                engine::preload_project(&project);
                let opts = ExportOpts { path: path_s.clone(), height: h, codec, hardware, quality };
                let started = match ExportJob::start(&project, &opts) {
                    Ok(j) => j,
                    Err(e) => { status.set_text(&format!("Export konnte nicht starten: {e}")); status.add_css_class("ws-err"); return; }
                };
                status.set_text(&format!("Exportiere mit {} …", started.encoder));
                *job.borrow_mut() = Some(started);
                progress.set_visible(true);
                progress.set_fraction(0.0);
                b_start.set_sensitive(false);
                b_close.set_label("Abbrechen");
                let t0 = std::time::Instant::now();
                // Fällt die Grafikkarte beim Kodieren aus, einmal automatisch mit der CPU wiederholen
                let retried = std::cell::Cell::new(false);
                let (job, progress, status, b_start, b_close, b_folder, dlg4, project) = (job.clone(), progress.clone(), status.clone(), b_start.clone(), b_close.clone(), b_folder.clone(), dlg3.clone(), project.clone());
                glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
                    let state = match job.borrow().as_ref() { Some(j) => { progress.set_fraction(j.progress()); j.poll() } None => return glib::ControlFlow::Break };
                    let Some(result) = state else {
                        let f = progress.fraction();
                        if f > 0.02 {
                            let rest = t0.elapsed().as_secs_f64() * (1.0 - f) / f;
                            progress.set_show_text(true);
                            progress.set_text(Some(&format!("{:.0} %, noch etwa {}", f * 100.0, crate::model::format_dur(rest))));
                        }
                        return glib::ControlFlow::Continue;
                    };
                    let old = job.borrow_mut().take();
                    match result {
                        Ok(()) => {
                            progress.set_fraction(1.0);
                            progress.set_text(Some("100 %"));
                            let size = std::fs::metadata(&opts.path).map(|m| m.len() as f64 / 1e6).unwrap_or(0.0);
                            status.set_text(&format!("Fertig in {}: {} ({:.1} MB)", crate::model::format_dur(t0.elapsed().as_secs_f64()), opts.path, size));
                            b_folder.set_visible(true);
                            b_close.set_label("Schließen");
                            b_start.set_sensitive(true);
                            dlg4.present();
                        }
                        Err(e) => {
                            let was_gpu = old.as_ref().map(|j| j.on_gpu).unwrap_or(false);
                            if let Some(j) = old { j.cancel(); }
                            if was_gpu && !retried.get() {
                                retried.set(true);
                                let mut o2 = opts.clone();
                                o2.hardware = false;
                                if let Ok(j) = ExportJob::start(&project, &o2) {
                                    status.set_text(&format!("Die Grafikkarte hat den Export abgelehnt. Ich wiederhole ihn auf der CPU ({}) …", j.encoder));
                                    *job.borrow_mut() = Some(j);
                                    return glib::ControlFlow::Continue;
                                }
                            }
                            progress.set_visible(false);
                            status.set_text(&format!("Export fehlgeschlagen: {e}"));
                            status.add_css_class("ws-err");
                            b_close.set_label("Schließen");
                            b_start.set_sensitive(true);
                        }
                    }
                    glib::ControlFlow::Break
                });
            });
        });
    }
    dlg.present();
}
