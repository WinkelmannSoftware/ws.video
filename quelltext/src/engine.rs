//! Schnitt-Engine: baut aus dem Projektmodell eine Zeitleiste der GStreamer Editing Services (GES),
//! liefert die Vorschau und rendert den Export.
use crate::annot;
use crate::model::*;
use ges::prelude::*;
use gst::glib;
use gst_controller::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub fn init() -> anyhow::Result<()> {
    gst::init()?;
    ges::init()?;
    crate::elements::register()?;
    Ok(())
}

pub fn ct(t: f64) -> gst::ClockTime { gst::ClockTime::from_nseconds((t.max(0.0) * 1e9).round() as u64) }
pub fn secs(t: gst::ClockTime) -> f64 { t.nseconds() as f64 / 1e9 }
pub fn path_uri(path: &str) -> String {
    glib::filename_to_uri(path, None).map(|s| s.to_string()).unwrap_or_else(|_| format!("file://{path}"))
}

fn set_child<V: ToValue>(te: &impl IsA<ges::TimelineElement>, name: &str, v: V) {
    let _ = ges::prelude::TimelineElementExtManual::set_child_property(te, name, &v.to_value());
}
/// Zuletzt gesetzter Stand einer steuerbaren Eigenschaft (Lage, Größe, Deckkraft, Lautstärke).
#[derive(Default)]
struct Ctl {
    sig: String,
    cs: Option<gst_controller::InterpolationControlSource>,
    stamps: Vec<u64>,
}
type Sent = std::cell::RefCell<HashMap<String, Ctl>>;

/// Festen Wert (eine Stützstelle) oder Kurve einer Eigenschaft setzen, und zwar nur, wenn sich etwas geändert hat.
///
/// Zwei Fehler der Schnittbibliothek bestimmen, wie das geschieht:
/// Die Wiedergabe liest die Liste der Bindungen eines Elements ohne Sperre (`gst_object_sync_values`). Wird
/// währenddessen eine Bindung entfernt, greift sie auf freigegebenen Speicher zu und das Programm stürzt ab.
/// Deshalb wird eine Bindung nie entfernt oder ersetzt: Sie bleibt, und nur ihre Stützstellen ändern sich
/// (die Kurve selbst ist gegen gleichzeitigen Zugriff gesichert). Ein fester Wert ist dann eine flache Kurve.
fn set_ctl(sent: &Sent, te: &ges::TrackElement, name: &str, kind: &str, pts: &[(u64, f64)], int: bool) {
    const FAR: u64 = 1_000_000_000_000_000;
    if pts.is_empty() { return; }
    let key = format!("{:p}|{name}", te.as_ptr());
    let sig = format!("{pts:?}");
    let mut map = sent.borrow_mut();
    let e = map.entry(key).or_default();
    if e.sig == sig { return; }
    e.sig = sig;
    if pts.len() == 1 && e.cs.is_none() {
        if int { set_child(te, name, pts[0].1.round() as i32); } else { set_child(te, name, pts[0].1); }
        return;
    }
    let flat = [(0u64, pts[0].1), (FAR, pts[0].1)];
    let pts: &[(u64, f64)] = if pts.len() == 1 { &flat } else { pts };
    let fresh = e.cs.is_none();
    let cs = e.cs.get_or_insert_with(|| {
        let cs = gst_controller::InterpolationControlSource::new();
        cs.set_mode(gst_controller::InterpolationMode::Linear);
        cs
    });
    for (t, v) in pts { cs.set(gst::ClockTime::from_nseconds(*t), *v); }
    for old in &e.stamps {
        if !pts.iter().any(|(t, _)| t == old) { cs.unset(gst::ClockTime::from_nseconds(*old)); }
    }
    e.stamps = pts.iter().map(|(t, _)| *t).collect();
    if fresh { te.set_control_source(&*cs, name, kind); }
}

/// Frisch angelegten Clip für Änderungen im laufenden Betrieb vorbereiten.
///
/// Setzt man Breite oder Höhe, hält die Schnittbibliothek die Sperre ihres Positionierers fest und meldet dabei
/// eine Änderung an dessen Formatfilter die Behälter hinauf. Schaltet gerade ein anderer Thread denselben
/// Behälter um, warten beide für immer aufeinander: Das Programm friert ein. Die Meldungen des Formatfilters
/// braucht niemand, sie bleiben deshalb dauerhaft angehalten.
fn harden(clip: &ges::Clip) {
    for te in track_elements(clip) {
        te.set_auto_clamp_control_sources(false);
        if !te.is::<ges::VideoSource>() { continue; }
        let mut n = 0;
        if let Some((el, _)) = ges::prelude::TrackElementExt::lookup_child(&te, "width") {
            if let Some(bin) = el.parent().and_then(|p| p.downcast::<gst::Bin>().ok()) {
                for ch in bin.children() {
                    if ch.factory().map(|f| f.name() == "capsfilter").unwrap_or(false) {
                        unsafe { glib::gobject_ffi::g_object_freeze_notify(ch.as_ptr() as *mut glib::gobject_ffi::GObject); }
                        n += 1;
                    }
                }
            }
        }
        if n == 0 && std::env::var_os("WS_DEBUG").is_some() { eprintln!("ws-video: kein Formatfilter neben dem Positionierer gefunden"); }
    }
}

fn track_elements(c: &ges::Clip) -> Vec<ges::TrackElement> {
    c.children(false).into_iter().filter_map(|c| c.downcast::<ges::TrackElement>().ok()).collect()
}

// ---------------------------------------------------------------------------------------------
// Medien-Informationen

#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub has_video: bool,
    pub has_audio: bool,
    pub is_image: bool,
}

pub fn probe(path: &str) -> anyhow::Result<MediaInfo> {
    let asset = ges::UriClipAsset::request_sync(&path_uri(path))?;
    let info = asset.info();
    let mut mi = MediaInfo { is_image: asset.is_image(), ..Default::default() };
    mi.duration = asset.duration().map(secs).unwrap_or(0.0);
    if let Some(v) = info.video_streams().first() {
        mi.has_video = true;
        mi.width = v.width();
        mi.height = v.height();
        if v.is_image() { mi.is_image = true; }
    }
    mi.has_audio = !info.audio_streams().is_empty();
    if mi.is_image { mi.duration = 0.0; mi.has_audio = false; }
    if !mi.has_video && !mi.has_audio { anyhow::bail!("Datei enthält weder Bild noch Ton"); }
    Ok(mi)
}

/// Ein BGRx-Bild (Vorschau, Vorschaubilder).
#[derive(Clone)]
pub struct Frame {
    pub w: i32,
    pub h: i32,
    pub stride: i32,
    pub data: Vec<u8>,
}

pub fn sample_to_frame(sample: &gst::Sample) -> Option<Frame> {
    let caps = sample.caps()?;
    let info = gst_video::VideoInfo::from_caps(caps).ok()?;
    let buf = sample.buffer()?;
    let map = buf.map_readable().ok()?;
    let stride = info.stride()[0];
    let need = stride as usize * info.height() as usize;
    if map.len() < need { return None; }
    Some(Frame { w: info.width() as i32, h: info.height() as i32, stride, data: map[..need].to_vec() })
}

/// Einzelbild einer Datei holen (für Medienliste und Zeitleiste).
pub fn thumbnail(path: &str, at: f64, height: i32) -> Option<Frame> {
    let desc = format!(
        "uridecodebin uri=\"{}\" ! videoconvert ! videoscale ! video/x-raw,format=BGRx,height={},pixel-aspect-ratio=1/1 ! appsink name=sink sync=false max-buffers=1",
        path_uri(path), height
    );
    let pipeline = gst::parse::launch(&desc).ok()?;
    let bin = pipeline.clone().downcast::<gst::Bin>().ok()?;
    let sink = bin.by_name("sink")?.downcast::<gst_app::AppSink>().ok()?;
    pipeline.set_state(gst::State::Paused).ok()?;
    let (res, _, _) = pipeline.state(gst::ClockTime::from_seconds(5));
    let mut frame = None;
    if res.is_ok() {
        if at > 0.0 {
            let _ = pipeline.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, ct(at));
            let _ = pipeline.state(gst::ClockTime::from_seconds(5));
        }
        if let Some(sample) = sink.try_pull_preroll(gst::ClockTime::from_seconds(3)) {
            frame = sample_to_frame(&sample);
        }
    }
    let _ = pipeline.set_state(gst::State::Null);
    frame
}

/// Lautstärke-Spitzen einer Datei (100 Werte je Sekunde) für die Wellenform.
pub fn waveform(path: &str) -> Option<Vec<f32>> {
    let desc = format!(
        "uridecodebin uri=\"{}\" caps=\"audio/x-raw\" expose-all-streams=false ! audioconvert ! audioresample ! audio/x-raw,format=S16LE,channels=1,rate=8000 ! appsink name=sink sync=false",
        path_uri(path)
    );
    let pipeline = gst::parse::launch(&desc).ok()?;
    let bin = pipeline.clone().downcast::<gst::Bin>().ok()?;
    let sink = bin.by_name("sink")?.downcast::<gst_app::AppSink>().ok()?;
    pipeline.set_state(gst::State::Playing).ok()?;
    let mut peaks: Vec<f32> = Vec::new();
    let mut cur: i32 = 0;
    let mut n = 0;
    while let Ok(sample) = sink.pull_sample() {
        let Some(buf) = sample.buffer() else { continue };
        let Ok(map) = buf.map_readable() else { continue };
        for ch in map.chunks_exact(2) {
            let v = (i16::from_le_bytes([ch[0], ch[1]]) as i32).abs();
            if v > cur { cur = v; }
            n += 1;
            if n == 80 {
                peaks.push(cur as f32 / 32768.0);
                cur = 0;
                n = 0;
            }
        }
    }
    let _ = pipeline.set_state(gst::State::Null);
    Some(peaks)
}

/// Ton einer Datei messen: Rauschprofil aus den leisesten Stellen, mittlere Lautstärke, Spitze.
pub fn analyse_audio(path: &str) -> Option<crate::audiofx::Profile> {
    let desc = format!(
        "uridecodebin uri=\"{}\" caps=\"audio/x-raw\" expose-all-streams=false ! audioconvert ! audioresample ! audio/x-raw,format=F32LE,channels=1,rate=48000 ! appsink name=sink sync=false",
        path_uri(path)
    );
    let pipeline = gst::parse::launch(&desc).ok()?;
    let bin = pipeline.clone().downcast::<gst::Bin>().ok()?;
    let sink = bin.by_name("sink")?.downcast::<gst_app::AppSink>().ok()?;
    pipeline.set_state(gst::State::Playing).ok()?;
    let mut a = crate::audiofx::Analyzer::new();
    let mut chunk: Vec<f32> = Vec::new();
    while let Ok(sample) = sink.pull_sample() {
        let Some(buf) = sample.buffer() else { continue };
        let Ok(map) = buf.map_readable() else { continue };
        chunk.clear();
        chunk.extend(map.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
        a.push(&chunk);
    }
    let _ = pipeline.set_state(gst::State::Null);
    a.finish()
}

/// Einstellungen der Tonbearbeitung eines Clips als Text für `wsaudio`.
fn afx_desc(p: &Project, c: &Clip) -> String {
    let a = &c.audio;
    let mut q = crate::audiofx::Params {
        lowcut: if a.lowcut { 80.0 } else { 0.0 },
        gate: a.gate as f32, compress: a.compress as f32, voice: a.voice, bass: a.bass as f32, treble: a.treble as f32,
        ..Default::default()
    };
    if let Some(pr) = p.audio_profiles.get(&c.path) {
        q.floor = pr.floor;
        q.loud = pr.loud;
        if !pr.noise.is_empty() && a.denoise > 0.001 {
            q.denoise = a.denoise as f32;
            q.profile = c.path.clone();
        }
        if a.normalize && pr.loud > -70.0 { q.gain = (crate::audiofx::TARGET_DB - pr.loud).clamp(-12.0, 30.0); }
    }
    q.to_text()
}

// ---------------------------------------------------------------------------------------------
// Aufbau der GES-Zeitleiste aus dem Modell

struct Node {
    clip: ges::Clip,
    shadow: Option<ges::Clip>,
    key: String,
    /// Lage in der Zeitleiste (Ebene, Beginn, Einstieg, Dauer), wie sie zuletzt gesetzt wurde
    place: String,
    props: String,
    sent: Sent,
}

#[derive(Clone, Copy, PartialEq)]
enum Geo {
    /// Medien: Zuschnitt vergrößert das volle Bild über das sichtbare Rechteck hinaus
    Media,
    /// Anmerkung oder Schatten mit Rand
    Padded(f64),
}

struct Desired {
    key: String,
    place: String,
    props: String,
    uri: String,
    /// Anmerkung: Pfad der gerenderten PNG-Datei (läuft über einen Testclip mit `wsimage`, nicht über eine Mediendatei)
    image: Option<String>,
    layer: usize,
    types: ges::TrackType,
    start: f64,
    inpoint: f64,
    duration: f64,
    geo: Geo,
    fx: String,
    shadow: Option<(String, f64)>,
    regions: String,
    hidden: bool,
    muted: bool,
    xf: XFade,
    /// Tonbearbeitung als Text für `wsaudio` (leer = keine)
    afx: String,
}

/// Überblendung mit den Nachbarn auf derselben Spur (Überlappung in Sekunden).
#[derive(Clone, Copy, Default, Debug)]
struct XFade {
    upper: bool,
    ov_in: f64,
    ov_out: f64,
    /// Bild blendet am Anfang bzw. Ende (der obere Clip blendet immer, der untere nur bei abweichender Lage)
    ramp_in: bool,
    ramp_out: bool,
}

/// Deckt `upper` zur Zeit `t` das Bild von `lower` vollständig ab? Dann darf `lower` während der
/// Überblendung deckend bleiben (sonst würde das Bild in der Mitte der Blende dunkler).
fn covers(p: &Project, upper: &Clip, lower: &Clip, t: f64) -> bool {
    if !upper.is_visual() || !lower.is_visual() { return true; }
    if upper.chroma || upper.opacity < 0.999 || upper.kind.is_annotation() { return false; }
    let (ux, uy, uw, uh) = upper.rect_at(t);
    let (lx, ly, lw, lh) = lower.rect_at(t);
    if upper.shape != Shape::Rect {
        // gleiche Form an gleicher Stelle deckt ebenfalls
        let near = |a: f64, b: f64| (a - b).abs() < 1.0;
        return upper.shape == lower.shape && near(ux, lx) && near(uy, ly) && near(uw, lw) && near(uh, lh);
    }
    // nur der Teil innerhalb der Arbeitsfläche zählt
    let (cx0, cy0, cx1, cy1) = (lx.max(0.0), ly.max(0.0), (lx + lw).min(p.width as f64), (ly + lh).min(p.height as f64));
    if cx1 <= cx0 || cy1 <= cy0 { return true; }
    ux <= cx0 + 0.5 && uy <= cy0 + 0.5 && ux + uw >= cx1 - 0.5 && uy + uh >= cy1 - 0.5
}

/// Überblendungen je Clip bestimmen: überlappende Nachbarn liegen abwechselnd oben und unten.
fn crossfades(p: &Project) -> HashMap<u64, XFade> {
    let mut out = HashMap::new();
    for tr in &p.tracks {
        let clips = p.clips_on(tr.id);
        let clips: Vec<&Clip> = clips.into_iter().filter(|c| c.kind != Kind::Blur).collect();
        let mut prev_upper = false;
        for (i, c) in clips.iter().enumerate() {
            let mut x = XFade::default();
            if i > 0 {
                let a = clips[i - 1];
                let ov = a.end() - c.start;
                if ov > 1e-6 {
                    x.ov_in = ov.min(c.duration);
                    x.upper = !prev_upper;
                    x.ramp_in = x.upper || !(covers(p, a, c, c.start) && covers(p, a, c, a.end() - 1e-3));
                }
            }
            if i + 1 < clips.len() {
                let b = clips[i + 1];
                let ov = c.end() - b.start;
                if ov > 1e-6 {
                    x.ov_out = ov.min(c.duration);
                    // der Nachfolger liegt auf der jeweils anderen Ebene
                    x.ramp_out = x.upper || !(covers(p, b, c, b.start) && covers(p, b, c, c.end() - 1e-3));
                }
            }
            prev_upper = x.upper;
            out.insert(c.id, x);
        }
    }
    out
}

/// Stützpunkte (Zeit relativ zum Clip-Anfang, Faktor 0..1) für Ein-/Ausblenden und Überblendungen.
fn envelope(c: &Clip, ramp_in: f64, ramp_out: f64) -> Vec<(f64, f64)> {
    let d = c.duration;
    let fi = if c.fade_in_kind == FadeKind::Blend { c.fade_in.clamp(0.0, d) } else { 0.0 };
    let fo = if c.fade_out_kind == FadeKind::Blend { c.fade_out.clamp(0.0, d) } else { 0.0 };
    let (ri, ro) = (ramp_in.clamp(0.0, d), ramp_out.clamp(0.0, d));
    let mut ts = vec![0.0, d];
    for t in [fi, d - fo, ri, d - ro] { if t > 0.0 && t < d { ts.push(t); } }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let up = |t: f64, len: f64| if len > 0.0 { (t / len).clamp(0.0, 1.0) } else { 1.0 };
    ts.into_iter().map(|t| (t, up(t, fi) * up(d - t, fo) * up(t, ri) * up(d - t, ro))).collect()
}

thread_local! {
    /// Bereits angefragte Medien: true = geladen, false = nicht lesbar.
    static ASSETS: std::cell::RefCell<HashMap<String, bool>> = std::cell::RefCell::new(HashMap::new());
}

/// Lädt ein Medium in den Zwischenspeicher von GES.
/// Achtung: Beim ersten Laden lässt GES die Hauptschleife weiterlaufen. Deshalb darf währenddessen
/// kein `RefCell` der Oberfläche ausgeliehen sein. Später greift `Builder::create` nur noch auf den Speicher zu.
pub fn preload_asset(uri: &str) -> bool {
    if let Some(ok) = ASSETS.with(|a| a.borrow().get(uri).copied()) { return ok; }
    let ok = ges::UriClipAsset::request_sync(uri).is_ok();
    ASSETS.with(|a| a.borrow_mut().insert(uri.to_string(), ok));
    ok
}

/// Alle Mediendateien, die das Projekt in der Schnitt-Engine braucht.
pub fn project_uris(p: &Project) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in &p.clips {
        if c.kind.is_media() {
            let u = path_uri(&c.path);
            if !out.contains(&u) { out.push(u); }
        }
    }
    out
}

/// Lädt alles vor, was das Projekt braucht (siehe `preload_asset`).
pub fn preload_project(p: &Project) {
    for u in project_uris(p) { preload_asset(&u); }
}

/// Nicht lesbare Medien beim nächsten Mal erneut versuchen (z. B. nach dem Öffnen eines Projekts).
pub fn retry_failed_assets() {
    ASSETS.with(|a| a.borrow_mut().retain(|_, ok| *ok));
}

pub struct Builder {
    pub timeline: ges::Timeline,
    layers: Vec<ges::Layer>,
    track_ids: Vec<u64>,
    dims: (u32, u32, u32),
    nodes: HashMap<u64, Node>,
    pub scale: f64,
    /// Teiler der Bildrate (nur Vorschau): 1 = volle Bildrate des Projekts, 2 = die halbe …
    pub fps_div: u32,
    pub offline: HashSet<u64>,
    /// Entfernte Clips, die noch nicht freigegeben werden dürfen (siehe `bury`).
    grave: Vec<Buried>,
    /// zuletzt gewählte Stufe der Vorverkleinerung je Clip (1.0 = keine)
    pre: HashMap<u64, f64>,
    /// Zahl der abgeschlossenen Übernahmen (commit) der Zeitleiste
    commits: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

struct Buried {
    since: std::time::Instant,
    commits: u64,
    _objects: Vec<glib::Object>,
}

fn full_rect(c: &Clip, r: (f64, f64, f64, f64), geo: Geo) -> (f64, f64, f64, f64) {
    match geo {
        Geo::Media => {
            let fw = r.2 / (1.0 - c.crop[0] - c.crop[2]).max(0.01);
            let fh = r.3 / (1.0 - c.crop[1] - c.crop[3]).max(0.01);
            (r.0 - c.crop[0] * fw, r.1 - c.crop[1] * fh, fw, fh)
        }
        Geo::Padded(p) => (r.0 - p, r.1 - p, r.2 + 2.0 * p, r.3 + 2.0 * p),
    }
}

/// Beschreibung für `elements::fx` (Farbe, Spiegeln, Vignette, im Bild gerechnete Übergänge).
fn fx_desc(c: &Clip, inpoint: f64) -> String {
    use crate::elements::fx as f;
    let code = |k: FadeKind| match k {
        FadeKind::Black => f::K_BLACK, FadeKind::White => f::K_WHITE, FadeKind::Zoom => f::K_ZOOM,
        FadeKind::WhipLeft => f::K_WHIP_L, FadeKind::WhipRight => f::K_WHIP_R, FadeKind::WhipUp => f::K_WHIP_U, FadeKind::WhipDown => f::K_WHIP_D,
        FadeKind::Blur => f::K_BLUR, FadeKind::Glitch => f::K_GLITCH,
        _ => f::K_NONE,
    };
    let d = c.duration;
    let (fi, fo) = (c.fade_in.clamp(0.0, d), c.fade_out.clamp(0.0, d));
    let (ki, ko) = (if fi > 0.0 { code(c.fade_in_kind) } else { f::K_NONE }, if fo > 0.0 { code(c.fade_out_kind) } else { f::K_NONE });
    format!(
        "{},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{:.4}",
        ki, inpoint, fi, ko, inpoint + d - fo, fo, c.brightness, c.contrast, c.saturation, c.temperature,
        (c.flip_h as u32) | ((c.flip_v as u32) << 1), c.vignette.clamp(0.0, 1.0)
    )
}

/// Beschreibung für das Auf- und Zusammenrollen (siehe `elements::roll`), leer = aus.
fn roll_desc(c: &Clip, inpoint: f64, geo: Geo) -> String {
    let d = c.duration;
    let fi = if c.fade_in_kind == FadeKind::Roll { c.fade_in.clamp(0.0, d) } else { 0.0 };
    let fo = if c.fade_out_kind == FadeKind::Roll { c.fade_out.clamp(0.0, d) } else { 0.0 };
    if fi <= 0.0 && fo <= 0.0 { return String::new(); }
    let (x0, x1) = match geo {
        Geo::Media => (c.crop[0], 1.0 - c.crop[2]),
        Geo::Padded(p) => { let full = (c.w + 2.0 * p).max(1.0); (p / full, 1.0 - p / full) }
    };
    format!("{:.4},{:.4},{:.4},{:.4},{:.5},{:.5}", inpoint, fi, inpoint + d - fo, fo, x0, x1)
}

/// Größe, auf die ein Medium vor den Effekten verkleinert wird (None = es bleibt, wie es ist).
/// Die Zeitleiste skaliert erst beim Mischen. Ohne diesen Schritt liefen Farbumwandlung, Effekte und Mischen
/// mit der vollen Quellgröße (etwa 4K), auch wenn das Bild viel kleiner gezeigt wird.
fn prescale_need(c: &Clip, scale: f64) -> Option<f64> {
    if !matches!(c.kind, Kind::Video | Kind::Image) { return None; }
    // Logos, Icons und andere kleine Bilder kosten ohnehin nichts: Sie bleiben unangetastet.
    if (c.src_w as u64) * (c.src_h as u64) < 640 * 360 || c.src_w < 64 || c.src_h < 64 { return None; }
    // größte Darstellung des vollen Bildes, auch während Zoom und Schwenk
    let mut need: f64 = 0.0;
    for r in std::iter::once((c.x, c.y, c.w, c.h)).chain(c.anims.iter().map(|a| (a.x, a.y, a.w, a.h))) {
        let f = full_rect(c, r, Geo::Media);
        need = need.max(f.2 * scale / c.src_w as f64).max(f.3 * scale / c.src_h as f64);
    }
    Some(need)
}

/// Feste Stufen der Verkleinerung: die kleinste, die für die Darstellung noch reicht (1.0 = unverkleinert).
fn prescale_step(need: f64) -> f64 {
    [0.75, 0.5, 0.375, 0.25, 0.1875, 0.125].into_iter().filter(|s| *s >= need).fold(1.0, f64::min)
}

fn prescale_size(c: &Clip, f: f64) -> (u32, u32) {
    let even = |v: f64| ((v / 2.0).round() as u32 * 2).max(2);
    (even(c.src_w as f64 * f), even(c.src_h as f64 * f))
}

/// Anfang der Effektkette: verkleinern und zugleich ins Farbformat der Effekte umwandeln, auf mehreren Kernen.
fn prescale_desc(w: u32, h: u32) -> String {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4);
    let scaler = if gst::ElementFactory::find("videoconvertscale").is_some() {
        format!("videoconvertscale add-borders=false n-threads={n}")
    } else {
        format!("videoscale add-borders=false n-threads={n} ! videoconvert n-threads={n}")
    };
    // Der Filter steht als eigenes Element da: Ein nacktes Format am Ende der Kette (ohne folgendes Element)
    // ist keine gültige Beschreibung, und genau so sah die Kette bei Clips ohne weiteren Effekt aus.
    format!("{scaler} ! capsfilter caps=video/x-raw,width={w},height={h},pixel-aspect-ratio=1/1")
}

/// Effektkette ohne den Verkleinerungsschritt (Ausweichlösung, falls die Kette mit ihm nicht anzulegen ist).
fn without_prescale(fx: &str) -> String {
    fx.split(" ! ").filter(|s| !s.starts_with("videoconvertscale") && !s.starts_with("videoscale") && !s.starts_with("videoconvert ") && !s.starts_with("capsfilter")).collect::<Vec<_>>().join(" ! ")
}

fn needs_shape(c: &Clip) -> bool {
    matches!(c.kind, Kind::Video | Kind::Image)
        && (c.shape != Shape::Rect || c.border > 0.0 || c.chroma || c.crop.iter().any(|v| *v > 0.0005))
}

/// Weichzeichner-Bereiche für einen Clip aus den Weichzeichner-Anmerkungen auf höheren Spuren.
fn blur_regions(p: &Project, c: &Clip) -> String {
    if !matches!(c.kind, Kind::Video | Kind::Image) { return String::new(); }
    let Some(ci) = p.track_index(c.track) else { return String::new() };
    let mut out = Vec::new();
    for b in p.clips.iter().filter(|b| b.kind == Kind::Blur) {
        let Some(bi) = p.track_index(b.track) else { continue };
        if bi <= ci || p.tracks[bi].hidden { continue; }
        if b.end() <= c.start || b.start >= c.end() { continue; }
        let r = c.rect_at(b.start.max(c.start));
        let f = full_rect(c, r, Geo::Media);
        if f.2 <= 0.0 || f.3 <= 0.0 { continue; }
        let (x, y, w, h) = ((b.x - f.0) / f.2, (b.y - f.1) / f.3, b.w / f.2, b.h / f.3);
        if x + w <= 0.0 || y + h <= 0.0 || x >= 1.0 || y >= 1.0 { continue; }
        let t0 = c.source_time(b.start);
        let t1 = c.source_time(b.end());
        // Stärke gilt für das fertige Bild: bei klein dargestellten Clips entsprechend gröber rechnen
        let size = (p.height as f64 / f.3).clamp(0.2, 30.0);
        out.push(format!("{:.5},{:.5},{:.5},{:.5},{:.4},{:.4},{:.3},{},{:.2}", x, y, w, h, t0, t1, b.blur, b.pixelate as u8, size));
    }
    out.join(";")
}

impl Builder {
    pub fn new(p: &Project, scale: f64) -> Builder {
        let timeline = ges::Timeline::new_audio_video();
        let commits = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        {
            let c = commits.clone();
            timeline.connect_commited(move |_| { c.fetch_add(1, std::sync::atomic::Ordering::Relaxed); });
        }
        let mut b = Builder { timeline, layers: Vec::new(), track_ids: Vec::new(), dims: (0, 0, 0), nodes: HashMap::new(), scale, fps_div: 1, offline: HashSet::new(), grave: Vec::new(), pre: HashMap::new(), commits };
        b.setup_layers(p);
        b
    }

    pub fn out_size(p: &Project, scale: f64) -> (u32, u32) {
        let w = ((p.width as f64 * scale / 2.0).round() as u32 * 2).max(2);
        let h = ((p.height as f64 * scale / 2.0).round() as u32 * 2).max(2);
        (w, h)
    }

    /// Clip aus seiner Ebene nehmen, aber noch nicht freigeben.
    ///
    /// GES entfernt die Quelle eines Clips erst bei der nächsten Übernahme aus der laufenden Wiedergabe. Bis dahin
    /// kann sie in ihrem eigenen Thread noch am Laden sein und ruft dabei in ihren Clip zurück, ohne ihn
    /// festzuhalten. Ist der Clip dann schon freigegeben, stürzt das Programm ab (so geschehen mit einem PNG-Bild).
    /// Deshalb bleiben entfernte Clips samt ihren Teilen liegen, bis die Zeitleiste die Änderung sicher
    /// übernommen hat.
    fn bury(&mut self, clip: &ges::Clip) {
        let mut objects: Vec<glib::Object> = clip.children(true).into_iter().map(|c| c.upcast()).collect();
        objects.push(clip.clone().upcast());
        if let Some(l) = clip.layer() { let _ = l.remove_clip(clip); }
        if std::env::var_os("WS_NO_GRAVE").is_some() { return; }
        self.grave.push(Buried { since: std::time::Instant::now(), commits: self.commits.load(std::sync::atomic::Ordering::Relaxed), _objects: objects });
    }

    /// Liegen gebliebene Clips freigeben, sobald das gefahrlos geht: Die Zeitleiste hat seit dem Entfernen
    /// zweimal übernommen (die erste Übernahme kann schon vorher unterwegs gewesen sein), und es ist Zeit vergangen.
    fn reap(&mut self) {
        let done = self.commits.load(std::sync::atomic::Ordering::Relaxed);
        self.grave.retain(|b| {
            let age = b.since.elapsed().as_secs_f64();
            !((done >= b.commits + 2 && age > 5.0) || (done > b.commits && age > 60.0))
        });
    }

    fn setup_layers(&mut self, p: &Project) {
        // alles entfernen
        let old: Vec<Node> = self.nodes.drain().map(|(_, n)| n).collect();
        for n in old {
            self.bury(&n.clip);
            if let Some(s) = n.shadow { self.bury(&s); }
        }
        for l in self.layers.drain(..) { let _ = self.timeline.remove_layer(&l); }
        let (w, h) = Builder::out_size(p, self.scale);
        self.dims = (w, h, p.fps);
        self.set_video_caps();
        for track in self.timeline.tracks() {
            if track.track_type() == ges::TrackType::AUDIO {
                let caps = gst::Caps::builder("audio/x-raw").field("rate", 48000i32).field("channels", 2i32).build();
                track.set_restriction_caps(&caps);
            }
        }
        // je Spur drei Ebenen: obere und untere Clip-Ebene (für Überblendungen) und eine für Schatten
        for _ in 0..p.tracks.len() * 3 {
            let l = self.timeline.append_layer();
            self.layers.push(l);
        }
        self.track_ids = p.tracks.iter().map(|t| t.id).collect();
    }

    /// Größe und Bildrate der Bildspur festlegen. Die Quellen lassen überzählige Bilder gleich nach dem Dekodieren
    /// weg: Eine kleinere Bildrate entlastet deshalb Umwandlung, Effekte und Mischen zugleich.
    fn set_video_caps(&self) {
        let (w, h, fps) = self.dims;
        let caps = gst::Caps::builder("video/x-raw").field("width", w as i32).field("height", h as i32)
            .field("framerate", gst::Fraction::new(fps.max(1) as i32, self.fps_div.max(1) as i32)).build();
        for track in self.timeline.tracks() {
            if track.track_type() == ges::TrackType::VIDEO { track.set_restriction_caps(&caps); }
        }
    }

    pub fn fps(&self) -> u32 { self.dims.2.max(1) }

    /// Bildrate der Vorschau teilen (1 = voll). Liefert true, wenn sich etwas geändert hat.
    pub fn set_fps_div(&mut self, div: u32) -> bool {
        let div = div.clamp(1, 6);
        if div == self.fps_div { return false; }
        self.fps_div = div;
        self.set_video_caps();
        true
    }

    fn desire(&mut self, p: &Project, c: &Clip, xf: XFade) -> Option<Desired> {
        let ti = p.track_index(c.track)?;
        let track = &p.tracks[ti];
        let base = (p.tracks.len() - 1 - ti) * 3;
        let layer = if xf.upper { base } else { base + 1 };
        let mut image = None;
        let (uri, geo, types, inpoint) = match c.kind {
            Kind::Video => {
                let mut t = ges::TrackType::empty();
                if c.has_video { t |= ges::TrackType::VIDEO; }
                if c.has_audio { t |= ges::TrackType::AUDIO; }
                (path_uri(&c.path), Geo::Media, t, c.inpoint)
            }
            Kind::Audio => (path_uri(&c.path), Geo::Media, ges::TrackType::AUDIO, c.inpoint),
            Kind::Image => (path_uri(&c.path), Geo::Media, ges::TrackType::VIDEO, 0.0),
            Kind::Text | Kind::Shape | Kind::Arrow => {
                let r = annot::render(c)?;
                image = Some(r.path);
                ("bild".to_string(), Geo::Padded(r.pad), ges::TrackType::VIDEO, 0.0)
            }
            Kind::Blur => return None,
        };
        if types.is_empty() || c.duration <= 0.0 { return None; }
        let regions = blur_regions(p, c);
        let mut fx: Vec<String> = Vec::new();
        if c.is_visual() && c.kind.is_media() {
            if std::env::var_os("WS_NO_PRESCALE").is_none() {
                if let Some(need) = prescale_need(c, self.scale) {
                    let exact = prescale_step(need);
                    // Jeder Wechsel der Stufe baut den Clip neu auf. Beim Ziehen an der Größe soll das selten
                    // geschehen: Eine vorhandene Arbeitskopie bleibt, solange sie reicht und höchstens dreimal
                    // zu groß ist. Reicht sie nicht mehr, gibt es gleich etwas Luft nach oben.
                    let f = match self.pre.get(&c.id).copied() {
                        Some(old) if old + 1e-9 >= exact && old <= exact * 3.0 + 1e-9 => old,
                        Some(old) if old < exact => prescale_step((need * 1.5).min(1.0)),
                        _ => exact,
                    };
                    self.pre.insert(c.id, f);
                    if f < 1.0 {
                        let (w, h) = prescale_size(c, f);
                        fx.push(prescale_desc(w, h));
                    }
                }
            }
            if !regions.is_empty() { fx.push("wsblur".into()); }
            if c.cursor.as_ref().map(|k| !k.data.is_empty()).unwrap_or(false) { fx.push("wscursor".into()); }
            if c.bg_mode != BgMode::Off { fx.push("wsperson".into()); }
            if needs_shape(c) { fx.push("wsshape".into()); }
            if c.needs_fx() { fx.push("wsfx".into()); }
            if c.fade_in_kind == FadeKind::Roll || c.fade_out_kind == FadeKind::Roll { fx.push("wsroll".into()); }
        }
        // Anmerkungen (Text, Formen) laufen über `wsimage`; sie bekommen `wsfx` als zweiten Effekt.
        if image.is_some() && c.needs_fx() { fx.push("wsfx".into()); }
        let fx = fx.join(" ! ");
        let shadow = if c.shadow && c.kind.is_media() && c.is_visual() { annot::render_shadow(c).map(|r| (r.path, r.pad)) } else { None };
        // Der Schlüssel enthält nur, was den Aufbau der Zeitleiste bestimmt. Bilder von Anmerkungen und Schatten
        // sind Eigenschaften: Sie ändern sich beim Tippen oder Skalieren, ohne dass ein Clip neu angelegt wird.
        // Die Lage (Ebene, Beginn, Einstieg, Dauer) gehört nicht dazu: Verschieben und Trimmen ändern den
        // vorhandenen Clip, statt ihn bei jeder Mausbewegung neu zu laden.
        let afx = if types.contains(ges::TrackType::AUDIO) && c.audio.active() { afx_desc(p, c) } else { String::new() };
        let key = format!("{uri}|{}|{fx}|{}|{}", types.bits(), shadow.is_some(), !afx.is_empty());
        let place = format!("{layer}|{:.6}|{:.6}|{:.6}", c.start, inpoint, c.duration);
        let props = format!(
            "{:?}|{}|{}|{}|{}|{:?}|{}|{}|{}",
            c, self.scale, track.hidden, track.muted, regions, xf, image.as_deref().unwrap_or(""), shadow.as_ref().map(|s| s.0.as_str()).unwrap_or(""), afx
        );
        Some(Desired { key, place, props, uri, image, layer, types, start: c.start, inpoint, duration: c.duration, geo, fx, shadow, regions, hidden: track.hidden, muted: track.muted, xf, afx })
    }

    /// Effektkette eines Clips. Lässt sie sich mit dem Verkleinerungsschritt nicht anlegen, dann ohne ihn:
    /// Der Schritt spart nur Rechenzeit, das Bild ist ohne ihn dasselbe.
    fn effect(fx: &str) -> anyhow::Result<Option<ges::Effect>> {
        match ges::Effect::new(fx) {
            Ok(e) => Ok(Some(e)),
            Err(e) => {
                let plain = without_prescale(fx);
                if plain == fx { return Err(e.into()); }
                eprintln!("ws-video: Effektkette ohne Verkleinerung angelegt ({fx})");
                if plain.is_empty() { Ok(None) } else { Ok(Some(ges::Effect::new(&plain)?)) }
            }
        }
    }

    /// Clip, der ein PNG-Bild zeigt: durchsichtige Testquelle plus `wsimage`.
    fn image_clip(layer: &ges::Layer, start: f64, duration: f64, extra: &str) -> anyhow::Result<ges::Clip> {
        let clip = ges::TestClip::new().ok_or_else(|| anyhow::anyhow!("Testclip konnte nicht angelegt werden"))?;
        clip.set_vpattern(ges::VideoTestPattern::SolidColor);
        clip.set_supported_formats(ges::TrackType::VIDEO);
        clip.set_start(ct(start));
        clip.set_inpoint(ct(0.0));
        clip.set_duration(ct(duration));
        let eff = ges::Effect::new("wsimage")?;
        layer.add_clip(&clip)?;
        // Reihenfolge: Der zuletzt hinzugefügte Effekt wirkt zuerst. `wsimage` malt das Bild, der zweite Effekt
        // soll danach darauf wirken, also kommt er vorher in den Clip.
        if !extra.is_empty() {
            match ges::Effect::new(extra) {
                Ok(more) => { let _ = clip.add(&more); }
                Err(e) => eprintln!("ws-video: Effekt {extra} konnte nicht angelegt werden: {e}"),
            }
        }
        if let Err(e) = clip.add(&eff) {
            let _ = layer.remove_clip(&clip);
            return Err(e.into());
        }
        Ok(clip.upcast())
    }

    fn create(&self, d: &Desired) -> anyhow::Result<Node> {
        let layer = self.layers.get(d.layer).ok_or_else(|| anyhow::anyhow!("Spur fehlt"))?;
        let clip = if d.image.is_some() {
            Builder::image_clip(layer, d.start, d.duration, &d.fx)?
        } else {
            if ASSETS.with(|a| a.borrow().get(&d.uri).copied()) == Some(false) { anyhow::bail!("Medium nicht lesbar"); }
            let asset = ges::UriClipAsset::request_sync(&d.uri)?;
            // Effekt zuerst anlegen: Scheitert er, darf kein halb fertiger Clip in der Spur zurückbleiben.
            // (Ein solcher Rest wurde formatfüllend gezeigt und ließ sich nicht mehr bewegen.)
            let eff = if d.fx.is_empty() { None } else { Builder::effect(&d.fx)? };
            let clip = layer.add_asset(&asset, ct(d.start), ct(d.inpoint), ct(d.duration), d.types)?;
            if let Some(eff) = eff {
                if let Err(e) = clip.add(&eff) {
                    let _ = layer.remove_clip(&clip);
                    return Err(e.into());
                }
            }
            if !d.afx.is_empty() {
                // Tonbearbeitung: Fehlt sie, bleibt der Clip trotzdem benutzbar (dann eben unbearbeitet).
                match ges::Effect::new("wsaudio") {
                    Ok(a) => { if let Err(e) = clip.add(&a) { eprintln!("ws-video: Tonbearbeitung ließ sich nicht einsetzen: {e}"); } }
                    Err(e) => eprintln!("ws-video: Tonbearbeitung ließ sich nicht anlegen: {e}"),
                }
            }
            clip
        };
        let mut shadow = None;
        if d.shadow.is_some() {
            if let Some(aux) = self.layers.get(d.layer / 3 * 3 + 2) {
                shadow = Builder::image_clip(aux, d.start, d.duration, "").ok();
            }
        }
        harden(&clip);
        if let Some(s) = &shadow { harden(s); }
        Ok(Node { clip, shadow, key: d.key.clone(), place: d.place.clone(), props: String::new(), sent: Default::default() })
    }

    /// Vorhandenen Clip an seine neue Stelle bringen (andere Ebene, Beginn, Einstieg, Dauer), ohne ihn neu zu
    /// laden. Liefert `false`, wenn die Zeitleiste die Lage nicht annimmt; dann wird der Clip neu aufgebaut.
    fn place(&self, n: &Node, d: &Desired) -> bool {
        let put = |clip: &ges::Clip, layer: Option<&ges::Layer>, inpoint: f64| -> bool {
            let Some(layer) = layer else { return false };
            if clip.layer().as_ref() != Some(layer) && clip.move_to_layer(layer).is_err() { return false; }
            let (s, i, du) = (ct(d.start), ct(inpoint), ct(d.duration));
            // erst kürzen, dann Einstieg und Beginn, dann die neue Länge: So ist jeder Zwischenschritt gültig.
            if clip.duration() > du { clip.set_duration(du); }
            if clip.inpoint() != i { clip.set_inpoint(i); }
            if clip.start() != s { clip.set_start(s); }
            if clip.duration() != du { clip.set_duration(du); }
            clip.start() == s && clip.inpoint() == i && clip.duration() == du
        };
        let inpoint = if d.image.is_some() { 0.0 } else { d.inpoint };
        if !put(&n.clip, self.layers.get(d.layer), inpoint) { return false; }
        match &n.shadow {
            Some(sh) => put(sh, self.layers.get(d.layer / 3 * 3 + 2), 0.0),
            None => true,
        }
    }

    /// Position, Größe und Deckkraft setzen (fest oder als Keyframes).
    fn apply_geometry(&self, sent: &Sent, te: &ges::TrackElement, c: &Clip, d: &Desired, geo: Geo, alpha_factor: f64) {
        let s = self.scale;
        let to_eng = |st: f64| ct(st - c.inpoint + d.inpoint);
        let names = ["posx", "posy", "width", "height"];
        let (pw, ph) = (self.dims.0 as f64 / s.max(1e-6), self.dims.1 as f64 / s.max(1e-6));
        if c.anims.is_empty() && !c.has_motion() {
            let f = full_rect(c, (c.x, c.y, c.w, c.h), geo);
            let vals = [f.0 * s, f.1 * s, (f.2 * s).max(1.0), (f.3 * s).max(1.0)];
            for (n, v) in names.iter().zip(vals) { set_ctl(sent, te, n, "direct-absolute", &[(0, v.round())], true); }
        } else {
            let mut times: Vec<f64> = vec![c.inpoint.min(0.0), c.inpoint];
            for a in &c.anims {
                let steps = ((a.dur * 30.0).ceil() as usize).clamp(1, 120);
                for i in 0..=steps { times.push(a.t + a.dur * i as f64 / steps as f64); }
            }
            // Hinein-/Hinausgleiten und Aufspringen: dicht abtasten, damit die Kurve weich bleibt
            if c.has_motion() {
                let d = c.duration;
                for (a, len) in [(0.0, c.fade_in.clamp(0.0, d)), (d - c.fade_out.clamp(0.0, d), c.fade_out.clamp(0.0, d))] {
                    if len <= 0.0 { continue; }
                    let steps = ((len * 30.0).ceil() as usize).clamp(1, 120);
                    for i in 0..=steps { times.push(c.inpoint + a + len * i as f64 / steps as f64); }
                }
            }
            times.push(c.inpoint + c.duration + 1.0);
            times.retain(|t| *t >= c.inpoint - 1e-9);
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            times.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
            let mut curves: [Vec<(u64, f64)>; 4] = Default::default();
            for t in &times {
                let rel = (*t - c.inpoint).clamp(0.0, c.duration);
                let f = full_rect(c, c.motion_rect(c.rect_at_source(*t), rel, pw, ph), geo);
                let vals = [f.0 * s, f.1 * s, (f.2 * s).max(1.0), (f.3 * s).max(1.0)];
                for (cv, v) in curves.iter_mut().zip(vals) { cv.push((to_eng(*t).nseconds(), v.round())); }
            }
            for (n, cv) in names.iter().zip(curves.iter()) { set_ctl(sent, te, n, "direct-absolute", cv, true); }
        }
        // Deckkraft mit Ein-/Ausblenden und Überblendung
        let op = (c.opacity * alpha_factor).clamp(0.0, 1.0);
        let env = envelope(c, if d.xf.ramp_in { d.xf.ov_in } else { 0.0 }, if d.xf.ramp_out { d.xf.ov_out } else { 0.0 });
        if env.iter().any(|(_, v)| *v < 0.999) {
            let pts: Vec<(u64, f64)> = env.iter().map(|(t, v)| (ct(d.inpoint + t).nseconds(), v * op)).collect();
            set_ctl(sent, te, "alpha", "direct", &pts, false);
        } else {
            set_ctl(sent, te, "alpha", "direct", &[(0, op)], false);
        }
    }

    fn apply(&self, node: &Node, c: &Clip, d: &Desired) {
        for te in track_elements(&node.clip) {
            if te.is::<ges::VideoSource>() {
                self.apply_geometry(&node.sent, &te, c, d, d.geo, 1.0);
                if te.is_active() == d.hidden { let _ = te.set_active(!d.hidden); }
            } else if te.is::<ges::AudioSource>() {
                let vol = c.volume.clamp(0.0, 4.0);
                let env = envelope(c, d.xf.ov_in, d.xf.ov_out);
                if env.iter().any(|(_, v)| *v < 0.999) {
                    let pts: Vec<(u64, f64)> = env.iter().map(|(t, v)| (ct(d.inpoint + t).nseconds(), v * vol)).collect();
                    set_ctl(&node.sent, &te, "volume", "direct-absolute", &pts, false);
                } else {
                    set_ctl(&node.sent, &te, "volume", "direct-absolute", &[(0, vol)], false);
                }
                set_child(&te, "mute", c.muted || d.muted);
            } else if te.is::<ges::BaseEffect>() && te.track_type() == ges::TrackType::AUDIO {
                set_child(&te, "afx", d.afx.clone());
            } else if te.is::<ges::BaseEffect>() {
                if let Some(img) = &d.image { set_child(&te, "img-path", img.clone()); }
                if d.image.is_some() || d.fx.contains("wsroll") { set_child(&te, "roll", roll_desc(c, d.inpoint, d.geo)); }
                if d.fx.contains("wsfx") { set_child(&te, "fx", fx_desc(c, d.inpoint)); }
                if d.fx.contains("wsshape") {
                    // Rahmenbreite: Projektpixel -> Anteil der kürzeren sichtbaren Seite
                    let short_canvas = c.w.min(c.h).max(1.0);
                    set_child(&te, "shape", match c.shape { Shape::Rect => 0i32, Shape::Rounded => 1, Shape::Ellipse => 2 });
                    set_child(&te, "radius", c.radius.clamp(0.0, 1.0));
                    set_child(&te, "crop-left", c.crop[0].clamp(0.0, 1.0));
                    set_child(&te, "crop-top", c.crop[1].clamp(0.0, 1.0));
                    set_child(&te, "crop-right", c.crop[2].clamp(0.0, 1.0));
                    set_child(&te, "crop-bottom", c.crop[3].clamp(0.0, 1.0));
                    set_child(&te, "border", (c.border / short_canvas).clamp(0.0, 0.5));
                    set_child(&te, "border-color", color_u32(&c.border_color));
                    set_child(&te, "chroma", c.chroma);
                    set_child(&te, "chroma-color", color_u32(&c.chroma_color));
                    set_child(&te, "chroma-tolerance", c.chroma_tolerance.clamp(0.0, 1.0));
                }
                if d.fx.contains("wscursor") {
                    if let Some(k) = &c.cursor {
                        set_child(&te, "cur-data", k.data.clone());
                        set_child(&te, "cur-monitor", k.monitor);
                        set_child(&te, "cur-show", k.show);
                        set_child(&te, "cur-scale", k.scale.clamp(0.1, 10.0));
                        set_child(&te, "cur-smooth", k.smooth.clamp(0.0, 5.0));
                        set_child(&te, "cur-highlight", k.highlight);
                        set_child(&te, "cur-highlight-color", color_u32(&k.highlight_color));
                        set_child(&te, "cur-clicks", k.clicks);
                        set_child(&te, "cur-click-color", color_u32(&k.click_color));
                    }
                }
                if d.fx.contains("wsperson") {
                    set_child(&te, "bg-mode", c.bg_mode.index() as i32);
                    set_child(&te, "bg-path", c.bg_image.clone());
                }
                if d.fx.contains("wsblur") {
                    set_child(&te, "regions", d.regions.clone());
                }
            }
        }
        if let (Some(sh), Some((img, pad))) = (&node.shadow, &d.shadow) {
            for te in track_elements(sh) {
                if te.is::<ges::BaseEffect>() {
                    set_child(&te, "img-path", img.clone());
                    set_child(&te, "roll", roll_desc(c, 0.0, Geo::Padded(*pad)));
                } else if te.is::<ges::VideoSource>() {
                    let mut d2 = Desired { inpoint: 0.0, ..clone_desired(d) };
                    d2.geo = Geo::Padded(*pad);
                    self.apply_geometry(&node.sent, &te, c, &d2, Geo::Padded(*pad), 1.0);
                    if te.is_active() == d.hidden { let _ = te.set_active(!d.hidden); }
                }
            }
        }
    }

    /// Modell und GES-Zeitleiste abgleichen. Liefert true, wenn sich etwas geändert hat.
    pub fn sync(&mut self, p: &Project) -> bool {
        let mut changed = false;
        for (path, prof) in &p.audio_profiles { crate::audiofx::set_profile(path, prof); }
        let tids: Vec<u64> = p.tracks.iter().map(|t| t.id).collect();
        let (w, h) = Builder::out_size(p, self.scale);
        if tids != self.track_ids || (w, h, p.fps) != self.dims {
            self.setup_layers(p);
            changed = true;
        }
        let was_offline = std::mem::take(&mut self.offline);
        self.reap();
        let mut desired: Vec<(u64, Desired)> = Vec::new();
        let mut clips: Vec<&Clip> = p.clips.iter().collect();
        clips.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        let xfs = crossfades(p);
        for c in clips {
            let xf = xfs.get(&c.id).copied().unwrap_or_default();
            if let Some(d) = self.desire(p, c, xf) { desired.push((c.id, d)); }
        }
        let want: HashMap<u64, &str> = desired.iter().map(|(id, d)| (*id, d.key.as_str())).collect();
        let remove: Vec<u64> = self.nodes.iter().filter(|(id, n)| want.get(id).map(|k| *k != n.key).unwrap_or(true)).map(|(id, _)| *id).collect();
        for id in remove {
            if let Some(n) = self.nodes.remove(&id) {
                self.bury(&n.clip);
                if let Some(s) = n.shadow { self.bury(&s); }
                changed = true;
            }
        }
        // Verschobene und getrimmte Clips an Ort und Stelle ändern. Nur wenn das scheitert, neu aufbauen.
        for (id, d) in &desired {
            let moved = self.nodes.get(id).map(|n| n.place != d.place).unwrap_or(false);
            if !moved { continue; }
            let ok = self.nodes.get(id).map(|n| self.place(n, d)).unwrap_or(false);
            if ok {
                if let Some(n) = self.nodes.get_mut(id) { n.place = d.place.clone(); }
            } else if let Some(n) = self.nodes.remove(id) {
                self.bury(&n.clip);
                if let Some(s) = n.shadow { self.bury(&s); }
            }
            changed = true;
        }
        for (id, d) in &desired {
            if !self.nodes.contains_key(id) {
                match self.create(d) {
                    Ok(n) => {
                        crate::crashlog::note(&format!("Clip {id} aufgebaut ({})", if d.fx.is_empty() { "ohne Effekt" } else { d.fx.as_str() }));
                        self.nodes.insert(*id, n);
                        changed = true;
                    }
                    Err(e) => {
                        // nur einmal melden, nicht bei jedem Abgleich
                        if !was_offline.contains(id) { eprintln!("ws-video: Clip {id} konnte nicht geladen werden: {e}"); }
                        self.offline.insert(*id);
                    }
                }
            }
        }
        for (id, d) in &desired {
            let Some(c) = p.clip(*id) else { continue };
            let needs = self.nodes.get(id).map(|n| n.props != d.props).unwrap_or(false);
            if needs {
                if let Some(n) = self.nodes.get(id) { self.apply(n, c, d); }
                if let Some(n) = self.nodes.get_mut(id) { n.props = d.props.clone(); }
                changed = true;
            }
        }
        changed
    }
}

fn clone_desired(d: &Desired) -> Desired {
    Desired {
        key: d.key.clone(), place: d.place.clone(), props: d.props.clone(), uri: d.uri.clone(), image: d.image.clone(), layer: d.layer, types: d.types, start: d.start, inpoint: d.inpoint,
        duration: d.duration, geo: d.geo, fx: d.fx.clone(), shadow: d.shadow.clone(), regions: d.regions.clone(), hidden: d.hidden, muted: d.muted, xf: d.xf, afx: d.afx.clone(),
    }
}

// ---------------------------------------------------------------------------------------------
// Vorschau

/// Zähler der Vorschau, vom Wiedergabe-Thread geschrieben.
pub struct PreviewStats {
    /// Zahl der beim Abspielen angekommenen Bilder
    pub shown: AtomicU64,
    /// Stelle im Projekt (Nanosekunden), zu der das zuletzt angekommene Bild gehört; `u64::MAX` = unbekannt
    pub frame_ns: AtomicU64,
}

fn note_frame(sample: &gst::Sample, stats: &PreviewStats, playing: bool) {
    if playing { stats.shown.fetch_add(1, Ordering::Relaxed); }
    let t = sample.buffer().and_then(|b| b.pts()).and_then(|pts| {
        sample.segment().and_then(|s| s.downcast_ref::<gst::ClockTime>().and_then(|s| s.to_stream_time(pts)))
    });
    stats.frame_ns.store(t.map(|t| t.nseconds()).unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// Ab diesem Rückstand (Sekunden) gilt das Bild als hinter dem Ton zurückgeblieben.
const LAG_LIMIT: f64 = 0.3;

/// Wächter für den Gleichlauf von Bild und Ton in der Vorschau.
///
/// Der Ton läuft immer in Echtzeit. Schafft der Rechner das Bild nicht in Echtzeit (Dekodieren, Effekte, Mischen),
/// fiele es ohne Eingriff immer weiter hinter den Ton zurück. Die übliche Abhilfe von GStreamer (QoS: verspätete
/// Bilder auslassen) ist hier unbrauchbar: Nach einem Sprung verwirft die Schnittschicht (nlecomposition) alle
/// QoS-Meldungen, und wo sie durchkommen, bricht die Bildrate auf wenige Bilder je Sekunde ein. Deshalb misst die
/// Anwendung selbst, wie weit das gezeigte Bild hinter der Uhr liegt. Bleibt es zurück, wird die Bildrate der
/// Vorschau geteilt (die Quellen lassen dann gleichmäßig Bilder aus, das entlastet alle folgenden Stufen) und das
/// Bild mit einem Sprung an die Stelle des Tons geholt.
struct Pace {
    /// vorher wird nicht geprüft (Anlauf nach Start, Sprung oder Umbau)
    settle: std::time::Instant,
    /// seit wann das Bild ununterbrochen zurückliegt, und die Zahl der bis dahin gezeigten Bilder
    late: Option<(std::time::Instant, u64)>,
    /// Es kommen gar keine Bilder an: einmal neu ansetzen, dann nicht weiter eingreifen.
    stuck: bool,
    last_resync: Option<std::time::Instant>,
    last_down: Option<std::time::Instant>,
    /// Sekunden seit der letzten Absenkung, bevor beim Abspielen wieder die volle Bildrate versucht wird
    retry: f64,
    /// seit wann versuchsweise wieder die volle Bildrate läuft. Ob der Versuch trägt, zeigt sich erst nach einer
    /// Weile: Die Wiedergabe hält bis zu zwei Sekunden fertige Bilder vor, die ein zu langsamer Rechner aufzehrt.
    trial: Option<std::time::Instant>,
}

pub struct Engine {
    pub builder: Builder,
    pub pipeline: ges::Pipeline,
    frame: Arc<Mutex<Option<Frame>>>,
    serial: Arc<AtomicU64>,
    pub stats: Arc<PreviewStats>,
    pace: Pace,
    /// für den Wiedergabe-Thread: läuft die Wiedergabe gerade?
    live: Arc<std::sync::atomic::AtomicBool>,
    playing: bool,
    pos: f64,
    want_seek: Option<f64>,
    need_commit: bool,
    busy: bool,
    busy_since: std::time::Instant,
    pub duration: f64,
    pub ended: bool,
    _watch: Option<gst::bus::BusWatchGuard>,
}

pub fn preview_scale(p: &Project) -> f64 {
    let max_w = 1280.0;
    (max_w / p.width.max(1) as f64).min(1.0)
}

impl Engine {
    pub fn new(p: &Project) -> anyhow::Result<std::rc::Rc<std::cell::RefCell<Engine>>> {
        retry_failed_assets();
        preload_project(p);
        let builder = Builder::new(p, preview_scale(p));
        let pipeline = ges::Pipeline::new();
        pipeline.set_timeline(&builder.timeline)?;
        let frame: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
        let serial = Arc::new(AtomicU64::new(0));
        let sink = gst_app::AppSink::builder().caps(&gst::Caps::builder("video/x-raw").field("format", "BGRx").build()).max_buffers(2).build();
        // QoS bleibt bewusst aus (siehe `Pace`): Durch die Schnittschicht hindurch ist es unzuverlässig.
        // Den Gleichlauf von Bild und Ton überwacht `Engine::pace_tick`.
        sink.set_property("qos", false);
        let stats = Arc::new(PreviewStats { shown: AtomicU64::new(0), frame_ns: AtomicU64::new(u64::MAX) });
        let live = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (f1, s1, st1) = (frame.clone(), serial.clone(), stats.clone());
        let (f2, s2, st2, live2) = (frame.clone(), serial.clone(), stats.clone(), live.clone());
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_preroll(move |sink| {
                    if let Ok(sample) = sink.pull_preroll() {
                        note_frame(&sample, &st1, false);
                        if let Some(f) = sample_to_frame(&sample) { *f1.lock().unwrap() = Some(f); s1.fetch_add(1, Ordering::SeqCst); }
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .new_sample(move |sink| {
                    if let Ok(sample) = sink.pull_sample() {
                        note_frame(&sample, &st2, live2.load(Ordering::Relaxed));
                        if let Some(f) = sample_to_frame(&sample) { *f2.lock().unwrap() = Some(f); s2.fetch_add(1, Ordering::SeqCst); }
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        pipeline.preview_set_video_sink(Some(&sink));
        let audio_sink = if std::env::var_os("WS_NO_AUDIO").is_some() {
            gst::ElementFactory::make("fakesink").property("sync", true).build()?
        } else {
            gst::ElementFactory::make("autoaudiosink").build().or_else(|_| gst::ElementFactory::make("fakesink").property("sync", true).build())?
        };
        pipeline.preview_set_audio_sink(Some(&audio_sink));
        pipeline.set_mode(ges::PipelineFlags::FULL_PREVIEW)?;
        let eng = std::rc::Rc::new(std::cell::RefCell::new(Engine {
            builder, pipeline: pipeline.clone(), frame, serial, stats,
            pace: Pace { settle: std::time::Instant::now(), late: None, stuck: false, last_resync: None, last_down: None, retry: 30.0, trial: None },
            live,
            playing: false, pos: 0.0, want_seek: None, need_commit: false,
            busy: false, busy_since: std::time::Instant::now(), duration: 0.0, ended: false, _watch: None,
        }));
        let weak = std::rc::Rc::downgrade(&eng);
        let bus = pipeline.bus().unwrap();
        let watch = bus.add_watch_local(move |_, msg| {
            if let Some(e) = weak.upgrade() {
                if let Ok(mut e) = e.try_borrow_mut() { e.on_message(msg); }
            }
            glib::ControlFlow::Continue
        })?;
        {
            let mut e = eng.borrow_mut();
            e._watch = Some(watch);
            e.builder.sync(p);
            e.builder.timeline.commit();
            e.duration = p.duration();
            let _ = e.pipeline.set_state(gst::State::Paused);
        }
        Ok(eng)
    }

    fn on_message(&mut self, msg: &gst::Message) {
        match msg.view() {
            gst::MessageView::AsyncDone(_) => {
                self.busy = false;
                self.pump();
            }
            gst::MessageView::Eos(_) => {
                if self.playing { self.finish(); }
            }
            gst::MessageView::Error(e) => {
                eprintln!("ws-video: Vorschau-Fehler: {} ({:?})", e.error(), e.debug());
                self.busy = false;
            }
            _ => {}
        }
    }

    fn pump(&mut self) {
        if self.busy && self.busy_since.elapsed() < std::time::Duration::from_millis(1500) { return; }
        self.busy = false;
        if self.need_commit {
            self.need_commit = false;
            let committed = self.builder.timeline.commit();
            if committed {
                self.busy = true;
                self.busy_since = std::time::Instant::now();
                if !self.playing && self.want_seek.is_none() { self.want_seek = Some(self.pos); }
                return;
            }
            if !self.playing && self.want_seek.is_none() { self.want_seek = Some(self.pos); }
        }
        if let Some(t) = self.want_seek.take() {
            let t = t.clamp(0.0, self.duration.max(0.0));
            let flags = gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE;
            if self.pipeline.seek_simple(flags, ct(t)).is_ok() {
                self.busy = true;
                self.busy_since = std::time::Instant::now();
            }
        }
    }

    /// Nach einer Änderung des Projekts aufrufen.
    pub fn sync(&mut self, p: &Project) {
        let changed = self.builder.sync(p);
        self.duration = p.duration();
        if changed {
            self.need_commit = true;
            self.pump();
        }
    }

    pub fn tick(&mut self) {
        if self.playing && !self.busy {
            if let Some(t) = self.pipeline.query_position::<gst::ClockTime>() { self.pos = secs(t).min(self.duration); }
            // Die Uhr ist am Ende angekommen: nicht auf ein verspätetes Bild warten.
            if self.duration > 0.0 && self.pos >= self.duration - 1e-3 && !self.need_commit && self.want_seek.is_none() { self.finish(); }
        }
        if self.busy || self.need_commit || self.want_seek.is_some() { self.pump(); }
        self.pace_tick();
    }

    /// Wiedergabe am Ende des Projekts anhalten.
    fn finish(&mut self) {
        let late = self.lag().map(|l| l > self.frame_span() * 1.5).unwrap_or(false);
        self.set_playing(false);
        self.ended = true;
        let _ = self.pipeline.set_state(gst::State::Paused);
        self.pos = self.duration;
        // Lag das Bild zurück, steht sonst ein Bild von vor dem Ende da.
        if late {
            self.want_seek = Some((self.duration - 1.0 / self.builder.fps() as f64).max(0.0));
            self.pump();
        }
    }

    fn set_playing(&mut self, on: bool) {
        self.playing = on;
        self.live.store(on, Ordering::Relaxed);
    }

    /// Dauer eines Bildes der Vorschau bei der gerade geltenden Bildrate.
    fn frame_span(&self) -> f64 { self.builder.fps_div.max(1) as f64 / self.builder.fps() as f64 }

    /// Rückstand des zuletzt gezeigten Bildes gegenüber der Uhr der Wiedergabe (dem Ton), in Sekunden.
    pub fn lag(&self) -> Option<f64> {
        let f = self.stats.frame_ns.load(Ordering::Relaxed);
        if f == u64::MAX { None } else { Some(self.pos - f as f64 / 1e9) }
    }

    /// Gleichlauf von Bild und Ton überwachen (siehe `Pace`).
    fn pace_tick(&mut self) {
        let now = std::time::Instant::now();
        if !self.playing || self.busy || self.need_commit || self.want_seek.is_some() {
            // nur im eingeschwungenen Abspielen prüfen
            self.pace.late = None;
            self.pace.settle = now + std::time::Duration::from_millis(600);
            return;
        }
        if now < self.pace.settle || std::env::var_os("WS_NO_PACE").is_some() { return; }
        let shown = self.stats.shown.load(Ordering::Relaxed);
        let lag = match self.lag() {
            // kurz vor dem Ende kommen naturgemäß keine Bilder mehr
            Some(l) if l >= LAG_LIMIT && self.pos < self.duration - 0.3 => l,
            _ => {
                self.pace.late = None;
                self.pace.stuck = false;
                return;
            }
        };
        let (since, shown0) = *self.pace.late.get_or_insert((now, shown));
        let dt = now.duration_since(since).as_secs_f64();
        if dt < 0.8 { return; }
        if shown == shown0 {
            // Es kommt gar kein Bild an. Drei Sekunden abwarten, dann einmal neu ansetzen; hilft das nicht,
            // wird nicht weiter eingegriffen (sonst würde der Ton in einer Schleife immer wieder unterbrochen).
            if dt >= 3.0 && !self.pace.stuck {
                self.pace.stuck = true;
                self.debug(&format!("seit {dt:.1} s kein Bild, Wiedergabe wird neu angesetzt"));
                self.resync();
            }
            return;
        }
        self.pace.stuck = false;
        self.pace.late = None;
        let fps = self.builder.fps() as f64;
        let div = self.builder.fps_div.max(1);
        let rate = (shown - shown0) as f64 / dt;
        if rate > fps / div as f64 * 1.15 {
            // Das Bild holt von selbst auf (es war nur ein kurzer Hänger, etwa an einem Clipwechsel).
            // Erst bei großem Rückstand wird nachgeholfen.
            if lag > 1.5 { self.debug(&format!("Bild {lag:.2} s zurück, holt auf, wird herangeholt")); self.resync(); }
            return;
        }
        let max_div = ((fps / 7.0) as u32).clamp(1, 6);
        if div < max_div {
            // So viele Bilder je Sekunde schafft der Rechner gerade: Die neue Bildrate soll sicher darunter liegen.
            let want = (fps / (rate * 0.9).max(0.1)).ceil() as u32;
            let new = want.clamp(div + 1, max_div);
            // Ein Versuch mit der vollen Bildrate ist gescheitert: beim nächsten Mal länger warten.
            // Hat er dagegen minutenlang gehalten, war er tragfähig, und es gilt wieder die kurze Wartezeit.
            if let Some(t) = self.pace.trial.take() {
                self.pace.retry = if t.elapsed().as_secs_f64() < 180.0 { (self.pace.retry * 2.0).min(900.0) } else { 30.0 };
            }
            self.pace.last_down = Some(now);
            self.debug(&format!("Bild {lag:.2} s hinter dem Ton, {rate:.1} von {:.1} Bildern/s: Vorschau läuft jetzt mit {:.1} Bildern/s", fps / div as f64, fps / new as f64));
            self.builder.set_fps_div(new);
            self.resync();
        } else if lag > 1.0 && self.pace.last_resync.map(|t| t.elapsed().as_secs_f64() > 5.0).unwrap_or(true) {
            // Die kleinste Bildrate ist erreicht und reicht nicht: das Bild wenigstens in Abständen heranholen
            self.debug(&format!("Bild {lag:.2} s hinter dem Ton bei kleinster Bildrate, wird herangeholt"));
            self.resync();
        }
    }

    /// Bild und Ton an der Stelle der Uhr neu ansetzen.
    fn resync(&mut self) {
        self.pace.last_resync = Some(std::time::Instant::now());
        self.want_seek = Some(self.pos);
        self.pump();
    }

    fn debug(&self, text: &str) {
        if std::env::var_os("WS_DEBUG").is_some() { eprintln!("ws-video: Vorschau bei {:.2} s: {text}", self.pos); }
    }

    /// Bildrate, mit der die Vorschau gerade läuft, falls sie gesenkt wurde.
    pub fn reduced_fps(&self) -> Option<f64> {
        if self.builder.fps_div > 1 { Some(self.builder.fps() as f64 / self.builder.fps_div as f64) } else { None }
    }

    /// Wartezeit, bevor nach einer Absenkung wieder die volle Bildrate versucht wird (für Tests).
    pub fn set_pace_retry(&mut self, secs: f64) { self.pace.retry = secs; }

    pub fn seek(&mut self, t: f64) {
        self.pos = t.clamp(0.0, self.duration.max(0.0));
        self.ended = false;
        // Das alte Bild gehört nicht mehr zur neuen Stelle.
        self.stats.frame_ns.store(u64::MAX, Ordering::Relaxed);
        self.want_seek = Some(self.pos);
        self.pump();
    }
    pub fn play(&mut self) {
        if self.duration <= 0.0 { return; }
        if self.pos >= self.duration - 0.02 { self.seek(0.0); }
        // Nach einer Weile wieder die volle Bildrate versuchen (vielleicht war der Rechner nur kurz beschäftigt
        // oder das Projekt ist inzwischen leichter). Scheitert der Versuch, verdoppelt sich die Wartezeit.
        if self.builder.fps_div > 1 && self.pace.last_down.map(|t| t.elapsed().as_secs_f64() >= self.pace.retry).unwrap_or(true) {
            self.debug("volle Bildrate wird wieder versucht");
            self.builder.set_fps_div(1);
            self.pace.trial = Some(std::time::Instant::now());
        }
        self.ended = false;
        self.set_playing(true);
        let _ = self.pipeline.set_state(gst::State::Playing);
    }
    pub fn pause(&mut self) {
        if !self.playing { return; }
        self.set_playing(false);
        let _ = self.pipeline.set_state(gst::State::Paused);
        if let Some(t) = self.pipeline.query_position::<gst::ClockTime>() { self.pos = secs(t).min(self.duration); }
        // Das Standbild soll genau zur Abspielposition gehören, auch wenn das Bild zurücklag
        // oder die Vorschau mit gesenkter Bildrate lief.
        if self.lag().map(|l| l > 1.2 / self.builder.fps() as f64).unwrap_or(false) && self.want_seek.is_none() {
            self.want_seek = Some(self.pos);
            self.pump();
        }
    }
    pub fn is_playing(&self) -> bool { self.playing }
    pub fn position(&self) -> f64 { self.pos }
    pub fn frame_serial(&self) -> u64 { self.serial.load(Ordering::SeqCst) }
    pub fn frame(&self) -> Option<Frame> { self.frame.lock().unwrap().clone() }
    pub fn with_frame<R>(&self, f: impl FnOnce(Option<&Frame>) -> R) -> R { f(self.frame.lock().unwrap().as_ref()) }
    pub fn shutdown(&mut self) {
        self.set_playing(false);
        let _ = self.pipeline.set_state(gst::State::Null);
        self._watch = None;
    }
}

impl Drop for Engine {
    fn drop(&mut self) { let _ = self.pipeline.set_state(gst::State::Null); }
}

// ---------------------------------------------------------------------------------------------
// Export

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec { H264, Hevc, Av1 }

impl Codec {
    pub fn label(self) -> &'static str {
        match self { Codec::H264 => "H.264 (läuft überall)", Codec::Hevc => "HEVC / H.265 (kleiner, gut für 2K und 4K)", Codec::Av1 => "AV1 (am kleinsten, langsam ohne GPU)" }
    }
    fn caps(self) -> gst::Caps {
        match self {
            Codec::H264 => gst::Caps::builder("video/x-h264").build(),
            Codec::Hevc => gst::Caps::builder("video/x-h265").build(),
            Codec::Av1 => gst::Caps::builder("video/x-av1").build(),
        }
    }
    fn hardware(self) -> &'static [&'static str] {
        match self {
            Codec::H264 => &["vah264enc", "vah264lpenc", "nvh264enc", "vaapih264enc", "qsvh264enc"],
            Codec::Hevc => &["vah265enc", "vah265lpenc", "nvh265enc", "vaapih265enc", "qsvh265enc"],
            Codec::Av1 => &["vaav1enc", "nvav1enc", "qsvav1enc"],
        }
    }
    fn software(self) -> &'static [&'static str] {
        match self {
            Codec::H264 => &["x264enc", "openh264enc"],
            Codec::Hevc => &["x265enc"],
            Codec::Av1 => &["svtav1enc", "rav1enc", "av1enc"],
        }
    }
}

/// Prüft mit einer Mini-Pipeline, ob ein Encoder auf diesem Rechner wirklich läuft.
pub fn encoder_works(name: &str) -> bool {
    static CACHE: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
    if let Some(v) = CACHE.lock().unwrap().get_or_insert_with(HashMap::new).get(name) { return *v; }
    let ok = (|| {
        if gst::ElementFactory::find(name).is_none() { return false; }
        let desc = format!("videotestsrc num-buffers=8 ! video/x-raw,width=1280,height=720,framerate=30/1 ! videoconvert ! {name} ! fakesink");
        let Ok(p) = gst::parse::launch(&desc) else { return false };
        let ok = if p.set_state(gst::State::Playing).is_err() {
            false
        } else {
            let bus = p.bus().unwrap();
            let mut ok = false;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            while std::time::Instant::now() < deadline {
                if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(200)) {
                    match msg.view() {
                        gst::MessageView::Eos(_) => { ok = true; break; }
                        gst::MessageView::Error(_) => break,
                        _ => {}
                    }
                }
            }
            ok
        };
        let _ = p.set_state(gst::State::Null);
        ok
    })();
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(name.to_string(), ok);
    ok
}

/// Passenden Encoder wählen: (Elementname, läuft auf der GPU).
pub fn pick_encoder(codec: Codec, hardware: bool) -> Option<(String, bool)> {
    if hardware {
        for n in codec.hardware() { if encoder_works(n) { return Some((n.to_string(), true)); } }
    }
    for n in codec.software() { if encoder_works(n) { return Some((n.to_string(), false)); } }
    None
}

/// Encoder-Eigenschaften nur setzen, wenn der Encoder sie kennt.
pub fn encoder_settings(name: &str, kbps: u32, fps: u32, fast: bool) -> Vec<(String, String)> {
    let Some(f) = gst::ElementFactory::find(name) else { return vec![] };
    let Ok(e) = f.create().build() else { return vec![] };
    let mut out = Vec::new();
    let mut put = |prop: &str, val: String| { if e.find_property(prop).is_some() { out.push((prop.to_string(), val)); } };
    match name {
        "x264enc" if fast => {
            // Aufnahme: feste Qualität statt fester Datenmenge. Ein ruhiger Bildschirm braucht dann fast keine Daten
            // und kaum Rechenzeit; `bitrate` ist nur die Obergrenze für sehr bewegte Bilder (ohne sie gälte der
            // Standardwert von 2 Mbit/s als Grenze). „ultrafast“ verzichtet auf CABAC und Deblocking: Das halbiert
            // die Rechenzeit beim Kodieren und macht die Datei beim Bearbeiten mehrfach leichter zu dekodieren.
            put("speed-preset", "ultrafast".into());
            put("pass", "qual".into());
            put("quantizer", "21".into());
            put("bitrate", kbps.to_string());
            put("vbv-buf-capacity", "2000".into());
            put("key-int-max", fps.to_string());
            put("bframes", "0".into());
        }
        "x264enc" => {
            put("bitrate", kbps.to_string());
            put("speed-preset", "medium".to_string());
            put("key-int-max", (fps * 5).to_string());
        }
        "x265enc" => {
            put("bitrate", kbps.to_string());
            put("speed-preset", if fast { "superfast" } else { "medium" }.to_string());
            put("key-int-max", (fps * if fast { 1 } else { 5 }).to_string());
        }
        "svtav1enc" => {
            put("target-bitrate", kbps.to_string());
            put("preset", if fast { "10" } else { "8" }.to_string());
        }
        "openh264enc" => put("bitrate", (kbps * 1000).to_string()),
        _ => {
            // Hardware-Encoder (VA, NVENC, QSV): Bitrate in kbit/s, kurzer Schlüsselbildabstand bei Aufnahmen
            put("bitrate", kbps.to_string());
            if name.starts_with("va") && !name.starts_with("vaapi") {
                put("rate-control", "vbr".into());
                put("key-int-max", (fps * if fast { 1 } else { 5 }).to_string());
            } else if name.starts_with("nv") {
                put("gop-size", (fps * if fast { 1 } else { 5 }).to_string());
            } else if name.starts_with("vaapi") {
                put("keyframe-period", (fps * if fast { 1 } else { 5 }).to_string());
            }
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct ExportOpts {
    pub path: String,
    pub height: u32,
    pub codec: Codec,
    pub hardware: bool,
    /// 0 = gut, 1 = hoch, 2 = sehr hoch
    pub quality: u32,
}

pub fn export_bitrate(w: u32, h: u32, fps: u32, codec: Codec, quality: u32) -> u32 {
    // Richtwert: 1080p30 H.264 = 8 / 12 / 20 Mbit/s
    let base = [8000.0, 12000.0, 20000.0][quality.min(2) as usize];
    let px = (w as f64 * h as f64) / (1920.0 * 1080.0);
    let f = (fps as f64 / 30.0).powf(0.6);
    let c = match codec { Codec::H264 => 1.0, Codec::Hevc => 0.65, Codec::Av1 => 0.55 };
    (base * px.powf(0.85) * f * c).round().max(500.0) as u32
}

pub struct ExportJob {
    pub pipeline: ges::Pipeline,
    _builder: Builder,
    pub duration: f64,
    pub encoder: String,
    pub on_gpu: bool,
    pub path: String,
}

impl ExportJob {
    pub fn start(p: &Project, o: &ExportOpts) -> anyhow::Result<ExportJob> {
        let duration = p.duration();
        if duration <= 0.0 { anyhow::bail!("Das Projekt ist leer."); }
        let scale = o.height as f64 / p.height.max(1) as f64;
        let mut builder = Builder::new(p, scale);
        builder.sync(p);
        builder.timeline.commit_sync();
        let (w, h) = Builder::out_size(p, scale);
        let (enc, on_gpu) = pick_encoder(o.codec, o.hardware).ok_or_else(|| anyhow::anyhow!("Kein Encoder für {} gefunden. Bitte die GStreamer-Plugins installieren (siehe LIESMICH).", o.codec.label()))?;
        let kbps = export_bitrate(w, h, p.fps, o.codec, o.quality);
        let mkv = o.path.to_lowercase().ends_with(".mkv");
        let container_caps = if mkv { gst::Caps::builder("video/x-matroska").build() } else { gst::Caps::builder("video/quicktime").field("variant", "iso").build() };
        let vcaps = o.codec.caps();
        // Farbformat 4:2:0 vorgeben. Ohne Vorgabe handeln die Software-Encoder das verlustärmste Format aus (4:4:4),
        // das Fernseher, Telefone und viele Abspielprogramme gar nicht oder nur ruckelnd wiedergeben.
        let raw = gst::Caps::builder("video/x-raw").field("format", gst::List::new(["I420", "NV12"])).build();
        let mut vprof = gst_pbutils::EncodingVideoProfile::builder(&vcaps).preset_name(&enc).presence(0);
        if !on_gpu { vprof = vprof.restriction(&raw); }
        let settings = encoder_settings(&enc, kbps, p.fps, false);
        let aac = ["fdkaacenc", "avenc_aac", "voaacenc", "faac"].iter().any(|n| gst::ElementFactory::find(n).is_some());
        let acaps = if aac && !mkv { gst::Caps::builder("audio/mpeg").field("mpegversion", 4i32).build() } else { gst::Caps::builder("audio/x-opus").build() };
        let aprof = gst_pbutils::EncodingAudioProfile::builder(&acaps).presence(0).build();
        let container = gst_pbutils::EncodingContainerProfile::builder(&container_caps).add_profile(vprof.build()).add_profile(aprof).build();
        let pipeline = ges::Pipeline::new();
        // Encoder-Einstellungen setzen, sobald encodebin den Encoder anlegt
        let enc_name = enc.clone();
        pipeline.connect_deep_element_added(move |_, _, el| {
            if el.factory().map(|f| f.name() == enc_name.as_str()).unwrap_or(false) {
                for (k, v) in &settings { crate::recorder::session::safe_set(el, k, v); }
            }
        });
        pipeline.set_timeline(&builder.timeline)?;
        pipeline.set_render_settings(&path_uri(&o.path), &container)?;
        pipeline.set_mode(ges::PipelineFlags::RENDER)?;
        pipeline.set_state(gst::State::Playing)?;
        Ok(ExportJob { pipeline, _builder: builder, duration, encoder: enc, on_gpu, path: o.path.clone() })
    }
    /// Fortschritt 0..1
    pub fn progress(&self) -> f64 {
        self.pipeline.query_position::<gst::ClockTime>().map(|t| (secs(t) / self.duration).clamp(0.0, 1.0)).unwrap_or(0.0)
    }
    /// None = läuft noch, Some(Ok) = fertig, Some(Err) = Fehler
    pub fn poll(&self) -> Option<Result<(), String>> {
        let bus = self.pipeline.bus()?;
        while let Some(msg) = bus.pop() {
            match msg.view() {
                gst::MessageView::Eos(_) => { let _ = self.pipeline.set_state(gst::State::Null); return Some(Ok(())); }
                gst::MessageView::Error(e) => {
                    let _ = self.pipeline.set_state(gst::State::Null);
                    return Some(Err(format!("{} ({})", e.error(), e.debug().map(|d| d.to_string()).unwrap_or_default())));
                }
                _ => {}
            }
        }
        None
    }
    pub fn cancel(&self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        let _ = std::fs::remove_file(&self.path);
    }
}
