//! Projektmodell von WS.Video. Das Modell ist die Quelle der Wahrheit; die Schnitt-Engine
//! (GStreamer Editing Services) wird daraus aufgebaut.
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE_EXT: &str = "wsvideo";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Video,
    Audio,
    Image,
    Text,
    Shape,
    Arrow,
    Blur,
}

impl Kind {
    pub fn is_media(self) -> bool { matches!(self, Kind::Video | Kind::Audio | Kind::Image) }
    pub fn is_annotation(self) -> bool { matches!(self, Kind::Text | Kind::Shape | Kind::Arrow) }
    pub fn is_visual(self) -> bool { self != Kind::Audio }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Video => "Video",
            Kind::Audio => "Ton",
            Kind::Image => "Bild",
            Kind::Text => "Text",
            Kind::Shape => "Form",
            Kind::Arrow => "Pfeil",
            Kind::Blur => "Weichzeichner",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Rect,
    Rounded,
    Ellipse,
}

/// Eigene Farbe für einen Teil des Textes (Zeichenpositionen, Ende ausgeschlossen)
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct TextColor {
    pub start: u32,
    pub end: u32,
    pub color: String,
}

/// Hintergrund hinter der Person im Kamerabild
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum BgMode {
    /// Bild unverändert
    #[default]
    Off,
    /// Hintergrund durchsichtig: Darunterliegende Spuren scheinen durch
    Transparent,
    /// Hintergrund weichgezeichnet
    Blur,
    /// eigenes Hintergrundbild (`bg_image`)
    Image,
}
impl BgMode {
    pub fn index(self) -> u32 { match self { BgMode::Off => 0, BgMode::Transparent => 1, BgMode::Blur => 2, BgMode::Image => 3 } }
    pub fn from_index(i: u32) -> BgMode { match i { 1 => BgMode::Transparent, 2 => BgMode::Blur, 3 => BgMode::Image, _ => BgMode::Off } }
}

/// Art des Ein- oder Ausblendens
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum FadeKind {
    /// weich über die Deckkraft
    #[default]
    Blend,
    /// von links aufrollen bzw. nach links zusammenrollen
    Roll,
    /// über Schwarz bzw. Weiß (das Bild färbt sich ein, die Deckkraft bleibt)
    Black,
    White,
    /// Zoom-Übergang: hinein in den ausgehenden, heraus aus dem kommenden Clip
    Zoom,
    /// schneller Schwenk mit Bewegungsunschärfe; die Richtung ist die Bewegung des Bildes
    WhipLeft,
    WhipRight,
    WhipUp,
    WhipDown,
    /// unscharf werden bzw. scharf werden
    Blur,
    /// digitale Störung mit versetzten Farbkanälen
    Glitch,
    /// hinein- bzw. hinausgleiten; die Seite, von der das Objekt kommt bzw. zu der es geht
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    /// aufspringen mit kurzem Überschwingen bzw. zusammenschrumpfen
    Pop,
}

impl FadeKind {
    /// Reihenfolge in den Auswahllisten: Art, Name am Anfang, Name am Ende
    pub const ALL: [(FadeKind, &'static str, &'static str); 16] = [
        (FadeKind::Blend, "Weich einblenden", "Weich ausblenden"),
        (FadeKind::Roll, "Aufrollen", "Zusammenrollen"),
        (FadeKind::Black, "Aus Schwarz", "Nach Schwarz"),
        (FadeKind::White, "Aus Weiß", "Nach Weiß"),
        (FadeKind::Zoom, "Zoom heraus", "Zoom hinein"),
        (FadeKind::WhipLeft, "Schwenk nach links", "Schwenk nach links"),
        (FadeKind::WhipRight, "Schwenk nach rechts", "Schwenk nach rechts"),
        (FadeKind::WhipUp, "Schwenk nach oben", "Schwenk nach oben"),
        (FadeKind::WhipDown, "Schwenk nach unten", "Schwenk nach unten"),
        (FadeKind::Blur, "Scharf werden", "Unscharf werden"),
        (FadeKind::Glitch, "Störung (Glitch)", "Störung (Glitch)"),
        (FadeKind::SlideLeft, "Von links gleiten", "Nach links gleiten"),
        (FadeKind::SlideRight, "Von rechts gleiten", "Nach rechts gleiten"),
        (FadeKind::SlideUp, "Von oben gleiten", "Nach oben gleiten"),
        (FadeKind::SlideDown, "Von unten gleiten", "Nach unten gleiten"),
        (FadeKind::Pop, "Aufspringen", "Schrumpfen"),
    ];
    pub fn index(self) -> u32 { FadeKind::ALL.iter().position(|k| k.0 == self).unwrap_or(0) as u32 }
    pub fn from_index(i: u32) -> FadeKind { FadeKind::ALL.get(i as usize).map(|k| k.0).unwrap_or_default() }
    /// Wird im Bild selbst gerechnet (Element `wsfx`)
    pub fn is_pixel(self) -> bool {
        matches!(self, FadeKind::Black | FadeKind::White | FadeKind::Zoom | FadeKind::WhipLeft | FadeKind::WhipRight | FadeKind::WhipUp | FadeKind::WhipDown | FadeKind::Blur | FadeKind::Glitch)
    }
    /// Bewegt oder skaliert das Objekt als Ganzes
    pub fn is_motion(self) -> bool {
        matches!(self, FadeKind::SlideLeft | FadeKind::SlideRight | FadeKind::SlideUp | FadeKind::SlideDown | FadeKind::Pop)
    }
}

/// Eine Animation (Zoom & Schwenk): ab `t` (Quellzeit des Clips) bewegt sich der Clip
/// innerhalb von `dur` Sekunden zum Zielrechteck.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Anim {
    pub t: f64,
    pub dur: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct CursorFx {
    /// Pfad zur Cursor-Datei der Aufnahme
    pub data: String,
    /// Index des aufgenommenen Monitors in der Cursor-Datei (-1 = automatisch)
    pub monitor: i32,
    pub show: bool,
    pub scale: f64,
    pub smooth: f64,
    pub highlight: bool,
    pub highlight_color: String,
    pub clicks: bool,
    pub click_color: String,
}
impl Default for CursorFx {
    fn default() -> Self {
        CursorFx {
            data: String::new(),
            monitor: -1,
            show: true,
            scale: 1.0,
            smooth: 0.0,
            highlight: false,
            highlight_color: "#ffd23d".into(),
            clicks: true,
            click_color: "#ff8a3d".into(),
        }
    }
}

/// Tonbearbeitung eines Clips. Gemessene Werte (Rauschprofil, Lautstärke) liegen je Quelle im Projekt.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct AudioFx {
    /// Rauschen entfernen, 0 = aus, 1 = stark
    pub denoise: f64,
    /// tiefes Brummen und Poltern entfernen (unter 80 Hz)
    pub lowcut: bool,
    /// Pausen leiser, 0 = aus
    pub gate: f64,
    /// Lautstärke angleichen, 0 = aus
    pub compress: f64,
    /// auf normale Lautstärke bringen
    pub normalize: bool,
    pub voice: bool,
    /// Tiefen und Höhen in dB
    pub bass: f64,
    pub treble: f64,
}

impl AudioFx {
    pub fn active(&self) -> bool {
        self.denoise > 0.001 || self.lowcut || self.gate > 0.001 || self.compress > 0.001 || self.normalize || self.voice || self.bass.abs() > 0.05 || self.treble.abs() > 0.05
    }
    /// Braucht gemessene Werte der Quelle
    pub fn needs_profile(&self) -> bool { self.denoise > 0.001 || self.gate > 0.001 || self.compress > 0.001 || self.normalize }
    /// Ein Klick: Rauschen weg, Poltern weg, Lautstärke angleichen und auf Normalmaß
    pub fn improved() -> AudioFx { AudioFx { denoise: 0.6, lowcut: true, gate: 0.0, compress: 0.4, normalize: true, voice: false, bass: 0.0, treble: 0.0 } }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Clip {
    pub id: u64,
    pub track: u64,
    pub kind: Kind,
    pub name: String,
    pub path: String,
    /// Position auf der Zeitleiste (Sekunden)
    pub start: f64,
    pub duration: f64,
    /// Anfang innerhalb der Quelle
    pub inpoint: f64,
    /// Länge der Quelle (0 = unbegrenzt, z. B. Bilder)
    pub media_duration: f64,
    pub has_video: bool,
    pub has_audio: bool,
    pub src_w: u32,
    pub src_h: u32,
    /// Sichtbares Rechteck auf der Arbeitsfläche (Projektpixel)
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub opacity: f64,
    /// Zuschnitt links, oben, rechts, unten (Anteil der Quelle, 0..1)
    pub crop: [f64; 4],
    pub shape: Shape,
    pub radius: f64,
    pub border: f64,
    pub border_color: String,
    pub shadow: bool,
    pub fade_in: f64,
    pub fade_out: f64,
    #[serde(default)]
    pub fade_in_kind: FadeKind,
    #[serde(default)]
    pub fade_out_kind: FadeKind,
    pub volume: f64,
    pub muted: bool,
    pub audio: AudioFx,
    pub chroma: bool,
    pub chroma_color: String,
    pub chroma_tolerance: f64,
    pub anims: Vec<Anim>,
    // Anmerkungen
    pub text: String,
    pub font: String,
    pub font_size: f64,
    pub bold: bool,
    pub italic: bool,
    pub align: i32,
    pub color: String,
    pub fill: String,
    pub stroke: String,
    pub stroke_w: f64,
    /// Text: Kontur um die Buchstaben (Breite in Projektpixeln) und ihre Farbe
    pub outline: f64,
    pub outline_color: String,
    #[serde(default)]
    pub bg_mode: BgMode,
    #[serde(default)]
    pub bg_image: String,
    #[serde(default)]
    pub text_colors: Vec<TextColor>,
    /// Pfeil: Richtung innerhalb des Rechtecks (0 = links oben nach rechts unten, 1 = links unten nach rechts oben), Spitze am Ende/Anfang
    pub arrow_dir: i32,
    pub arrow_head: bool,
    pub blur: f64,
    /// Weichzeichner-Bereich: verpixeln statt weichzeichnen
    #[serde(default)]
    pub pixelate: bool,
    pub cursor: Option<CursorFx>,
    /// Farbkorrektur, jeweils -1..1 (0 = unverändert)
    #[serde(default)]
    pub brightness: f64,
    #[serde(default)]
    pub contrast: f64,
    #[serde(default)]
    pub saturation: f64,
    #[serde(default)]
    pub temperature: f64,
    /// Abdunklung der Bildränder, 0..1
    #[serde(default)]
    pub vignette: f64,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
}

impl Default for Clip {
    fn default() -> Self {
        Clip {
            id: 0,
            track: 0,
            kind: Kind::Video,
            name: String::new(),
            path: String::new(),
            start: 0.0,
            duration: 5.0,
            inpoint: 0.0,
            media_duration: 0.0,
            has_video: false,
            has_audio: false,
            src_w: 0,
            src_h: 0,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            opacity: 1.0,
            crop: [0.0; 4],
            shape: Shape::Rect,
            radius: 0.25,
            border: 0.0,
            border_color: "#ffffff".into(),
            shadow: false,
            fade_in: 0.0,
            fade_out: 0.0,
            fade_in_kind: FadeKind::Blend,
            fade_out_kind: FadeKind::Blend,
            volume: 1.0,
            muted: false,
            audio: AudioFx::default(),
            chroma: false,
            chroma_color: "#00d000".into(),
            chroma_tolerance: 0.3,
            anims: Vec::new(),
            text: String::new(),
            font: "Sans".into(),
            font_size: 64.0,
            bold: true,
            italic: false,
            align: 1,
            color: "#ffffff".into(),
            fill: "#ff8a3d".into(),
            stroke: "#ffffff".into(),
            stroke_w: 0.0,
            outline: 0.0,
            outline_color: "#000000".into(),
            text_colors: Vec::new(),
            bg_mode: BgMode::Off,
            bg_image: String::new(),
            arrow_dir: 0,
            arrow_head: true,
            blur: 0.5,
            pixelate: false,
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            temperature: 0.0,
            vignette: 0.0,
            flip_h: false,
            flip_v: false,
            cursor: None,
        }
    }
}

impl Clip {
    pub fn end(&self) -> f64 { self.start + self.duration }
    pub fn is_visual(&self) -> bool {
        match self.kind {
            Kind::Audio => false,
            Kind::Video => self.has_video,
            _ => true,
        }
    }
    pub fn carries_audio(&self) -> bool { matches!(self.kind, Kind::Video | Kind::Audio) && self.has_audio }
    /// Quellzeit für eine Zeitleistenzeit
    pub fn source_time(&self, t: f64) -> f64 { t - self.start + self.inpoint }
    /// Seitenverhältnis des sichtbaren (zugeschnittenen) Quellbereichs
    pub fn visible_aspect(&self) -> f64 {
        let cw = (1.0 - self.crop[0] - self.crop[2]).max(0.01) * self.src_w.max(1) as f64;
        let ch = (1.0 - self.crop[1] - self.crop[3]).max(0.01) * self.src_h.max(1) as f64;
        cw / ch
    }
    /// Rechteck (x, y, w, h) zur Quellzeit `st` unter Berücksichtigung der Animationen.
    pub fn rect_at_source(&self, st: f64) -> (f64, f64, f64, f64) {
        let mut r = (self.x, self.y, self.w, self.h);
        let mut anims: Vec<&Anim> = self.anims.iter().collect();
        anims.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        for a in anims {
            if st <= a.t { break; }
            let target = (a.x, a.y, a.w, a.h);
            if a.dur <= 0.0 || st >= a.t + a.dur {
                r = target;
            } else {
                let k = ease((st - a.t) / a.dur);
                r = (lerp(r.0, target.0, k), lerp(r.1, target.1, k), lerp(r.2, target.2, k), lerp(r.3, target.3, k));
                break;
            }
        }
        r
    }
    pub fn rect_at(&self, t: f64) -> (f64, f64, f64, f64) { self.rect_at_source(self.source_time(t)) }
    /// Index der Animation, deren Zielzustand zur Quellzeit `st` gilt (für die Bearbeitung auf der Arbeitsfläche).
    pub fn anim_index_at(&self, st: f64) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, a) in self.anims.iter().enumerate() {
            if a.t <= st + 1e-6 {
                match best {
                    Some(b) if self.anims[b].t > a.t => {}
                    _ => best = Some(i),
                }
            }
        }
        best
    }
    /// Braucht der Clip das Element `wsfx` (Farbe, Spiegeln, Vignette, im Bild gerechnete Übergänge)?
    pub fn needs_fx(&self) -> bool {
        if !self.is_visual() || self.kind == Kind::Blur { return false; }
        self.has_color() || self.flip_h || self.flip_v || self.vignette > 0.001
            || (self.fade_in > 0.0 && self.fade_in_kind.is_pixel()) || (self.fade_out > 0.0 && self.fade_out_kind.is_pixel())
    }
    pub fn has_color(&self) -> bool {
        self.brightness.abs() > 0.001 || self.contrast.abs() > 0.001 || self.saturation.abs() > 0.001 || self.temperature.abs() > 0.001
    }
    /// Lage und Größe mit Hinein-/Hinausgleiten und Aufspringen. `rel` = Sekunden seit Beginn des Clips,
    /// `r` = Rechteck ohne diese Bewegung, `pw`/`ph` = Projektgröße.
    pub fn motion_rect(&self, r: (f64, f64, f64, f64), rel: f64, pw: f64, ph: f64) -> (f64, f64, f64, f64) {
        let mut r = r;
        let mut one = |kind: FadeKind, k: f64, enter: bool| {
            // k: 0 = ganz draußen bzw. unsichtbar, 1 = an seinem Platz
            let k = k.clamp(0.0, 1.0);
            match kind {
                FadeKind::SlideLeft | FadeKind::SlideRight | FadeKind::SlideUp | FadeKind::SlideDown => {
                    // schnell von außen kommen und abbremsen; beim Gehen umgekehrt anfahren und beschleunigen
                    let away = (1.0 - k).powi(3);
                    match kind {
                        FadeKind::SlideLeft => r.0 -= (r.0 + r.2) * away,
                        FadeKind::SlideRight => r.0 += (pw - r.0) * away,
                        FadeKind::SlideUp => r.1 -= (r.1 + r.3) * away,
                        _ => r.1 += (ph - r.1) * away,
                    }
                }
                FadeKind::Pop => {
                    let f = if enter {
                        if k < 0.6 { ease(k / 0.6) * 1.12 } else { 1.12 - 0.12 * ease((k - 0.6) / 0.4) }
                    } else { ease(k) };
                    let f = f.max(0.02);
                    let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
                    r = (cx - r.2 * f / 2.0, cy - r.3 * f / 2.0, r.2 * f, r.3 * f);
                }
                _ => {}
            }
        };
        let d = self.duration;
        if self.fade_in > 0.0 && self.fade_in_kind.is_motion() && rel < self.fade_in { one(self.fade_in_kind, rel / self.fade_in, true); }
        if self.fade_out > 0.0 && self.fade_out_kind.is_motion() && rel > d - self.fade_out { one(self.fade_out_kind, (d - rel) / self.fade_out, false); }
        r
    }
    pub fn has_motion(&self) -> bool {
        (self.fade_in > 0.0 && self.fade_in_kind.is_motion()) || (self.fade_out > 0.0 && self.fade_out_kind.is_motion())
    }
    pub fn opacity_at(&self, t: f64) -> f64 {
        let rel = t - self.start;
        let mut o = self.opacity;
        if self.fade_in > 0.0 && self.fade_in_kind == FadeKind::Blend && rel < self.fade_in { o *= (rel / self.fade_in).clamp(0.0, 1.0); }
        if self.fade_out > 0.0 && self.fade_out_kind == FadeKind::Blend && rel > self.duration - self.fade_out { o *= ((self.duration - rel) / self.fade_out).clamp(0.0, 1.0); }
        o
    }
}

pub fn lerp(a: f64, b: f64, k: f64) -> f64 { a + (b - a) * k }
/// Weiches Anfahren und Abbremsen
pub fn ease(k: f64) -> f64 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub muted: bool,
    pub hidden: bool,
    pub locked: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct MediaItem {
    pub path: String,
    pub name: String,
    pub kind: Kind,
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub has_video: bool,
    pub has_audio: bool,
    /// Cursor-Datei der Aufnahme (falls vorhanden)
    pub cursor: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Project {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub background: String,
    /// Spuren von unten (Index 0) nach oben
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    pub media: Vec<MediaItem>,
    /// Gemessene Tonwerte je Quelldatei (Rauschprofil, Lautstärke)
    pub audio_profiles: std::collections::BTreeMap<String, crate::audiofx::Profile>,
    pub next_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        let mut p = Project {
            version: 3,
            width: 1920,
            height: 1080,
            fps: 30,
            background: "#000000".into(),
            tracks: Vec::new(),
            clips: Vec::new(),
            media: Vec::new(),
            audio_profiles: Default::default(),
            next_id: 1,
        };
        p.add_track("Spur 1");
        p.add_track("Spur 2");
        p
    }
}

impl Project {
    pub fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn add_track(&mut self, name: &str) -> u64 {
        let id = self.new_id();
        self.tracks.push(Track { id, name: name.to_string(), ..Default::default() });
        id
    }
    pub fn track_index(&self, id: u64) -> Option<usize> { self.tracks.iter().position(|t| t.id == id) }
    pub fn track(&self, id: u64) -> Option<&Track> { self.tracks.iter().find(|t| t.id == id) }
    pub fn clip(&self, id: u64) -> Option<&Clip> { self.clips.iter().find(|c| c.id == id) }
    pub fn clip_mut(&mut self, id: u64) -> Option<&mut Clip> { self.clips.iter_mut().find(|c| c.id == id) }
    pub fn duration(&self) -> f64 { self.clips.iter().map(|c| c.end()).fold(0.0, f64::max) }
    pub fn clips_on(&self, track: u64) -> Vec<&Clip> {
        let mut v: Vec<&Clip> = self.clips.iter().filter(|c| c.track == track).collect();
        v.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        v
    }
    /// Sichtbare Clips zur Zeit `t`, von oben nach unten.
    pub fn visual_clips_at(&self, t: f64) -> Vec<&Clip> {
        let mut out = Vec::new();
        for tr in self.tracks.iter().rev() {
            if tr.hidden { continue; }
            let mut on: Vec<&Clip> = self.clips.iter().filter(|c| c.track == tr.id && c.is_visual() && t >= c.start && t < c.end()).collect();
            on.sort_by(|a, b| b.start.partial_cmp(&a.start).unwrap());
            out.extend(on);
        }
        out
    }
    /// Prüft, ob `clip` (ggf. noch nicht eingefügt) auf seiner Spur gültig liegt:
    /// Überlappungen sind nur mit direkten Nachbarn erlaubt (Überblendung), kein Clip darf
    /// einen anderen ganz überdecken, und nie dürfen drei Clips gleichzeitig laufen.
    pub fn placement_ok(&self, clip: &Clip) -> bool {
        let mut v: Vec<(f64, f64)> = self.clips.iter().filter(|c| c.track == clip.track && c.id != clip.id).map(|c| (c.start, c.end())).collect();
        v.push((clip.start, clip.end()));
        v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let eps = 1e-6;
        for i in 0..v.len() {
            if i + 1 < v.len() {
                // kein Clip darf vollständig im vorherigen liegen
                if v[i + 1].1 <= v[i].1 + eps && v[i + 1].0 < v[i].1 - eps { return false; }
                if (v[i + 1].0 - v[i].0).abs() < eps && v[i + 1].0 < v[i].1 - eps { return false; }
            }
            if i + 2 < v.len() && v[i + 2].0 < v[i].1 - eps { return false; }
        }
        true
    }
    /// Freie Spur für den Zeitraum suchen (von unten), sonst neue Spur anlegen.
    pub fn free_track(&mut self, start: f64, dur: f64, prefer: Option<u64>) -> u64 {
        let free = |p: &Project, tid: u64| !p.clips.iter().any(|c| c.track == tid && c.start < start + dur - 1e-6 && c.end() > start + 1e-6);
        if let Some(t) = prefer {
            if self.track(t).map(|t| !t.locked).unwrap_or(false) && free(self, t) { return t; }
        }
        let ids: Vec<u64> = self.tracks.iter().filter(|t| !t.locked).map(|t| t.id).collect();
        for id in ids {
            if free(self, id) { return id; }
        }
        let n = self.tracks.len() + 1;
        self.add_track(&format!("Spur {n}"))
    }
    /// Alle Dateipfade des Projekts umschreiben (Medien, Hintergrundbilder, Zeigerdaten).
    pub fn map_paths(&mut self, f: &dyn Fn(&str) -> String) {
        let g = |s: &mut String| { if !s.is_empty() { *s = f(s); } };
        for c in &mut self.clips {
            g(&mut c.path);
            g(&mut c.bg_image);
            if let Some(k) = c.cursor.as_mut() { g(&mut k.data); }
        }
        for m in &mut self.media {
            g(&mut m.path);
            g(&mut m.cursor);
        }
        self.audio_profiles = std::mem::take(&mut self.audio_profiles).into_iter().map(|(k, v)| (f(&k), v)).collect();
    }

    /// Projekt speichern. Ein Projekt ist ein Ordner: Was darin liegt, steht in der Datei mit einem Pfad relativ
    /// zum Ordner. So bleibt das Projekt vollständig, wenn der Ordner verschoben, kopiert oder gesichert wird.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        if !dir.as_os_str().is_empty() { std::fs::create_dir_all(&dir)?; }
        let mut out = self.clone();
        out.map_paths(&|p| match Path::new(p).strip_prefix(&dir) {
            Ok(rel) if !dir.as_os_str().is_empty() => rel.to_string_lossy().to_string(),
            _ => p.to_string(),
        });
        let s = serde_json::to_string_pretty(&out)?;
        let tmp = path.with_extension("wsvideo.tmp");
        std::fs::write(&tmp, s)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
    pub fn load(path: &Path) -> anyhow::Result<Project> {
        let s = std::fs::read_to_string(path)?;
        let mut p: Project = serde_json::from_str(&s)?;
        if p.tracks.is_empty() { p.add_track("Spur 1"); }
        let max = p.clips.iter().map(|c| c.id).chain(p.tracks.iter().map(|t| t.id)).max().unwrap_or(0);
        if p.next_id <= max { p.next_id = max + 1; }
        // Im Speicher sind alle Pfade vollständig. Fehlt eine Datei an ihrem alten Ort (der Ordner wurde
        // verschoben oder stammt von einem anderen Rechner), wird sie im Projektordner gesucht.
        let dir = std::fs::canonicalize(path).ok().and_then(|f| f.parent().map(|d| d.to_path_buf())).or_else(|| path.parent().map(|d| d.to_path_buf())).unwrap_or_default();
        p.map_paths(&|f| {
            let fp = Path::new(f);
            if fp.is_relative() { return dir.join(fp).to_string_lossy().to_string(); }
            if fp.exists() { return f.to_string(); }
            if let Some(name) = fp.file_name() {
                for cand in [dir.join(name), dir.join(MEDIA_DIR).join(name)] {
                    if cand.exists() { return cand.to_string_lossy().to_string(); }
                }
            }
            f.to_string()
        });
        Ok(p)
    }
}

/// Unterordner eines Projekts für importierte Dateien
pub const MEDIA_DIR: &str = "medien";

/// Datei in einen Ordner übernehmen. Liefert den neuen Pfad. Liegt dort schon dieselbe Datei, wird sie
/// wiederverwendet; bei gleichem Namen und anderem Inhalt bekommt die neue eine Nummer.
/// Große Dateien werden nach Möglichkeit nur verknüpft (kein zweiter Platzverbrauch, sofort fertig).
pub fn adopt_file(src: &Path, dir: &Path) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let meta = std::fs::metadata(src)?;
    let name = src.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "datei".into());
    let stem = src.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "datei".into());
    let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut dst = dir.join(&name);
    let mut n = 1;
    loop {
        match std::fs::metadata(&dst) {
            Err(_) => break,
            Ok(m) => {
                use std::os::unix::fs::MetadataExt;
                let same_file = m.dev() == meta.dev() && m.ino() == meta.ino();
                if same_file || (m.len() == meta.len() && m.modified().ok() == meta.modified().ok()) { return Ok(dst); }
            }
        }
        n += 1;
        dst = dir.join(format!("{stem}-{n}{ext}"));
    }
    let big = meta.len() > 100 * 1024 * 1024;
    if big && std::fs::hard_link(src, &dst).is_ok() { return Ok(dst); }
    std::fs::copy(src, &dst)?;
    // Änderungszeit übernehmen: daran wird dieselbe Datei beim nächsten Mal wiedererkannt
    if let (Ok(f), Ok(t)) = (std::fs::File::options().write(true).open(&dst), meta.modified()) { let _ = f.set_modified(t); }
    Ok(dst)
}

pub fn parse_color(s: &str) -> (f64, f64, f64, f64) {
    let h = s.trim().trim_start_matches('#');
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64 / 255.0;
    match h.len() {
        6 => (v(0), v(2), v(4), 1.0),
        8 => (v(0), v(2), v(4), v(6)),
        _ => (0.0, 0.0, 0.0, 1.0),
    }
}
pub fn color_u32(s: &str) -> u32 {
    let (r, g, b, _) = parse_color(s);
    (((r * 255.0).round() as u32) << 16) | (((g * 255.0).round() as u32) << 8) | ((b * 255.0).round() as u32)
}
pub fn color_hex(r: f64, g: f64, b: f64, a: f64) -> String {
    let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a >= 0.999 { format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b)) } else { format!("#{:02x}{:02x}{:02x}{:02x}", c(r), c(g), c(b), c(a)) }
}

pub fn format_time(t: f64, fps: u32) -> String {
    let t = t.max(0.0);
    let total = t.floor() as u64;
    let frames = ((t - t.floor()) * fps as f64).floor() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02};{frames:02}") } else { format!("{m:02}:{s:02};{frames:02}") }
}
pub fn format_dur(t: f64) -> String {
    let total = t.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}
