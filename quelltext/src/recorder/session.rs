//! Die eigentliche Aufnahme: eine GStreamer-Pipeline mit gemeinsamer Uhr, jede Quelle in eine eigene Datei.
//! Dazu die Mauszeiger-Daten als eigene Datenspur.
use super::{devices, hypr, input};
use crate::cursordata::{guess_monitor, CursorData};
use crate::engine::{self, Frame};
use gst::prelude::*;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct RecOptions {
    pub screen: bool,
    pub cursor_data: bool,
    /// Gerätepfad der Kamera
    pub camera: Option<String>,
    /// Pulse-Gerätename (leer = Standard)
    pub mic: Option<String>,
    pub system: Option<String>,
    pub fps: u32,
    pub hardware: bool,
    /// Größe der Bildschirmaufnahme
    pub size: RecSize,
}

/// Größe der Bildschirmaufnahme: Ein sehr großer Bildschirm (4K) kostet beim Kodieren, beim späteren Dekodieren
/// und in jedem Effekt ein Mehrfaches an Rechenzeit. Verkleinert wird im selben Schritt wie die Farbumwandlung.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RecSize {
    /// Originalgröße; nur ohne Grafikkarten-Encoder auf Rechnern mit weniger als acht Kernen höchstens 1440p
    Auto,
    Original,
    /// kürzere Bildseite höchstens so viele Bildpunkte
    Max(u32),
}

impl RecSize {
    pub fn from_setting(s: &str) -> RecSize {
        match s { "original" => RecSize::Original, "1440" => RecSize::Max(1440), "1080" => RecSize::Max(1080), _ => RecSize::Auto }
    }
    /// Zielgröße für einen Bildschirm der Größe (w, h); None = Originalgröße.
    pub fn target(self, w: u32, h: u32, on_gpu: bool) -> Option<(u32, u32)> {
        let max = match self {
            RecSize::Original => return None,
            RecSize::Max(m) => m.max(360),
            RecSize::Auto => {
                let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
                if on_gpu || cores >= 8 { return None; }
                1440
            }
        };
        let short = w.min(h);
        if short <= max { return None; }
        let even = |v: u64| ((v as u32 + 1) / 2 * 2).max(2);
        Some((even(w as u64 * max as u64 / short as u64), even(h as u64 * max as u64 / short as u64)))
    }
}

/// Verbindung zur gewählten Bildschirmquelle für genau eine Pipeline.
pub struct ScreenInput {
    pub fd: Option<OwnedFd>,
    pub node: u32,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
    pub cursor_hidden: bool,
    /// Name des Monitors in Hyprland (leer = unbekannt, wird dann aus Größe und Lage erraten)
    pub monitor: String,
}

/// Zeigerposition der Testquelle: mit `follow` wandert der Zeiger zwischen zwei nebeneinanderliegenden Testmonitoren.
fn fake_cursor(t: f64, follow: bool) -> (f64, f64) {
    if follow { (1920.0 + 1500.0 * (t / 1.6).cos(), 540.0 + 250.0 * (t * 1.5).sin()) } else { (960.0 + 400.0 * (t * 1.5).cos(), 540.0 + 250.0 * (t * 1.5).sin()) }
}
fn fake_monitors(follow: bool) -> Vec<crate::cursordata::Monitor> {
    let m = |name: &str, x: f64| crate::cursordata::Monitor { name: name.into(), x, y: 0.0, w: 1920.0, h: 1080.0, pw: 1920, ph: 1080, scale: 1.0 };
    if follow { vec![m("TEST-1", 0.0), m("TEST-2", 1920.0)] } else { vec![m("TEST-1", 0.0)] }
}

/// Ein Versuch: welcher Encoder, mit oder ohne Bildraten-Vorgabe an PipeWire.
#[derive(Clone, Debug, PartialEq)]
pub struct Attempt {
    pub encoder: String,
    pub on_gpu: bool,
    pub limit_rate: bool,
}

pub fn attempts(hardware: bool) -> Vec<Attempt> {
    let mut out = Vec::new();
    let soft = engine::pick_encoder(engine::Codec::H264, false);
    if hardware {
        if let Some((e, true)) = engine::pick_encoder(engine::Codec::H264, true) {
            out.push(Attempt { encoder: e.clone(), on_gpu: true, limit_rate: true });
            out.push(Attempt { encoder: e, on_gpu: true, limit_rate: false });
        }
    }
    if let Some((e, _)) = soft {
        // Auch der Rückfall auf den Software-Encoder bittet die Quelle zuerst um die gewünschte Bildrate:
        // Sonst liefert sie mit der Bildwiederholrate des Monitors, und alles Überzählige wäre vergebliche Arbeit.
        out.push(Attempt { encoder: e.clone(), on_gpu: false, limit_rate: true });
        out.push(Attempt { encoder: e, on_gpu: false, limit_rate: false });
    }
    out
}

const NONE: u64 = u64::MAX;
const SCREEN: usize = 0;
const CAMERA: usize = 1;
const MIC: usize = 2;
const SYSTEM: usize = 3;
const NAMES: [&str; 4] = ["scr", "cam", "mic", "sys"];
/// Bildrate, auf die die Kamera begrenzt wird
const CAM_FPS: u32 = 30;

/// Zähler je Bildquelle (Bildschirm, Kamera) für die Diagnose.
#[derive(Default)]
struct VideoStats {
    /// von der Quelle geliefert (außerhalb von Pausen)
    seen: AtomicU64,
    /// nach der Begrenzung der Bildrate behalten
    kept: AtomicU64,
    /// hinter der Entkopplung angekommen; was gegenüber `kept` fehlt, hat der Rechner nicht geschafft
    passed: AtomicU64,
    /// vom Encoder ausgegeben
    encoded: AtomicU64,
    /// Zeitstempel der Quelle fehlte oder war unbrauchbar: Ankunftszeit verwendet
    restamped: AtomicU64,
    /// größter Abstand zweier behaltener Bilder (ns)
    max_gap: AtomicU64,
    /// größte Verzögerung zwischen Aufnahmezeitpunkt und Ankunft (ns)
    max_delay: AtomicU64,
    /// Bildratenbegrenzung: Zeitpunkt (ns Dateizeit), ab dem das nächste Bild fällig ist
    next: AtomicU64,
}

struct Shared {
    video: [VideoStats; 2],
    paused: AtomicBool,
    pause_start: AtomicU64,
    resume_at: AtomicU64,
    pause_total: AtomicU64,
    first: [AtomicU64; 4],
    last: [AtomicU64; 4],
    stop: AtomicBool,
}

impl Shared {
    /// Dateizeit des Bildschirm-Videos zur Laufzeit `rt` (ns), None solange pausiert oder noch kein Bild da ist.
    fn file_time(&self, rt: u64) -> Option<f64> {
        if self.paused.load(Ordering::SeqCst) { return None; }
        let first = self.first[SCREEN].load(Ordering::SeqCst);
        if first == NONE { return None; }
        let t = rt.checked_sub(self.pause_total.load(Ordering::SeqCst))?.checked_sub(first)?;
        Some(t as f64 / 1e9)
    }
}

#[derive(Clone, Debug, Default)]
pub struct RecFile {
    pub path: String,
    /// Versatz gegenüber dem frühesten Stream (Sekunden)
    pub offset: f64,
}

#[derive(Clone, Debug, Default)]
pub struct RecResult {
    pub dir: PathBuf,
    pub screen: Option<RecFile>,
    pub camera: Option<RecFile>,
    pub mic: Option<RecFile>,
    pub system: Option<RecFile>,
    pub cursor: Option<String>,
    pub duration: f64,
    pub encoder: String,
    pub notes: Vec<String>,
}

pub struct Recording {
    pipeline: gst::Pipeline,
    shared: Arc<Shared>,
    pub dir: PathBuf,
    files: [Option<String>; 4],
    levels: [f32; 2],
    cam_frame: Arc<Mutex<Option<Frame>>>,
    cursor: Arc<Mutex<CursorData>>,
    threads: Vec<std::thread::JoinHandle<()>>,
    pub attempt: Attempt,
    pub notes: Vec<String>,
    pub error: Option<String>,
    /// Name des Elements, das den Fehler gemeldet hat (scr, cam, mic, sys, scr_enc …)
    pub error_source: String,
    pub clicks_available: bool,
    cursor_active: bool,
    /// Eckdaten für die Diagnose
    info: RecInfo,
    _fd: Vec<OwnedFd>,
    /// „Aktivem Bildschirm folgen“: gerade gezeigte Quelle und Zahl der Wechsel
    follow: Option<(Arc<std::sync::atomic::AtomicUsize>, Arc<AtomicU64>)>,
}

#[derive(Clone, Debug, Default)]
struct RecInfo {
    fps: u32,
    screen_src: (u32, u32),
    screen_rec: (u32, u32),
    camera: (u32, u32),
    nominal: bool,
    calm: bool,
}

/// Rechenlastige Aufnahme-Threads (Farbumwandlung, Encoder) laufen mit etwas niedrigerer Priorität:
/// Wird die Rechenzeit knapp, sollen Fenstermanager, Ton und das aufgenommene Programm flüssig bleiben.
/// Die Aufnahme lässt dann einzelne Bilder aus, statt den ganzen Rechner ins Stocken zu bringen.
mod calm {
    use gst::glib;
    use gst::subclass::prelude::*;

    const NICE: libc::c_int = 5;

    #[derive(Default)]
    pub struct Imp;

    #[glib::object_subclass]
    impl ObjectSubclass for Imp {
        const NAME: &'static str = "WsCalmTaskPool";
        type Type = Pool;
        type ParentType = gst::TaskPool;
    }
    impl ObjectImpl for Imp {}
    impl GstObjectImpl for Imp {}
    impl TaskPoolImpl for Imp {
        type Handle = Handle;
        fn push(&self, func: gst::subclass::TaskPoolFunction) -> Result<Option<Handle>, glib::Error> {
            // Eigene Threads statt des gemeinsamen Vorrats: Die Priorität lässt sich ohne Sonderrechte nicht wieder
            // anheben, ein wiederverwendeter Thread bliebe also dauerhaft gebremst.
            let t = std::thread::Builder::new().name("ws-aufnahme".into()).spawn(move || {
                unsafe {
                    let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
                    libc::setpriority(libc::PRIO_PROCESS as _, tid, NICE);
                }
                func.call();
            });
            match t {
                Ok(t) => Ok(Some(Handle(Some(t)))),
                Err(e) => Err(glib::Error::new(gst::CoreError::Failed, &format!("Thread konnte nicht gestartet werden: {e}"))),
            }
        }
    }

    pub struct Handle(Option<std::thread::JoinHandle<()>>);
    impl gst::TaskHandle for Handle {
        fn join(mut self) {
            if let Some(t) = self.0.take() { let _ = t.join(); }
        }
    }

    glib::wrapper! {
        pub struct Pool(ObjectSubclass<Imp>) @extends gst::TaskPool, gst::Object;
    }

    impl Pool {
        pub fn new() -> Pool { glib::Object::new() }
    }
}

/// Elemente, deren Threads rechnen (hinter diesen Warteschlangen laufen Umwandlung und Encoder).
const CALM: [&str; 4] = ["scr_q", "scr_q2", "cam_q", "cam_q2"];

/// Farbformat, in dem der Encoder die Bilder bekommt. Ohne Vorgabe handelt die Umwandlung bei x264enc das
/// verlustärmste Format aus (4:4:4), das doppelt so viel Rechenzeit kostet und kaum ein Abspieler flüssig zeigt.
fn encoder_format(enc: &str) -> &'static str {
    if enc.starts_with("nv") { "" }                    // NVENC nimmt die Bildschirmdaten direkt und wandelt auf der Grafikkarte
    else if enc == "openh264enc" { "format=I420" }
    else { "format=NV12" }
}

fn has_element(name: &str) -> bool { gst::ElementFactory::find(name).is_some() }

/// Breite und Höhe aus einem Caps-Text lesen.
fn caps_size(text: &str) -> Option<(u32, u32)> {
    let num = |key: &str| text.split(',').find_map(|f| f.trim().strip_prefix(key).and_then(|v| v.trim().parse::<u32>().ok()));
    Some((num("width=")?, num("height=")?))
}

pub fn safe_set(el: &gst::Element, name: &str, value: &str) -> bool {
    let Some(pspec) = el.find_property(name) else { return false };
    match gst::glib::Value::deserialize_with_pspec(value, &pspec) {
        Ok(v) => { el.set_property_from_value(name, &v); true }
        Err(_) => false,
    }
}

fn record_bitrate(w: u32, h: u32, fps: u32) -> u32 {
    let px = (w.max(640) as f64 * h.max(360) as f64) / (1920.0 * 1080.0);
    (16000.0 * px.powf(0.85) * (fps as f64 / 30.0).powf(0.6)).round() as u32
}

pub fn new_session_dir() -> PathBuf {
    let now = gst::glib::DateTime::now_local().ok().and_then(|d| d.format("%Y-%m-%d_%H-%M-%S").ok()).map(|s| s.to_string()).unwrap_or_else(|| "neu".into());
    let base = crate::settings::videos_dir();
    let mut dir = base.join(format!("Aufnahme_{now}"));
    let mut n = 2;
    while dir.exists() { dir = base.join(format!("Aufnahme_{now}_{n}")); n += 1; }
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn fake() -> bool { std::env::var_os("WS_FAKE").is_some() }

/// Testquelle für Messungen ohne Wayland: Die Umgebungsvariable `var` kann eine eigene GStreamer-Beschreibung
/// enthalten, deren letztes Element den Namen der Quelle bekommt.
fn fake_source(var: &str, name: &str, default: &str) -> String {
    match std::env::var(var) {
        Ok(d) if !d.trim().is_empty() => format!("{} name={name}", d.trim()),
        _ => default.to_string(),
    }
}

impl Recording {
    /// `screens`: keine, eine oder mehrere Bildschirmquellen. Bei mehreren zeigt die Aufnahme immer den Monitor,
    /// auf dem der Mauszeiger gerade ist („aktivem Bildschirm folgen“).
    pub fn start(opts: &RecOptions, screens: Vec<ScreenInput>, attempt: &Attempt) -> anyhow::Result<Recording> {
        let fake_follow = fake() && std::env::var_os("WS_FAKE_FOLLOW").is_some();
        let follow = opts.screen && (if fake() { fake_follow } else { screens.len() >= 2 });
        let dir = new_session_dir();
        let fps = opts.fps.clamp(5, 60);
        let mut desc = String::new();
        let mut files: [Option<String>; 4] = [None, None, None, None];
        let mut notes = Vec::new();
        let enc = &attempt.encoder;
        let parse = if has_element("h264parse") { "! h264parse " } else { "" };
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let conv = cores.clamp(1, 4);
        let pin = encoder_format(enc);
        // Die Quellen liefern nur dann Bilder, wenn sich etwas ändert. Der Encoder bekommt trotzdem eine Nenn-Bildrate
        // (für Schlüsselbildabstand und Mengensteuerung); die wirklichen Zeitpunkte stehen an jedem einzelnen Bild.
        let nominal = has_element("capssetter");
        let rate_caps = |f: u32| if nominal { format!(" ! capssetter caps=\"video/x-raw,framerate={f}/1\"") } else { String::new() };
        // Entkopplung gleich hinter der Quelle: Der Aufnahme-Thread wird nie aufgehalten. Kommt der Rechner nicht mit,
        // fällt hier das jeweils älteste noch unbearbeitete Bild weg, alle übrigen behalten ihren wahren Zeitpunkt.
        let decouple = |name: &str| format!("queue name={name} leaky=downstream max-size-buffers=2 max-size-time=0 max-size-bytes=0");
        let before_enc = |name: &str| format!("queue name={name} max-size-buffers=3 max-size-time=0 max-size-bytes=0");
        let after_enc = "queue max-size-buffers=0 max-size-time=0 max-size-bytes=67108864";
        let mut info = RecInfo { fps, nominal, ..Default::default() };
        let mut screen_size = (1920u32, 1080u32);
        let mut cursor_hidden = false;
        let mut fd_keep: Vec<OwnedFd> = Vec::new();
        let mut position = None;
        let mut monitor_name = String::new();
        // Monitore der Quellen (nur beim Folgen): Lage im Layout, um den Zeiger zuzuordnen
        let mut src_mon: Vec<crate::cursordata::Monitor> = Vec::new();

        if opts.screen {
            let path = dir.join("bildschirm.mkv").to_string_lossy().to_string();
            // Quellen: eine heißt `scr`; beim Folgen heißen sie `scr0`, `scr1` … und `scr` ist der Umschalter dahinter.
            let mut srcs: Vec<String> = Vec::new();
            if fake() {
                if follow {
                    srcs.push("videotestsrc name=scr0 is-live=true pattern=ball ! video/x-raw,width=1920,height=1080,framerate=30/1".to_string());
                    srcs.push("videotestsrc name=scr1 is-live=true pattern=smpte ! video/x-raw,width=1920,height=1080,framerate=30/1".to_string());
                } else {
                    srcs.push(fake_source("WS_FAKE_SCREEN", "scr", "videotestsrc name=scr is-live=true pattern=ball ! video/x-raw,width=1920,height=1080,framerate=30/1"));
                }
            } else {
                if screens.is_empty() { anyhow::bail!("Keine Bildschirmquelle gewählt."); }
                let limit = if attempt.limit_rate { format!(" ! video/x-raw,max-framerate={fps}/1") } else { String::new() };
                for (k, s) in screens.iter().enumerate() {
                    let fd = s.fd.as_ref().ok_or_else(|| anyhow::anyhow!("Keine Verbindung zu PipeWire."))?;
                    let name = if follow { format!("scr{k}") } else { "scr".to_string() };
                    // keepalive: Bei stehendem Bild kommt viermal je Sekunde das letzte Bild erneut. So endet die Datei
                    // zeitgleich mit dem Ton, und nach einer Pause oder einem Bildschirmwechsel steht sofort der
                    // aktuelle Bildschirminhalt da.
                    srcs.push(format!("pipewiresrc name={name} fd={} path={} do-timestamp=true keepalive-time=250 resend-last=true{}", fd.as_raw_fd(), s.node, limit));
                    if !follow { break; }
                }
            }
            let mut known = false;
            let mut differ = false;
            for (k, s) in screens.iter().enumerate() {
                if let Some((w, h)) = s.size {
                    let sz = (w.max(2) as u32, h.max(2) as u32);
                    if !known { screen_size = sz; known = true; }
                    else if follow && sz != screen_size {
                        differ = true;
                        if sz.0 as u64 * sz.1 as u64 > screen_size.0 as u64 * screen_size.1 as u64 { screen_size = sz; }
                    }
                }
                if k == 0 {
                    cursor_hidden = s.cursor_hidden;
                    position = s.position;
                    monitor_name = s.monitor.clone();
                }
                if !follow { break; }
            }
            if follow {
                let all = if fake() { fake_monitors(true) } else { hypr::monitors() };
                let names: Vec<String> = if fake() { vec!["TEST-1".into(), "TEST-2".into()] } else { screens.iter().map(|s| s.monitor.clone()).collect() };
                for n in &names {
                    match all.iter().find(|m| &m.name == n) {
                        Some(m) => src_mon.push(m.clone()),
                        None => anyhow::bail!("Für „Aktivem Bildschirm folgen“ muss bei jedem Bildschirm der Monitor gewählt sein (unbekannt: „{n}“)."),
                    }
                }
                if fake() { cursor_hidden = true; }
            }
            let mut target = if known { opts.size.target(screen_size.0, screen_size.1, attempt.on_gpu) } else { None };
            // Unterschiedlich große Monitore: alles auf die Größe des größten bringen, sonst wechselt die Bildgröße mitten in der Datei
            if follow && differ && target.is_none() { target = Some((screen_size.0 / 2 * 2, screen_size.1 / 2 * 2)); }
            let src = if follow { "input-selector name=scr sync-streams=false".to_string() } else { srcs[0].clone() };
            let convert = match target {
                Some(_) if has_element("videoconvertscale") => format!("videoconvertscale n-threads={conv}"),
                Some(_) => format!("videoscale n-threads={conv} ! videoconvert n-threads={conv}"),
                None => format!("videoconvert n-threads={conv}"),
            };
            let mut fields: Vec<String> = Vec::new();
            if !pin.is_empty() { fields.push(pin.to_string()); }
            if let Some((w, h)) = target { fields.push(format!("width={w},height={h},pixel-aspect-ratio=1/1")); }
            let caps = if fields.is_empty() { String::new() } else { format!(" ! video/x-raw,{}", fields.join(",")) };
            desc += &format!(
                "{src} ! {} ! {convert}{caps}{} ! {} ! {enc} name=scr_enc {parse}! {after_enc} ! matroskamux ! filesink location=\"{path}\"\n",
                decouple("scr_q"), rate_caps(fps), before_enc("scr_q2")
            );
            if follow {
                for (k, s) in srcs.iter().enumerate() {
                    desc += &format!("{s} ! queue leaky=downstream max-size-buffers=1 max-size-time=0 max-size-bytes=0 ! scr.sink_{k}\n");
                }
            }
            info.screen_src = screen_size;
            info.screen_rec = target.unwrap_or(screen_size);
            files[SCREEN] = Some(path);
        }
        for s in screens { if let Some(fd) = s.fd { fd_keep.push(fd); } }

        if let Some(cam) = &opts.camera {
            let path = dir.join("kamera.mkv").to_string_lossy().to_string();
            // Quelle, danach die Entkopplung, erst dann das Dekodieren (MJPEG) und Umwandeln
            let (src, decode, size) = if fake() {
                (fake_source("WS_FAKE_CAM", "cam", "videotestsrc name=cam is-live=true pattern=smpte ! video/x-raw,width=1280,height=720,framerate=30/1"), "", (0, 0))
            } else {
                match devices::camera_format(cam) {
                    Some((caps, jpeg)) => (format!("v4l2src name=cam device={cam} ! {caps}"), if jpeg { " ! jpegdec" } else { "" }, caps_size(&caps).unwrap_or((0, 0))),
                    None => (format!("v4l2src name=cam device={cam}"), " ! decodebin", (0, 0)),
                }
            };
            let caps = if pin.is_empty() { String::new() } else { format!(" ! video/x-raw,{pin}") };
            desc += &format!(
                "{src} ! {}{decode} ! videoconvert{caps}{} ! tee name=camt \
                 camt. ! {} ! {enc} name=cam_enc {parse}! {after_enc} ! matroskamux ! filesink location=\"{path}\" \
                 camt. ! queue leaky=downstream max-size-buffers=1 ! videoscale ! videoconvert ! video/x-raw,format=BGRx,height=180,pixel-aspect-ratio=1/1 ! appsink name=campreview drop=true max-buffers=1 sync=false\n",
                decouple("cam_q"), rate_caps(CAM_FPS), before_enc("cam_q2")
            );
            info.camera = size;
            files[CAMERA] = Some(path);
        }
        let audio = |name: &str, device: &str, file: &str, fake_src: &str| -> String {
            let src = if fake() {
                format!("{fake_src} name={name} is-live=true")
            } else if device.is_empty() {
                format!("pulsesrc name={name} provide-clock=false do-timestamp=true client-name=WS.Video")
            } else {
                format!("pulsesrc name={name} device=\"{device}\" provide-clock=false do-timestamp=true client-name=WS.Video")
            };
            format!(
                "{src} ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! level name={name}_level interval=60000000 post-messages=true ! queue ! opusenc bitrate=160000 ! queue ! matroskamux ! filesink location=\"{file}\"\n"
            )
        };
        if let Some(mic) = &opts.mic {
            let path = dir.join("mikrofon.mka").to_string_lossy().to_string();
            desc += &audio("mic", mic, &path, "audiotestsrc wave=ticks");
            files[MIC] = Some(path);
        }
        if let Some(sys) = &opts.system {
            let path = dir.join("systemton.mka").to_string_lossy().to_string();
            desc += &audio("sys", sys, &path, "audiotestsrc freq=330 volume=0.05");
            files[SYSTEM] = Some(path);
        }
        if desc.is_empty() { anyhow::bail!("Es ist keine Quelle eingeschaltet."); }
        if std::env::var_os("WS_DEBUG").is_some() { eprintln!("ws-video: Aufnahme-Pipeline:\n{desc}"); }

        let pipeline = gst::parse::launch(&desc).map_err(|e| anyhow::anyhow!("Aufnahme-Pipeline konnte nicht gebaut werden: {e}"))?;
        let pipeline = pipeline.downcast::<gst::Pipeline>().map_err(|_| anyhow::anyhow!("Pipeline-Fehler"))?;
        pipeline.use_clock(Some(&gst::SystemClock::obtain()));

        // Encoder einstellen. Bei x264enc ist die Datenrate nur eine Obergrenze (feste Qualität), siehe `encoder_settings`.
        let cam_size = if info.camera.0 > 0 { info.camera } else { (1280, 720) };
        for (name, (w, h), f) in [("scr_enc", info.screen_rec, fps), ("cam_enc", cam_size, CAM_FPS)] {
            let Some(e) = pipeline.by_name(name) else { continue };
            let kbps = record_bitrate(w, h, f);
            let kbps = if attempt.on_gpu { kbps * 6 / 10 } else { kbps };
            for (k, v) in engine::encoder_settings(enc, kbps, f, true) { safe_set(&e, &k, &v); }
            if enc == "x264enc" && name == "cam_enc" {
                safe_set(&e, "quantizer", "23");
                // Das kleine Kamerabild soll dem Bildschirm keine Rechenzeit streitig machen.
                if opts.screen { safe_set(&e, "threads", "2"); }
            }
        }

        let shared = Arc::new(Shared {
            video: [VideoStats::default(), VideoStats::default()],
            paused: AtomicBool::new(false),
            pause_start: AtomicU64::new(0),
            resume_at: AtomicU64::new(0),
            pause_total: AtomicU64::new(0),
            first: [AtomicU64::new(NONE), AtomicU64::new(NONE), AtomicU64::new(NONE), AtomicU64::new(NONE)],
            last: [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)],
            stop: AtomicBool::new(false),
        });

        // Zeitstempel je Quelle: Pausen herausrechnen, jede Datei beginnt bei 0.
        // Bei den Bildquellen außerdem: unbrauchbare Zeitstempel ersetzen, Bildrate begrenzen, mitzählen.
        for (i, name) in NAMES.iter().enumerate() {
            let Some(el) = pipeline.by_name(name) else { continue };
            let Some(pad) = el.static_pad("src") else { continue };
            let sh = shared.clone();
            let video = i == SCREEN || i == CAMERA;
            let period = 1_000_000_000u64 / if i == SCREEN { fps } else { CAM_FPS } as u64;
            pad.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
                let Some(gst::PadProbeData::Buffer(ref mut buffer)) = info.data else { return gst::PadProbeReturn::Ok };
                let mut pts = buffer.pts().map(|t| t.nseconds());
                if video {
                    // Die Quelle stempelt mit dem Aufnahmezeitpunkt. Fehlt der Stempel oder liegt er weit neben der
                    // Uhr der Aufnahme (anderer Zeitmaßstab), zählt der Zeitpunkt der Ankunft.
                    let now = pad.parent_element().and_then(|e| e.current_running_time()).map(|t| t.nseconds());
                    if let Some(now) = now {
                        match pts {
                            Some(p) if p <= now + 250_000_000 && p + 1_000_000_000 >= now => {
                                sh.video[i].max_delay.fetch_max(now.saturating_sub(p), Ordering::Relaxed);
                            }
                            _ => {
                                pts = Some(now);
                                sh.video[i].restamped.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
                let Some(pts) = pts else { return gst::PadProbeReturn::Ok };
                if sh.paused.load(Ordering::SeqCst) && pts >= sh.pause_start.load(Ordering::SeqCst) { return gst::PadProbeReturn::Drop; }
                if pts < sh.resume_at.load(Ordering::SeqCst) { return gst::PadProbeReturn::Drop; }
                let Some(t) = pts.checked_sub(sh.pause_total.load(Ordering::SeqCst)) else { return gst::PadProbeReturn::Drop };
                let mut first = sh.first[i].load(Ordering::SeqCst);
                let is_first = first == NONE;
                if is_first { first = t; }
                let Some(new) = t.checked_sub(first) else { return gst::PadProbeReturn::Drop };
                let last = sh.last[i].load(Ordering::SeqCst);
                if video {
                    let st = &sh.video[i];
                    st.seen.fetch_add(1, Ordering::Relaxed);
                    if !is_first {
                        if new <= last { return gst::PadProbeReturn::Drop; }
                        // Bildrate begrenzen, ohne Zeitstempel zu verändern: Ein Bild, das deutlich vor dem nächsten
                        // fälligen Zeitpunkt kommt, entfällt. Bilder im Raster bleiben trotz kleiner Schwankungen alle.
                        let next = st.next.load(Ordering::Relaxed);
                        if new + period * 3 / 10 < next { return gst::PadProbeReturn::Drop; }
                        st.next.store(if new > next + period { new + period } else { next + period }, Ordering::Relaxed);
                        st.max_gap.fetch_max(new - last, Ordering::Relaxed);
                    } else {
                        st.next.store(period, Ordering::Relaxed);
                    }
                    st.kept.fetch_add(1, Ordering::Relaxed);
                } else if new < last {
                    return gst::PadProbeReturn::Drop;
                }
                if is_first { sh.first[i].store(first, Ordering::SeqCst); }
                sh.last[i].store(new, Ordering::SeqCst);
                let b = buffer.make_mut();
                b.set_pts(gst::ClockTime::from_nseconds(new));
                b.set_dts(gst::ClockTime::NONE);
                gst::PadProbeReturn::Ok
            });
        }
        // Zählstellen hinter der Entkopplung und hinter dem Encoder
        for (i, q, e) in [(SCREEN, "scr_q", "scr_enc"), (CAMERA, "cam_q", "cam_enc")] {
            if let Some(pad) = pipeline.by_name(q).and_then(|el| el.static_pad("src")) {
                let sh = shared.clone();
                pad.add_probe(gst::PadProbeType::BUFFER, move |_, _| { sh.video[i].passed.fetch_add(1, Ordering::Relaxed); gst::PadProbeReturn::Ok });
            }
            if let Some(pad) = pipeline.by_name(e).and_then(|el| el.static_pad("src")) {
                let sh = shared.clone();
                pad.add_probe(gst::PadProbeType::BUFFER, move |_, _| { sh.video[i].encoded.fetch_add(1, Ordering::Relaxed); gst::PadProbeReturn::Ok });
            }
        }
        // Die rechnenden Threads bekommen eine niedrigere Priorität (siehe `calm`).
        if std::env::var_os("WS_NO_NICE").is_none() {
            if let Some(bus) = pipeline.bus() {
                let pool = calm::Pool::new();
                bus.set_sync_handler(move |_, msg| {
                    if let gst::MessageView::StreamStatus(s) = msg.view() {
                        let (kind, owner) = s.get();
                        if kind == gst::StreamStatusType::Create && CALM.contains(&owner.name().as_str()) {
                            if let Some(task) = s.stream_status_object().and_then(|v| v.get::<gst::Task>().ok()) { task.set_pool(&pool); }
                        }
                    }
                    gst::BusSyncReply::Pass
                });
                info.calm = true;
            }
        }

        // Kamerabild für die Steuerleiste
        let cam_frame: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
        if let Some(sink) = pipeline.by_name("campreview").and_then(|e| e.downcast::<gst_app::AppSink>().ok()) {
            let slot = cam_frame.clone();
            sink.set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |s| {
                        if let Ok(sample) = s.pull_sample() {
                            if let Some(f) = engine::sample_to_frame(&sample) { *slot.lock().unwrap() = Some(f); }
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );
        }

        // Mauszeiger-Daten
        let cursor = Arc::new(Mutex::new(CursorData { version: 1, monitor: -1, stream_w: screen_size.0, stream_h: screen_size.1, ..Default::default() }));
        let mut threads_v = Vec::new();
        let mut clicks_available = false;
        let cursor_active = opts.screen && opts.cursor_data && cursor_hidden && (hypr::available() || fake());
        let t_start = std::time::Instant::now();
        // Aktivem Bildschirm folgen: Der Umschalter zeigt die Quelle, auf deren Monitor der Zeiger steht.
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let switches = Arc::new(AtomicU64::new(0));
        if follow {
            if let Some(sel) = pipeline.by_name("scr") {
                let (sh, act, sw, mons) = (shared.clone(), active.clone(), switches.clone(), src_mon.clone());
                threads_v.push(std::thread::spawn(move || {
                    let (mut cand, mut count) = (0usize, 0u32);
                    while !sh.stop.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        let pos = if fake() { Some(fake_cursor(t_start.elapsed().as_secs_f64(), true)) } else { hypr::cursor_pos() };
                        let Some((x, y)) = pos else { continue };
                        // Steht der Zeiger auf einem Monitor, der nicht aufgenommen wird, bleibt das Bild, wo es ist.
                        let Some(k) = mons.iter().position(|m| x >= m.x && x < m.x + m.w && y >= m.y && y < m.y + m.h) else { count = 0; continue };
                        if k == act.load(Ordering::SeqCst) { count = 0; continue; }
                        if k != cand { cand = k; count = 0; }
                        count += 1;
                        // erst nach einer Fünftelsekunde umschalten, damit kurzes Überfahren nichts auslöst
                        if count >= 4 {
                            if let Some(pad) = sel.static_pad(&format!("sink_{k}")) {
                                sel.set_property("active-pad", &pad);
                                act.store(k, Ordering::SeqCst);
                                sw.fetch_add(1, Ordering::SeqCst);
                            }
                            count = 0;
                        }
                    }
                }));
            }
        }
        // Zeigerposition in die Koordinaten der Aufnahme bringen. Beim Folgen gibt es einen gedachten Monitor in der
        // Größe des ersten; steht der Zeiger nicht auf dem gerade gezeigten Monitor, liegt er weit außerhalb.
        let map_pos: Arc<dyn Fn(f64, f64) -> (f64, f64) + Send + Sync> = if follow && !src_mon.is_empty() {
            let (mons, act) = (src_mon.clone(), active.clone());
            Arc::new(move |x, y| {
                let m = &mons[act.load(Ordering::SeqCst).min(mons.len() - 1)];
                if x >= m.x && x < m.x + m.w && y >= m.y && y < m.y + m.h {
                    ((x - m.x) / m.w.max(1.0) * mons[0].w, (y - m.y) / m.h.max(1.0) * mons[0].h)
                } else { (-10000.0, -10000.0) }
            })
        } else { Arc::new(|x, y| (x, y)) };
        if cursor_active {
            let monitors = if follow {
                let m = &src_mon[0];
                vec![crate::cursordata::Monitor { name: "Aktiver Bildschirm".into(), x: 0.0, y: 0.0, w: m.w, h: m.h, pw: m.pw, ph: m.ph, scale: m.scale }]
            } else if fake() { fake_monitors(false) } else { hypr::monitors() };
            {
                let mut c = cursor.lock().unwrap();
                c.monitor = if follow { 0 } else { -1 };
                // ausdrücklich gewählter Monitor geht vor
                if c.monitor < 0 && !monitor_name.is_empty() { c.monitor = monitors.iter().position(|m| m.name == monitor_name).map(|i| i as i32).unwrap_or(-1); }
                // Lage laut Portal zählt nur, wenn auch die Größe passt: manche Portale melden immer (0, 0)
                if c.monitor < 0 {
                    if let Some((x, y)) = position {
                        c.monitor = monitors.iter().position(|m| (m.x - x as f64).abs() < 1.0 && (m.y - y as f64).abs() < 1.0 && m.pw == screen_size.0 && m.ph == screen_size.1).map(|i| i as i32).unwrap_or(-1);
                    }
                }
                if c.monitor < 0 { c.monitor = guess_monitor(&monitors, screen_size.0, screen_size.1).map(|i| i as i32).unwrap_or(-1); }
                c.monitors = monitors;
            }
            let last_pos = Arc::new(Mutex::new((0.0f64, 0.0f64)));
            // Position 60-mal je Sekunde
            {
                let (sh, pl, cur, lp, map) = (shared.clone(), pipeline.clone(), cursor.clone(), last_pos.clone(), map_pos.clone());
                threads_v.push(std::thread::spawn(move || {
                    while !sh.stop.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_micros(16_600));
                        let pos = if fake() { Some(fake_cursor(t_start.elapsed().as_secs_f64(), follow)) } else { hypr::cursor_pos() };
                        let Some((x, y)) = pos else { continue };
                        let (x, y) = map(x, y);
                        *lp.lock().unwrap() = (x, y);
                        let Some(rt) = pl.current_running_time() else { continue };
                        let Some(t) = sh.file_time(rt.nseconds()) else { continue };
                        let mut c = cur.lock().unwrap();
                        let n = c.samples.len();
                        // Ruhephasen brauchen nur Anfang und Ende
                        if n >= 2 && c.samples[n - 1][1] == x && c.samples[n - 1][2] == y && c.samples[n - 2][1] == x && c.samples[n - 2][2] == y {
                            c.samples[n - 1][0] = t;
                        } else {
                            c.samples.push([t, x, y]);
                        }
                    }
                }));
            }
            // Klicks
            let mice = input::open_mice();
            clicks_available = !mice.is_empty();
            if clicks_available {
                let (sh, pl, cur, lp, map) = (shared.clone(), pipeline.clone(), cursor.clone(), last_pos.clone(), map_pos.clone());
                threads_v.push(std::thread::spawn(move || {
                    while !sh.stop.load(Ordering::SeqCst) {
                        for button in input::poll_clicks(&mice, 100) {
                            let Some(rt) = pl.current_running_time() else { continue };
                            let Some(t) = sh.file_time(rt.nseconds()) else { continue };
                            let (x, y) = hypr::cursor_pos().map(|(x, y)| map(x, y)).unwrap_or(*lp.lock().unwrap());
                            cur.lock().unwrap().clicks.push([t, button as f64, x, y]);
                        }
                    }
                }));
            } else {
                notes.push("Mausklicks werden nicht aufgezeichnet: kein Lesezugriff auf /dev/input (Benutzer in die Gruppe „input“ aufnehmen, siehe LIESMICH).".to_string());
            }
        } else if opts.screen && opts.cursor_data && !fake() {
            notes.push("Der Mauszeiger ist fest im Bild: getrennte Zeigerdaten gibt es nur unter Hyprland.".to_string());
        }

        pipeline.set_state(gst::State::Playing).map_err(|_| anyhow::anyhow!("Die Aufnahme konnte nicht gestartet werden (Encoder {enc})."))?;
        Ok(Recording {
            pipeline, shared, dir, files, levels: [0.0, 0.0], cam_frame, cursor, threads: threads_v, attempt: attempt.clone(), notes,
            error: None, error_source: String::new(), clicks_available, cursor_active, info, _fd: fd_keep,
            follow: if follow { Some((active, switches)) } else { None },
        })
    }

    /// Meldungen der Pipeline abholen (Pegel, Fehler). Regelmäßig aus der Oberfläche aufrufen.
    pub fn poll(&mut self) {
        let Some(bus) = self.pipeline.bus() else { return };
        while let Some(msg) = bus.pop() {
            match msg.view() {
                gst::MessageView::Error(e) => {
                    if self.error.is_none() {
                        let src = msg.src().map(|s| s.name().to_string()).unwrap_or_default();
                        self.error = Some(e.error().to_string());
                        self.error_source = src;
                    }
                }
                gst::MessageView::Element(el) => {
                    let Some(s) = el.structure() else { continue };
                    if s.name() != "level" { continue; }
                    let idx = match msg.src().map(|s| s.name().to_string()).as_deref() { Some("mic_level") => 0, Some("sys_level") => 1, _ => continue };
                    if let Ok(arr) = s.get::<gst::glib::ValueArray>("peak") {
                        let mut peak: f64 = -100.0;
                        for v in arr.iter() { if let Ok(db) = v.get::<f64>() { peak = peak.max(db); } }
                        self.levels[idx] = (10f64.powf(peak / 20.0)).clamp(0.0, 1.0) as f32;
                    }
                }
                _ => {}
            }
        }
    }

    /// Sind von allen eingeschalteten Quellen Daten angekommen?
    pub fn has_data(&self) -> bool {
        (0..4).all(|i| self.files[i].is_none() || self.shared.first[i].load(Ordering::SeqCst) != NONE)
    }
    pub fn screen_has_data(&self) -> bool { self.files[SCREEN].is_none() || self.shared.first[SCREEN].load(Ordering::SeqCst) != NONE }
    pub fn missing_sources(&self) -> Vec<&'static str> {
        let labels = ["Bildschirm", "Kamera", "Mikrofon", "Systemton"];
        (0..4).filter(|i| self.files[*i].is_some() && self.shared.first[*i].load(Ordering::SeqCst) == NONE).map(|i| labels[i]).collect()
    }

    fn running_ns(&self) -> u64 { self.pipeline.current_running_time().map(|t| t.nseconds()).unwrap_or(0) }

    pub fn pause(&self) {
        if self.shared.paused.load(Ordering::SeqCst) { return; }
        self.shared.pause_start.store(self.running_ns(), Ordering::SeqCst);
        self.shared.paused.store(true, Ordering::SeqCst);
    }
    pub fn resume(&self) {
        if !self.shared.paused.load(Ordering::SeqCst) { return; }
        let now = self.running_ns();
        let start = self.shared.pause_start.load(Ordering::SeqCst);
        self.shared.pause_total.fetch_add(now.saturating_sub(start), Ordering::SeqCst);
        self.shared.resume_at.store(now, Ordering::SeqCst);
        self.shared.paused.store(false, Ordering::SeqCst);
    }
    pub fn is_paused(&self) -> bool { self.shared.paused.load(Ordering::SeqCst) }

    /// Aufgenommene Zeit ohne Pausen (Sekunden)
    pub fn elapsed(&self) -> f64 {
        let now = if self.is_paused() { self.shared.pause_start.load(Ordering::SeqCst) } else { self.running_ns() };
        now.saturating_sub(self.shared.pause_total.load(Ordering::SeqCst)) as f64 / 1e9
    }
    pub fn levels(&self) -> (f32, f32) { (self.levels[0], self.levels[1]) }
    pub fn cam_frame(&self) -> Option<Frame> { self.cam_frame.lock().unwrap().clone() }
    /// Beim Folgen: Nummer des gerade gezeigten Bildschirms (ab 1), sonst None.
    pub fn active_screen(&self) -> Option<usize> { self.follow.as_ref().map(|f| f.0.load(Ordering::SeqCst) + 1) }
    pub fn has_camera(&self) -> bool { self.files[CAMERA].is_some() }
    pub fn has_mic(&self) -> bool { self.files[MIC].is_some() }
    pub fn has_system(&self) -> bool { self.files[SYSTEM].is_some() }

    /// Pipeline beenden. Mit `wait_eos` werden die Dateien sauber abgeschlossen; liefert, ob das gelungen ist.
    fn shutdown(&mut self, wait_eos: bool) -> bool {
        let mut clean = !wait_eos;
        if wait_eos {
            self.pipeline.send_event(gst::event::Eos::new());
            if let Some(bus) = self.pipeline.bus() {
                // Der Abschluss dauert gewöhnlich Bruchteile einer Sekunde, weil nirgends ein Rückstau entstehen kann.
                // Kommen nach acht Sekunden immer noch Bilder aus dem Encoder, wird weiter gewartet, solange es vorangeht.
                let start = std::time::Instant::now();
                let encoded = |sh: &Shared| sh.video.iter().map(|v| v.encoded.load(Ordering::Relaxed)).sum::<u64>();
                let mut progress = (encoded(&self.shared), std::time::Instant::now());
                loop {
                    if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
                        match msg.view() {
                            gst::MessageView::Eos(_) => { clean = true; break; }
                            gst::MessageView::Error(e) => { if self.error.is_none() { self.error = Some(e.error().to_string()); } break; }
                            _ => {}
                        }
                    }
                    let waited = start.elapsed().as_secs_f64();
                    if waited > 8.0 {
                        let n = encoded(&self.shared);
                        if n != progress.0 { progress = (n, std::time::Instant::now()); }
                        if progress.1.elapsed().as_secs_f64() > 3.0 || waited > 45.0 { break; }
                    }
                }
            }
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(bus) = self.pipeline.bus() { bus.unset_sync_handler(); }
        self.shared.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) { let _ = t.join(); }
        clean
    }

    /// Aufnahme beenden und Dateien abschließen.
    pub fn stop(mut self) -> RecResult {
        let duration = self.elapsed();
        let clean = self.shutdown(true);
        let firsts: Vec<Option<f64>> = (0..4).map(|i| {
            let f = self.shared.first[i].load(Ordering::SeqCst);
            if self.files[i].is_some() && f != NONE { Some(f as f64 / 1e9) } else { None }
        }).collect();
        let min = firsts.iter().flatten().cloned().fold(f64::INFINITY, f64::min);
        let mut notes = std::mem::take(&mut self.notes);
        let labels = ["Bildschirm", "Kamera", "Mikrofon", "Systemton"];
        let file = |i: usize, notes: &mut Vec<String>| -> Option<RecFile> {
            let path = self.files[i].clone()?;
            match firsts[i] {
                Some(f) if std::fs::metadata(&path).map(|m| m.len() > 1000).unwrap_or(false) => Some(RecFile { path, offset: f - min }),
                _ => {
                    notes.push(format!("{}: es sind keine Daten angekommen.", labels[i]));
                    let _ = std::fs::remove_file(&path);
                    None
                }
            }
        };
        let screen = file(SCREEN, &mut notes);
        let camera = file(CAMERA, &mut notes);
        let mic = file(MIC, &mut notes);
        let system = file(SYSTEM, &mut notes);
        let mut cursor_path = None;
        if self.cursor_active && screen.is_some() {
            let c = self.cursor.lock().unwrap();
            if !c.samples.is_empty() {
                let p = self.dir.join("mauszeiger.json").to_string_lossy().to_string();
                if c.save(&p).is_ok() { cursor_path = Some(p); }
            }
        }
        if let Some(e) = &self.error { notes.push(format!("Während der Aufnahme trat ein Fehler auf: {e}")); }
        if let Some((_, sw)) = &self.follow { notes.push(format!("Aktivem Bildschirm gefolgt: {} Wechsel.", sw.load(Ordering::SeqCst))); }
        if !clean && self.error.is_none() {
            notes.push("Die Aufnahme ließ sich nicht sauber abschließen, am Ende können Bilder fehlen.".to_string());
        }
        let present = [screen.is_some(), camera.is_some(), mic.is_some(), system.is_some()];
        self.diagnose(duration, clean, &present, &mut notes);
        RecResult { dir: self.dir.clone(), screen, camera, mic, system, cursor: cursor_path, duration, encoder: self.attempt.encoder.clone(), notes }
    }

    /// Diagnose der Aufnahme: schreibt `diagnose.txt` in den Aufnahmeordner und ergänzt Hinweise, wenn etwas
    /// nicht in Ordnung war (Rechner überlastet, Kamera liefert zu wenige Bilder, Bild und Ton enden ungleich).
    fn diagnose(&self, duration: f64, clean: bool, present: &[bool; 4], notes: &mut Vec<String>) {
        let sh = &self.shared;
        let labels = ["Bildschirm", "Kamera", "Mikrofon", "Systemton"];
        let de = |v: f64, n: usize| format!("{v:.n$}").replace('.', ",");
        let mut out = String::new();
        let now = gst::glib::DateTime::now_local().ok().and_then(|d| d.format("%d.%m.%Y %H:%M").ok()).map(|s| s.to_string()).unwrap_or_default();
        out += &format!("WS.Video {} – Diagnose der Aufnahme vom {now}\n", env!("CARGO_PKG_VERSION"));
        out += &format!("Ordner: {}\n", self.dir.display());
        out += &format!("Encoder: {} ({}), Bildrate von der Quelle {}\n", self.attempt.encoder, if self.attempt.on_gpu { "Grafikkarte" } else { "Software" }, if self.attempt.limit_rate { "angefordert" } else { "nicht angefordert" });
        out += &format!("Rechner: {} Prozessorkerne, {}\n", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0), gst::version_string());
        out += &format!("Dauer ohne Pausen: {} s, Abschluss: {}\n", de(duration, 2), if clean { "sauber" } else { "NICHT sauber (Datei unvollständig)" });
        out += &format!("Rechnende Threads mit niedrigerer Priorität: {}, Nenn-Bildrate für den Encoder: {}\n", if self.info.calm { "ja" } else { "nein" }, if self.info.nominal { "ja" } else { "nein" });
        // Ende jeder Spur auf der gemeinsamen Uhr (Sekunden seit Start der Aufnahme, ohne Pausen)
        let mut ends: Vec<(usize, f64)> = Vec::new();
        for i in 0..4 {
            if self.files[i].is_none() { continue; }
            out += &format!("\n{}\n", labels[i]);
            let first = sh.first[i].load(Ordering::SeqCst);
            if !present[i] || first == NONE {
                out += "  keine Daten angekommen\n";
                continue;
            }
            let last = sh.last[i].load(Ordering::SeqCst) as f64 / 1e9;
            let end = first as f64 / 1e9 + last;
            ends.push((i, end));
            let size = self.files[i].as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0);
            if i == SCREEN || i == CAMERA {
                let st = &sh.video[i];
                let (seen, kept, passed, encoded) = (st.seen.load(Ordering::Relaxed), st.kept.load(Ordering::Relaxed), st.passed.load(Ordering::Relaxed), st.encoded.load(Ordering::Relaxed));
                // Bis zu zwei Bilder dürfen beim Beenden noch in der Entkopplung liegen.
                let lost = kept.saturating_sub(passed);
                let lost = if lost <= 2 { 0 } else { lost };
                let pct = if kept > 0 { lost as f64 * 100.0 / kept as f64 } else { 0.0 };
                let rate = if last > 0.5 { encoded as f64 / last } else { 0.0 };
                let (limit, size_text) = if i == SCREEN {
                    let (s, r) = (self.info.screen_src, self.info.screen_rec);
                    (self.info.fps, if s == r { format!("{}×{}", s.0, s.1) } else { format!("{}×{} (Bildschirm {}×{})", r.0, r.1, s.0, s.1) })
                } else {
                    (CAM_FPS, if self.info.camera.0 > 0 { format!("{}×{}", self.info.camera.0, self.info.camera.1) } else { "Größe laut Kamera".to_string() })
                };
                out += &format!("  Größe: {size_text}, höchstens {limit} Bilder/s\n");
                out += &format!("  von der Quelle geliefert:            {seen}\n");
                out += &format!("  wegen der Bildrate ausgelassen:      {}\n", seen.saturating_sub(kept));
                out += &format!("  vom Rechner nicht geschafft:         {lost} ({} %)\n", de(pct, 1));
                out += &format!("  kodiert:                             {encoded} ({} Bilder/s im Mittel)\n", de(rate, 1));
                out += &format!("  größter Abstand zweier Bilder:       {} s\n", de(st.max_gap.load(Ordering::Relaxed) as f64 / 1e9, 2));
                out += &format!("  größte Verzögerung bei der Ankunft:  {} ms\n", de(st.max_delay.load(Ordering::Relaxed) as f64 / 1e6, 0));
                out += &format!("  Zeitstempel ersetzt:                 {}\n", st.restamped.load(Ordering::Relaxed));
                if lost > 3 && pct > 2.0 {
                    notes.push(format!(
                        "Der Rechner kam bei der Aufnahme nicht mit: {lost} von {kept} Bildern ({}) fehlen ({} %). Abhilfe im Aufnahmefenster: kleinere Aufnahmegröße, 30 statt 60 Bilder/s oder der Encoder der Grafikkarte.",
                        labels[i], de(pct, 0)
                    ));
                }
                if i == CAMERA && last > 5.0 && (kept as f64 / last) < 20.0 {
                    notes.push(format!(
                        "Die Kamera lieferte nur {} Bilder/s. Viele Kameras senken bei wenig Licht die Bildrate: mehr Licht hilft, oder die Belichtungsautomatik der Kamera ausschalten.",
                        de(kept as f64 / last, 0)
                    ));
                }
            }
            out += &format!("  Beginn {} s nach dem Start, letzter Zeitstempel {} s, Datei {} MB\n", de(first as f64 / 1e9, 3), de(last, 2), de(size as f64 / 1e6, 1));
        }
        // Enden die Spuren gemeinsam? (Ein stehendes Bild wird viermal je Sekunde wiederholt, mehr Abstand ist ein Fehler.)
        if let (Some(max), Some(min)) = (ends.iter().map(|e| e.1).reduce(f64::max), ends.iter().map(|e| e.1).reduce(f64::min)) {
            if max - min > 1.0 {
                let short: Vec<&str> = ends.iter().filter(|e| max - e.1 > 1.0).map(|e| labels[e.0]).collect();
                notes.push(format!("{} endet {} s vor den anderen Spuren (Einzelheiten in diagnose.txt im Aufnahmeordner).", short.join(" und "), de(max - min, 1)));
            }
        }
        if !notes.is_empty() {
            out += "\nHinweise\n";
            for n in notes.iter() { out += &format!("  - {n}\n"); }
        }
        let _ = std::fs::write(self.dir.join("diagnose.txt"), out);
    }

    /// Aufnahme verwerfen und Dateien löschen.
    pub fn discard(mut self) {
        self.shutdown(false);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}
