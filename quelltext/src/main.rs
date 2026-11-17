mod annot;
mod audiofx;
mod bericht;
mod crashlog;
mod cursordata;
mod elements;
mod engine;
mod model;
mod fonts;
mod kuerzel;
mod ops;
mod recorder;
mod selbst;
mod settings;
mod ui;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_ID: &str = "app.wsvideo.WSVideo";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    engine::init()?;
    if args.len() >= 4 && args[1] == "--render" {
        // ws-video --render projekt.wsvideo ausgabe.mp4 [höhe] [h264|hevc|av1] [soft]
        let p = model::Project::load(std::path::Path::new(&args[2]))?;
        let height = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(p.height);
        let codec = match args.get(5).map(|s| s.as_str()) { Some("hevc") => engine::Codec::Hevc, Some("av1") => engine::Codec::Av1, _ => engine::Codec::H264 };
        let hardware = args.get(6).map(|s| s != "soft").unwrap_or(true);
        let job = engine::ExportJob::start(&p, &engine::ExportOpts { path: args[3].clone(), height, codec, hardware, quality: 1 })?;
        println!("Encoder: {} (GPU: {})", job.encoder, job.on_gpu);
        let t0 = std::time::Instant::now();
        loop {
            match job.poll() {
                Some(Ok(())) => { println!("fertig in {:.1} s", t0.elapsed().as_secs_f64()); break; }
                Some(Err(e)) => { anyhow::bail!("Export fehlgeschlagen: {e}"); }
                None => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        return Ok(());
    }
    if args.len() >= 3 && args[1] == "--test-ton" {
        // ws-video --test-ton projekt.wsvideo [rauschen|alles|aus]: Ton aller Clips messen, Bearbeitung setzen, speichern
        let path = std::path::Path::new(&args[2]);
        let mut p = model::Project::load(path)?;
        let mode = args.get(3).map(|s| s.as_str()).unwrap_or("alles");
        let paths: std::collections::BTreeSet<String> = p.clips.iter().filter(|c| c.carries_audio()).map(|c| c.path.clone()).collect();
        for f in paths {
            let t0 = std::time::Instant::now();
            match engine::analyse_audio(&f) {
                Some(pr) => {
                    println!("{f}: Grundrauschen {:.1} dB, Lautstärke {:.1} dB, Spitze {:.1} dB, gleichmäßig: {}, Bänder: {} ({} ms)", pr.floor, pr.loud, pr.peak, pr.steady, pr.noise.len(), t0.elapsed().as_millis());
                    p.audio_profiles.insert(f, pr);
                }
                None => println!("{f}: kein Ton messbar"),
            }
        }
        for c in p.clips.iter_mut().filter(|c| c.carries_audio()) {
            c.audio = match mode {
                "rauschen" => model::AudioFx { denoise: 0.6, ..Default::default() },
                "aus" => model::AudioFx::default(),
                "voll" => model::AudioFx { gate: 0.5, voice: true, bass: 2.0, treble: 2.0, ..model::AudioFx::improved() },
                _ => model::AudioFx::improved(),
            };
        }
        p.save(path)?;
        return Ok(());
    }
    if args.len() >= 3 && args[1] == "--test-load" {
        // Zeitmessung: wie lange braucht GES, um Dateien zu laden?
        use ges::prelude::*;
        for a in &args[2..] {
            let t0 = std::time::Instant::now();
            let ok = ges::UriClipAsset::request_sync(&engine::path_uri(a)).is_ok();
            println!("{a}: {} ms ({ok})", t0.elapsed().as_millis());
            let t0 = std::time::Instant::now();
            let ok = engine::probe(a).is_ok();
            println!("  probe: {} ms ({ok})", t0.elapsed().as_millis());
        }
        return Ok(());
    }
    if args.len() >= 2 && args[1] == "--test-record" {
        // Testaufnahme ohne Oberfläche: WS_FAKE=1 ws-video --test-record [Sekunden] [Bilder/s] [kamera] [pause] [soft] [breite=… höhe=…] [größe=auto|original|1440|1080]
        let secs: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4.0);
        let fps: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
        let has = |w: &str| args.iter().skip(2).any(|a| a == w);
        let num = |k: &str, d: i32| args.iter().find_map(|a| a.strip_prefix(k).and_then(|v| v.parse().ok())).unwrap_or(d);
        let opts = recorder::session::RecOptions {
            screen: true, cursor_data: true, camera: if has("kamera") { Some("/dev/video0".into()) } else { None }, mic: Some(String::new()),
            system: if has("systemton") { Some("@DEFAULT_MONITOR@".into()) } else { None }, fps, hardware: !has("soft"),
            size: recorder::session::RecSize::from_setting(args.iter().find_map(|a| a.strip_prefix("größe=")).unwrap_or("original")),
        };
        let att = recorder::session::attempts(opts.hardware);
        println!("Versuche: {att:?}");
        let screen = recorder::session::ScreenInput { fd: None, node: 0, size: Some((num("breite=", 1920), num("höhe=", 1080))), position: None, cursor_hidden: true, monitor: String::new() };
        let mut rec = recorder::session::Recording::start(&opts, vec![screen], &att[0])?;
        let t0 = std::time::Instant::now();
        let pause = has("pause");
        let mut paused_once = false;
        while t0.elapsed().as_secs_f64() < secs + if pause { 1.5 } else { 0.0 } {
            std::thread::sleep(std::time::Duration::from_millis(50));
            rec.poll();
            let e = t0.elapsed().as_secs_f64();
            if pause && !paused_once && e > secs / 2.0 { rec.pause(); paused_once = true; println!("Pause bei {:.2}", rec.elapsed()); }
            if paused_once && rec.is_paused() && e > secs / 2.0 + 1.5 { rec.resume(); println!("weiter"); }
            if let Some(err) = &rec.error { anyhow::bail!("Fehler: {err}"); }
        }
        println!("Daten da: {} Pegel {:?} Kamerabild {}", rec.has_data(), rec.levels(), rec.cam_frame().is_some());
        let t1 = std::time::Instant::now();
        let r = rec.stop();
        println!("Abschluss nach Stopp: {:.2} s", t1.elapsed().as_secs_f64());
        println!("{r:#?}");
        let project = ops::project_from_recording(&r, fps.max(30), (model::BgMode::Off, String::new()));
        let name = r.dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Aufnahme".into());
        let path = r.dir.join(format!("{name}.{}", model::FILE_EXT));
        project.save(&path)?;
        println!("Projekt: {}", path.display());
        return Ok(());
    }
    if args.get(1).map(|a| a == "--thema-beispiel").unwrap_or(false) {
        // gibt das blaue Design als JSON aus: Vorlage für eigene Designs in ~/.config/ws-video/themen/
        if let Some(t) = ui::theme::all().into_iter().find(|t| t.id == "blau") { println!("{}", t.to_json()); }
        return Ok(());
    }
    if args.len() >= 3 && args[1] == "--test-stress" {
        // Belastungsprobe: Clips in schneller Folge neu aufbauen, während die Vorschau läuft oder steht.
        gtk::init()?;
        let mut p = model::Project::load(std::path::Path::new(&args[2]))?;
        let secs: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20.0);
        let pause_ms: u64 = args.iter().find_map(|a| a.strip_prefix("takt=").and_then(|v| v.parse().ok())).unwrap_or(15);
        let eng = engine::Engine::new(&p)?;
        let ctx = glib::MainContext::default();
        let run = |ms: u64| { let t = std::time::Instant::now(); while t.elapsed().as_millis() < ms as u128 { ctx.iteration(false); eng.borrow_mut().tick(); std::thread::sleep(std::time::Duration::from_millis(2)); } };
        run(1000);
        let ids: Vec<u64> = p.clips.iter().filter(|c| c.kind == model::Kind::Image).map(|c| c.id).collect();
        let ids = if ids.is_empty() { p.clips.iter().map(|c| c.id).collect() } else { ids };
        // art=zieh: einen Clip wie mit der Maus in der Zeitleiste verschieben (viele kleine Schritte)
        let drag = args.iter().any(|a| a == "art=zieh");
        // art=lage: Clips mit Ein- und Ausblenden in der Vorschau verschieben (ändert nur Eigenschaften)
        let lage = args.iter().any(|a| a == "art=lage");
        let ids: Vec<u64> = if drag || lage { p.clips.iter().map(|c| c.id).collect() } else { ids };
        if lage { for c in p.clips.iter_mut() { c.fade_in = 0.5; c.fade_out = 0.5; } }
        let t0 = std::time::Instant::now();
        let mut n = 0u64;
        while t0.elapsed().as_secs_f64() < secs {
            n += 1;
            let id = ids[(n as usize) % ids.len()];
            if let Some(c) = p.clips.iter_mut().find(|c| c.id == id) {
                // abwechselnd Beginn, Dauer und Schatten ändern: Jede dieser Änderungen baut den Clip neu auf
                if lage {
                    let up = (n / 60) % 2 == 0;
                    c.x += if up { 2.0 } else { -2.0 };
                    // jede zweite Runde auch die Größe ändern, wie beim Ziehen an einer Ecke
                    if (n / 120) % 2 == 1 && c.w > 40.0 && c.h > 40.0 { let f = if up { 1.004 } else { 1.0 / 1.004 }; c.w *= f; c.h *= f; }
                }
                else if drag { let k = (n % 200) as f64; c.start = if k < 100.0 { k * 0.03 } else { (200.0 - k) * 0.03 }; }
                else { match n % 3 { 0 => c.start = if c.start > 0.05 { 0.0 } else { 0.1 }, 1 => c.duration = if c.duration > 4.5 { 4.0 } else { 5.0 }, _ => c.shadow = !c.shadow } }
            }
            if (drag || lage) && n % 50 == 0 {
                let st = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
                let get = |k: &str| st.lines().find(|l| l.starts_with(k)).map(|l| l[k.len()..].trim().to_string()).unwrap_or_default();
                println!("Schritt {n}: Speicher {}, Threads {}", get("VmRSS:"), get("Threads:"));
            }
            eng.borrow_mut().sync(&p);
            if n % 40 == 0 { eng.borrow_mut().play(); }
            if n % 40 == 20 && !lage { eng.borrow_mut().pause(); }
            if n % 25 == 0 { eng.borrow_mut().seek((n % 7) as f64 * 0.5); }
            // unregelmäßiger Takt, damit das Entfernen in jede Phase des Ladens fällt
            run((n * 7) % (pause_ms.max(1) * 2));
        }
        run(1500);
        println!("Belastungsprobe überstanden: {n} Umbauten in {:.0} s", t0.elapsed().as_secs_f64());
        eng.borrow_mut().shutdown();
        return Ok(());
    }
    if args.len() >= 3 && args[1] == "--test-play" {
        // Vorschau ohne Oberfläche abspielen und messen:
        //   ws-video --test-play projekt.wsvideo [Sekunden] [Startzeit] [ablauf=p8,h2,s12,p3,e] [retry=Sekunden]
        // Ablauf: pN = N Sekunden abspielen, hN = N Sekunden anhalten, sT = zur Stelle T springen, e = bis zum Ende abspielen
        use std::sync::atomic::Ordering::Relaxed;
        let p = model::Project::load(std::path::Path::new(&args[2]))?;
        let secs: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10.0);
        let start: f64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let script = args.iter().find_map(|a| a.strip_prefix("ablauf=")).map(|s| s.to_string()).unwrap_or_else(|| format!("p{secs}"));
        let eng = engine::Engine::new(&p)?;
        if let Some(r) = args.iter().find_map(|a| a.strip_prefix("retry=").and_then(|v| v.parse::<f64>().ok())) { eng.borrow_mut().set_pace_retry(r); }
        let ctx = glib::MainContext::default();
        let step = || { ctx.iteration(false); eng.borrow_mut().tick(); std::thread::sleep(std::time::Duration::from_millis(5)); };
        let wait = |d: f64| { let t = std::time::Instant::now(); while t.elapsed().as_secs_f64() < d { step(); } };
        let still = |what: &str| {
            let e = eng.borrow();
            let lag = e.lag().map(|l| format!("{:+.0} ms", l * 1000.0)).unwrap_or_else(|| "unbekannt".into());
            println!("{what}: Position {:.3} s, Abstand des Standbilds {lag}", e.position());
        };
        wait(1.5);
        if start > 0.0 { eng.borrow_mut().seek(start); wait(1.5); }
        let stats = eng.borrow().stats.clone();
        let (mut frames, mut playtime, mut worst) = (0u64, 0.0f64, 0.0f64);
        for st in script.split(',').filter(|s| !s.is_empty()) {
            let (kind, val) = st.split_at(1);
            let v: f64 = val.parse().unwrap_or(0.0);
            match kind {
                "p" | "e" => {
                    let from = eng.borrow().position();
                    eng.borrow_mut().play();
                    let t0 = std::time::Instant::now();
                    let n0 = stats.shown.load(Relaxed);
                    let (mut last_n, mut last_t) = (n0, 0.0f64);
                    let limit = if kind == "e" { 900.0 } else { v };
                    while t0.elapsed().as_secs_f64() < limit && eng.borrow().is_playing() {
                        step();
                        let e = t0.elapsed().as_secs_f64();
                        let lag = eng.borrow().lag().unwrap_or(0.0);
                        if e > 0.3 { worst = worst.max(lag); }
                        if e - last_t >= 1.0 {
                            let n = stats.shown.load(Relaxed);
                            let red = eng.borrow().reduced_fps().map(|f| format!(", Vorschau auf {f:.1} Bilder/s gesenkt")).unwrap_or_default();
                            println!("  {:5.1} s: {:5.1} Bilder/s, Bild {:+.0} ms gegenüber dem Ton, Position {:.2} s{red}", e, (n - last_n) as f64 / (e - last_t), lag * 1000.0, eng.borrow().position());
                            last_n = n;
                            last_t = e;
                        }
                    }
                    let e = t0.elapsed().as_secs_f64();
                    frames += stats.shown.load(Relaxed) - n0;
                    playtime += e;
                    if !eng.borrow().is_playing() {
                        println!("Ende erreicht nach {e:.2} s (Spielzeit ab {from:.2} s: {:.2} s)", eng.borrow().duration - from);
                        wait(1.0);
                        still("Am Ende");
                    }
                }
                "h" => { eng.borrow_mut().pause(); wait(v.max(0.5)); still("Angehalten"); }
                "s" => { eng.borrow_mut().seek(v); wait(1.0); still("Sprung"); }
                _ => {}
            }
        }
        let red = eng.borrow().reduced_fps().map(|f| format!(", Vorschau zuletzt mit {f:.1} Bildern/s")).unwrap_or_default();
        println!("Ergebnis: {:.1} Bilder/s im Mittel, größter Rückstand {:.0} ms, zuletzt {:+.0} ms{red}", frames as f64 / playtime.max(0.001), worst * 1000.0, eng.borrow().lag().unwrap_or(0.0) * 1000.0);
        eng.borrow_mut().shutdown();
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("WS.Video {VERSION}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--bericht" || a.eq_ignore_ascii_case("--fehlerbericht")) {
        let p = crashlog::report()?;
        println!("Fehlerbericht geschrieben: {}\n\n{}", p.display(), bericht::NOTICE);
        return Ok(());
    }
    if args.iter().any(|a| a == "--startmenue") {
        match selbst::integrate() { Ok(t) => { println!("{t}"); return Ok(()); } Err(e) => { eprintln!("{e}"); std::process::exit(1); } }
    }
    if args.iter().any(|a| a == "--startmenue-entfernen") { println!("{}", selbst::disintegrate()); return Ok(()); }
    if args.iter().any(|a| a == "--kuerzel") { kuerzel::show(); return Ok(()); }
    if args.iter().any(|a| a == "--kuerzel-einrichten") { std::process::exit(kuerzel::setup()); }
    if args.iter().any(|a| a == "--kuerzel-entfernen") { std::process::exit(kuerzel::remove()); }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("WS.Video {VERSION}\n\nAufruf:\n  ws-video [projekt.wsvideo | ordner | medien …]\n                                          Programm starten, Projekt (Datei oder Ordner) oder Medien öffnen\n  ws-video --record                       Aufnahme-Fenster öffnen\n  ws-video --start                        Aufnahme sofort starten, mit den letzten Einstellungen\n  ws-video --toggle                       Aufnahme starten, läuft schon eine: beenden (für ein Tastenkürzel)\n  ws-video --stop                         laufende Aufnahme beenden\n  ws-video --pause                        laufende Aufnahme pausieren oder fortsetzen\n  ws-video --kuerzel-einrichten           Tastenkürzel für Start, Stopp und Pause in Hyprland eintragen\n  ws-video --kuerzel                      die Zeilen dafür nur anzeigen\n  ws-video --kuerzel-entfernen            die eingetragenen Tastenkürzel wieder entfernen\n  ws-video --render projekt.wsvideo ausgabe.mp4 [höhe] [h264|hevc|av1] [soft]\n                                          Projekt ohne Oberfläche exportieren\n  ws-video --fehlerbericht                Fehlerbericht mit Angaben zu System und Hardware in den Ordner Downloads schreiben\n  ws-video --startmenue                   AppImage ins Startmenü eintragen (nur AppImage-Fassung)\n  ws-video --startmenue-entfernen         den Eintrag wieder entfernen");
        return Ok(());
    }
    gtk::init()?;
    glib::set_application_name("WS.Video");
    glib::set_prgname(Some(APP_ID));
    let gapp = gtk::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE).build();
    // Strg+Q schließt das Hauptfenster wie das X
    gapp.connect_startup(|a| a.set_accels_for_action("window.close", &["<Ctrl>q"]));
    let state: Rc<RefCell<Option<Rc<ui::App>>>> = Rc::new(RefCell::new(None));
    // Protokoll nur im Hauptprogramm, und erst, wenn es wirklich startet: Ein Kurzaufruf wie `--stop` reicht
    // seine Bitte nur weiter oder endet sofort, er darf das Protokoll nicht anfassen.
    let crashed: Rc<RefCell<Option<std::path::PathBuf>>> = Rc::new(RefCell::new(None));
    gapp.connect_command_line(move |gapp, cl| {
        let args: Vec<String> = cl.arguments().iter().skip(1).map(|a| a.to_string_lossy().to_string()).collect();
        let cwd = cl.cwd();
        let running = state.borrow().is_some();
        let control = args.iter().any(|a| a == "--stop" || a == "--pause");
        if control && !running {
            // nichts läuft: ein Tastenkürzel soll das Programm nicht starten
            eprintln!("ws-video: Es läuft gerade keine Aufnahme.");
            return glib::ExitCode::SUCCESS;
        }
        let app = state.borrow_mut().get_or_insert_with(|| {
            *crashed.borrow_mut() = crashlog::install();
            crashlog::watch();
            let a = ui::App::new(gapp);
            a.show_start();
            a
        }).clone();
        let mut media = Vec::new();
        let starter = args.iter().any(|a| a == "--start" || a == "--toggle");
        let mut present = !control && !starter;
        for a in &args {
            match a.as_str() {
                "--stop" => app.command("stop"),
                "--pause" => app.command("pause"),
                "--start" => app.command("start"),
                "--toggle" => app.command("toggle"),
                "--record" => { app.command("record"); present = false; }
                other if !other.starts_with("--") => {
                    let mut p = std::path::PathBuf::from(other);
                    if p.is_relative() { if let Some(c) = &cwd { p = c.join(p); } }
                    if p.extension().map(|e| e == model::FILE_EXT).unwrap_or(false) || (p.is_dir() && ui::project_file_in(&p).is_some()) { app.open_project(&p); } else if p.exists() { media.push(p.to_string_lossy().to_string()); }
                }
                _ => {}
            }
        }
        if !media.is_empty() { app.open_media(&media); }
        if present { app.window.present(); }
        if let Some(f) = crashed.borrow_mut().take() {
            let d = gtk::AlertDialog::builder().message("WS.Video wurde beim letzten Mal unerwartet beendet").detail(format!("Was dabei passiert ist, steht in\n{}\n\nEin Fehlerbericht fasst das Protokoll und die Angaben zu System und Hardware in einer Datei zusammen, die du weitergeben kannst. Im Terminal geht das auch mit „ws-video --fehlerbericht“.", f.display())).modal(true).buttons(["Schließen", "Fehlerbericht erstellen"]).cancel_button(0).default_button(1).build();
            let win = app.window.clone();
            d.choose(Some(&app.window), gio::Cancellable::NONE, move |r| {
                if r == Ok(1) {
                    match crashlog::report() {
                        Ok(p) => ui::message(&win, "Fehlerbericht erstellt", &format!("Die Datei liegt hier:\n{}\n\n{}", p.display(), bericht::NOTICE)),
                        Err(e) => ui::message(&win, "Der Fehlerbericht konnte nicht geschrieben werden", &e.to_string()),
                    }
                }
            });
        } else if present {
            // AppImage: einmal je Datei anbieten, das Programm ins Startmenü einzutragen
            if let Some(a) = selbst::appimage() {
                let path = a.to_string_lossy().to_string();
                if !selbst::integrated() && app.settings.borrow().menu_asked != path {
                    { let mut st = app.settings.borrow_mut(); st.menu_asked = path; st.save(); }
                    let d = gtk::AlertDialog::builder().message("WS.Video ins Startmenü eintragen?").detail("Dann findest du WS.Video wie jedes andere Programm im Startmenü, und Projektdateien öffnen sich per Doppelklick. Die AppImage-Datei muss dafür an ihrem Platz bleiben. Später geht das auch mit dem Befehl „--startmenue“.").modal(true).buttons(["Nicht jetzt", "Eintragen"]).cancel_button(0).default_button(1).build();
                    let win = app.window.clone();
                    d.choose(Some(&app.window), gio::Cancellable::NONE, move |r| {
                        if r == Ok(1) {
                            match selbst::integrate() {
                                Ok(t) => ui::message(&win, "WS.Video ist eingetragen", &t),
                                Err(e) => ui::message(&win, "Das Eintragen hat nicht geklappt", &e),
                            }
                        }
                    });
                }
            }
        }
        glib::ExitCode::SUCCESS
    });
    fonts::add_bundled();
    let code = gapp.run();
    // Ordentliches Ende vermerken. (Früher hing das an `is_registered`, das nach `run` aber schon wieder falsch
    // ist: Deshalb galt jedes normale Schließen beim nächsten Start als Absturz.)
    crashlog::finish();
    if code != glib::ExitCode::SUCCESS { anyhow::bail!("WS.Video wurde mit einem Fehler beendet."); }
    Ok(())
}
