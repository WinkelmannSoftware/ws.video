//! Tonbearbeitung: Rauschminderung mit gemessenem Rauschprofil, Tiefensperre, Pausen absenken, Klang,
//! Lautstärke angleichen, Pegel und Begrenzer. Reine Rechenarbeit ohne GStreamer, damit sie sich einzeln
//! prüfen lässt. Das Element `wsaudio` (elements/audio.rs) setzt sie in die Schnittkette.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// Fensterlänge und Schritt der Kurzzeit-Spektren. Das Rauschprofil bezieht sich auf 48 kHz.
pub const N: usize = 2048;
pub const HOP: usize = 512;
pub const BINS: usize = N / 2 + 1;
pub const RATE: f64 = 48000.0;
/// Zielpegel für „normale Lautstärke“ (mittlere Lautstärke der Sprache, dB unter Vollaussteuerung)
pub const TARGET_DB: f32 = -19.0;
/// Obergrenze des Begrenzers
const CEIL_DB: f32 = -1.0;

fn db(x: f32) -> f32 { 10.0 * (x.max(1e-20)).log10() }
fn lin(dbv: f32) -> f32 { 10f32.powf(dbv / 20.0) }

// ---------------------------------------------------------------------------------------------
// Schnelle Fourier-Transformation (Zweierpotenz)

pub struct Fft { n: usize, rev: Vec<usize>, cos: Vec<f32>, sin: Vec<f32> }

impl Fft {
    pub fn new(n: usize) -> Self {
        let bits = n.trailing_zeros();
        let rev = (0..n).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect();
        let cos = (0..n / 2).map(|k| (2.0 * std::f64::consts::PI * k as f64 / n as f64).cos() as f32).collect();
        let sin = (0..n / 2).map(|k| (2.0 * std::f64::consts::PI * k as f64 / n as f64).sin() as f32).collect();
        Fft { n, rev, cos, sin }
    }
    pub fn run(&self, re: &mut [f32], im: &mut [f32], inverse: bool) {
        let n = self.n;
        for i in 0..n { let j = self.rev[i]; if j > i { re.swap(i, j); im.swap(i, j); } }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            let mut start = 0;
            while start < n {
                for k in 0..half {
                    let wr = self.cos[k * step];
                    let wi = if inverse { self.sin[k * step] } else { -self.sin[k * step] };
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr; im[b] = im[a] - ti;
                    re[a] += tr; im[a] += ti;
                }
                start += len;
            }
            len <<= 1;
        }
        if inverse { let s = 1.0 / n as f32; for i in 0..n { re[i] *= s; im[i] *= s; } }
    }
}

fn window() -> Vec<f32> { (0..N).map(|n| (std::f64::consts::PI * n as f64 / N as f64).sin() as f32).collect() }

// ---------------------------------------------------------------------------------------------
// Messung einer Datei

/// Ergebnis der Messung einer Tonquelle.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    /// Rauschen je Frequenzband in dB (leer = kein Rauschen messbar)
    pub noise: Vec<f32>,
    /// Pegel des Grundrauschens, mittlere Lautstärke und Spitze in dB
    pub floor: f32,
    pub loud: f32,
    pub peak: f32,
    /// Das Grundrauschen ist gleichmäßig (Lüfter, Netzbrummen), nicht zufällig die leiseste Stelle von Musik
    pub steady: bool,
}

const BLOCK: usize = 24;
const KEEP: usize = 64;
const SILENT: f32 = 4e-9; // -84 dB: digitale Stille zählt nicht als Rauschen

pub struct Analyzer {
    fft: Fft, win: Vec<f32>, buf: Vec<f32>, fill: usize, re: Vec<f32>, im: Vec<f32>,
    acc: Vec<f64>, sq: f64, frames: usize, energies: Vec<f32>, kept: Vec<(f32, Vec<f32>)>, peak: f32, any: bool,
}

impl Analyzer {
    pub fn new() -> Self {
        Analyzer { fft: Fft::new(N), win: window(), buf: vec![0.0; N], fill: N - HOP, re: vec![0.0; N], im: vec![0.0; N],
            acc: vec![0.0; BINS], sq: 0.0, frames: 0, energies: Vec::new(), kept: Vec::new(), peak: 0.0, any: false }
    }
    /// Mono-Samples mit 48 kHz
    pub fn push(&mut self, samples: &[f32]) {
        for &x in samples {
            self.any = true;
            self.buf[self.fill] = x;
            self.fill += 1;
            self.sq += (x * x) as f64;
            if x.abs() > self.peak { self.peak = x.abs(); }
            if self.fill == N { self.frame(); }
        }
    }
    fn frame(&mut self) {
        for n in 0..N { self.re[n] = self.buf[n] * self.win[n]; self.im[n] = 0.0; }
        self.fft.run(&mut self.re, &mut self.im, false);
        for k in 0..BINS { self.acc[k] += (self.re[k] * self.re[k] + self.im[k] * self.im[k]) as f64; }
        self.buf.copy_within(HOP.., 0);
        self.fill = N - HOP;
        self.frames += 1;
        if self.frames == BLOCK {
            let e = (self.sq / (BLOCK * HOP) as f64) as f32;
            self.energies.push(e);
            if e > SILENT && (self.kept.len() < KEEP || e < self.kept.last().map(|k| k.0).unwrap_or(f32::MAX)) {
                let spec: Vec<f32> = self.acc.iter().map(|a| (*a / BLOCK as f64) as f32).collect();
                let at = self.kept.partition_point(|k| k.0 <= e);
                self.kept.insert(at, (e, spec));
                self.kept.truncate(KEEP);
            }
            self.acc.iter_mut().for_each(|a| *a = 0.0);
            self.sq = 0.0;
            self.frames = 0;
        }
    }
    pub fn finish(self) -> Option<Profile> {
        if !self.any { return None; }
        let mut p = Profile { peak: 2.0 * db(self.peak), floor: -120.0, loud: -120.0, ..Default::default() };
        // mittlere Lautstärke: nur Abschnitte, in denen wirklich etwas zu hören ist
        let audible: Vec<f32> = self.energies.iter().copied().filter(|e| *e > 1e-7).collect();
        if !audible.is_empty() {
            let mean = audible.iter().sum::<f32>() / audible.len() as f32;
            let gate = mean * 0.1;
            let loud: Vec<f32> = audible.iter().copied().filter(|e| *e > gate).collect();
            if !loud.is_empty() { p.loud = db(loud.iter().sum::<f32>() / loud.len() as f32); }
        } else if !self.energies.is_empty() {
            p.loud = db(self.energies.iter().sum::<f32>() / self.energies.len() as f32);
        }
        // Rauschen: Mittel über die leisesten zehn Prozent der Abschnitte
        let valid = self.energies.iter().filter(|e| **e > SILENT).count();
        let n = ((valid as f64 * 0.1).ceil() as usize).clamp(4, KEEP).min(self.kept.len());
        if n >= 2 {
            p.steady = db(self.kept[n - 1].0) - db(self.kept[n / 4].0) < 5.0;
            // Kein gleichmäßiges Grundrauschen (Musik, kaum Pausen): nur die allerleisesten Stellen zählen,
            // damit die Rauschminderung leise Töne nicht anfasst.
            let n = if p.steady { n } else { n.min(4) };
            let mut noise = vec![0f64; BINS];
            for (_, s) in &self.kept[..n] { for k in 0..BINS { noise[k] += s[k] as f64; } }
            p.noise = noise.iter().map(|x| ((db((*x / n as f64) as f32)) * 10.0).round() / 10.0).collect();
            p.floor = db(self.kept[..n].iter().map(|k| k.0).sum::<f32>() / n as f32);
        }
        Some(p)
    }
}

// ---------------------------------------------------------------------------------------------
// Rauschprofile, die das Element nachschlägt (Schlüssel = Pfad der Quelle)

type Stored = (Vec<f32>, Arc<Vec<f32>>);
static PROFILES: LazyLock<Mutex<HashMap<String, Stored>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn set_profile(key: &str, p: &Profile) {
    let mut m = PROFILES.lock().unwrap();
    if m.get(key).map(|s| s.0 == p.noise).unwrap_or(false) { return; }
    let linear: Vec<f32> = p.noise.iter().map(|d| 10f32.powf(d / 10.0)).collect();
    m.insert(key.to_string(), (p.noise.clone(), Arc::new(linear)));
}
pub fn profile(key: &str) -> Option<Arc<Vec<f32>>> { PROFILES.lock().unwrap().get(key).map(|s| s.1.clone()).filter(|v| v.len() >= 2) }

// ---------------------------------------------------------------------------------------------
// Einstellungen, wie sie das Element als Text bekommt

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// Rauschen entfernen, 0 = aus, 1 = stark
    pub denoise: f32,
    /// Tiefensperre in Hz, 0 = aus
    pub lowcut: f32,
    /// Pausen absenken, 0 = aus
    pub gate: f32,
    /// Lautstärke angleichen, 0 = aus
    pub compress: f32,
    pub voice: bool,
    pub bass: f32,
    pub treble: f32,
    /// Verstärkung in dB (aus „normale Lautstärke“)
    pub gain: f32,
    /// gemessenes Grundrauschen und mittlere Lautstärke der Quelle in dB
    pub floor: f32,
    pub loud: f32,
    pub limit: bool,
    /// Schlüssel des Rauschprofils
    pub profile: String,
}

impl Default for Params {
    fn default() -> Self {
        Params { denoise: 0.0, lowcut: 0.0, gate: 0.0, compress: 0.0, voice: false, bass: 0.0, treble: 0.0, gain: 0.0, floor: -60.0, loud: -24.0, limit: true, profile: String::new() }
    }
}

impl Params {
    pub fn to_text(&self) -> String {
        format!("denoise={:.3};lowcut={:.1};gate={:.3};compress={:.3};voice={};bass={:.2};treble={:.2};gain={:.2};floor={:.1};loud={:.1};limit={};profile={}",
            self.denoise, self.lowcut, self.gate, self.compress, self.voice as u8, self.bass, self.treble, self.gain, self.floor, self.loud, self.limit as u8, self.profile)
    }
    pub fn parse(s: &str) -> Params {
        let mut p = Params::default();
        let (head, profile) = match s.find("profile=") { Some(i) => (&s[..i], &s[i + 8..]), None => (s, "") };
        p.profile = profile.to_string();
        for part in head.split(';') {
            let Some((k, v)) = part.split_once('=') else { continue };
            let f: f32 = v.trim().parse().unwrap_or(0.0);
            match k.trim() {
                "denoise" => p.denoise = f.clamp(0.0, 1.0),
                "lowcut" => p.lowcut = f.clamp(0.0, 400.0),
                "gate" => p.gate = f.clamp(0.0, 1.0),
                "compress" => p.compress = f.clamp(0.0, 1.0),
                "voice" => p.voice = f > 0.5,
                "bass" => p.bass = f.clamp(-15.0, 15.0),
                "treble" => p.treble = f.clamp(-15.0, 15.0),
                "gain" => p.gain = f.clamp(-30.0, 40.0),
                "floor" => p.floor = f,
                "loud" => p.loud = f,
                "limit" => p.limit = f > 0.5,
                _ => {}
            }
        }
        p
    }
}

// ---------------------------------------------------------------------------------------------
// Bausteine

#[derive(Clone, Copy, Default)]
struct Biquad { b0: f32, b1: f32, b2: f32, a1: f32, a2: f32, z1: f32, z2: f32 }

impl Biquad {
    fn make(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Biquad {
        Biquad { b0: (b0 / a0) as f32, b1: (b1 / a0) as f32, b2: (b2 / a0) as f32, a1: (a1 / a0) as f32, a2: (a2 / a0) as f32, z1: 0.0, z2: 0.0 }
    }
    fn highpass(rate: f64, f: f64, q: f64) -> Biquad {
        let w = 2.0 * std::f64::consts::PI * f / rate;
        let (c, al) = (w.cos(), w.sin() / (2.0 * q));
        Biquad::make((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    fn peak(rate: f64, f: f64, q: f64, gain_db: f64) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * std::f64::consts::PI * f / rate;
        let (c, al) = (w.cos(), w.sin() / (2.0 * q));
        Biquad::make(1.0 + al * a, -2.0 * c, 1.0 - al * a, 1.0 + al / a, -2.0 * c, 1.0 - al / a)
    }
    fn shelf(rate: f64, f: f64, gain_db: f64, high: bool) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * std::f64::consts::PI * f / rate;
        let (c, s) = (w.cos(), w.sin());
        let al = s / 2.0 * 2f64.sqrt();
        let r = 2.0 * a.sqrt() * al;
        if high {
            Biquad::make(a * ((a + 1.0) + (a - 1.0) * c + r), -2.0 * a * ((a - 1.0) + (a + 1.0) * c), a * ((a + 1.0) + (a - 1.0) * c - r),
                (a + 1.0) - (a - 1.0) * c + r, 2.0 * ((a - 1.0) - (a + 1.0) * c), (a + 1.0) - (a - 1.0) * c - r)
        } else {
            Biquad::make(a * ((a + 1.0) - (a - 1.0) * c + r), 2.0 * a * ((a - 1.0) - (a + 1.0) * c), a * ((a + 1.0) - (a - 1.0) * c - r),
                (a + 1.0) + (a - 1.0) * c + r, -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - r)
        }
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Rauschminderung eines Kanals: Wiener-Filter je Frequenzband, der Rauschabstand wird über die Zeit geglättet
/// (entscheidungsgestützte Schätzung), damit kein Zwitschern entsteht. Verzögert um genau N Samples.
struct Den { inp: Vec<f32>, fill: usize, ola: Vec<f32>, ready: Vec<f32>, rd: usize, prev: Vec<f32> }

impl Den {
    fn new() -> Den { Den { inp: vec![0.0; N], fill: N - HOP, ola: vec![0.0; N], ready: vec![0.0; HOP], rd: 0, prev: vec![0.0; BINS] } }
}

struct Denoise { fft: Fft, win: Vec<f32>, noise: Vec<f32>, gmin: f32, re: Vec<f32>, im: Vec<f32>, ch: Vec<Den> }

impl Denoise {
    fn new(profile: &[f32], strength: f32, rate: f64, channels: usize) -> Denoise {
        // Profil auf die Bänder dieser Abtastrate umrechnen und etwas großzügiger ansetzen
        let over = 1.0 + 1.2 * strength;
        let noise = (0..BINS).map(|k| {
            let at = k as f64 * rate / RATE;
            let i = (at.round() as usize).min(profile.len() - 1);
            if at > (profile.len() - 1) as f64 + 0.5 { 0.0 } else { profile[i] * over * (rate / RATE) as f32 }
        }).collect();
        Denoise { fft: Fft::new(N), win: window(), noise, gmin: lin(-(8.0 + 22.0 * strength)), re: vec![0.0; N], im: vec![0.0; N], ch: (0..channels).map(|_| Den::new()).collect() }
    }
    #[inline]
    fn run(&mut self, c: usize, x: f32) -> f32 {
        let d = &mut self.ch[c];
        let y = d.ready[d.rd];
        d.inp[d.fill] = x;
        d.fill += 1;
        d.rd += 1;
        if d.fill == N { self.frame(c); }
        y
    }
    fn frame(&mut self, c: usize) {
        let d = &mut self.ch[c];
        for n in 0..N { self.re[n] = d.inp[n] * self.win[n]; self.im[n] = 0.0; }
        self.fft.run(&mut self.re, &mut self.im, false);
        for k in 0..BINS {
            let p = self.re[k] * self.re[k] + self.im[k] * self.im[k];
            let nk = self.noise[k];
            let g = if nk <= 1e-18 { 1.0 } else {
                let gamma = (p / nk).min(1e6);
                let xi = 0.95 * d.prev[k] + 0.05 * (gamma - 1.0).max(0.0);
                let g = (xi / (1.0 + xi)).max(self.gmin);
                d.prev[k] = (g * g * gamma).min(1e5);
                g
            };
            self.re[k] *= g; self.im[k] *= g;
            if k > 0 && k < N / 2 { self.re[N - k] *= g; self.im[N - k] *= g; }
        }
        self.fft.run(&mut self.re, &mut self.im, true);
        for n in 0..N { d.ola[n] += self.re[n] * self.win[n] * 0.5; }
        d.ready.copy_from_slice(&d.ola[..HOP]);
        d.ola.copy_within(HOP.., 0);
        d.ola[N - HOP..].iter_mut().for_each(|v| *v = 0.0);
        d.inp.copy_within(HOP.., 0);
        d.fill = N - HOP;
        d.rd = 0;
    }
}

fn coef(rate: f64, seconds: f64) -> f32 { (1.0 - (-1.0 / (rate * seconds)).exp()) as f32 }

/// Begrenzer mit kurzer Vorschau: senkt Spitzen ab, bevor sie ankommen, statt sie abzuschneiden.
struct Limiter { la: usize, delay: Vec<f32>, gains: Vec<f32>, pos: usize, g: f32, ceil: f32, att: f32, rel: f32, ch: usize }

impl Limiter {
    fn new(rate: f64, ch: usize) -> Limiter {
        let la = ((rate * 0.002).round() as usize).max(8);
        Limiter { la, delay: vec![0.0; la * ch], gains: vec![1.0; la], pos: 0, g: 1.0, ceil: lin(CEIL_DB), att: coef(rate, 0.0004), rel: coef(rate, 0.08), ch }
    }
    fn run(&mut self, frame: &mut [f32]) {
        let peak = frame.iter().fold(0f32, |m, v| m.max(v.abs()));
        self.gains[self.pos] = if peak > self.ceil { self.ceil / peak } else { 1.0 };
        let want = self.gains.iter().fold(1f32, |m, g| m.min(*g));
        self.g += (want - self.g) * if want < self.g { self.att } else { self.rel };
        let at = self.pos * self.ch;
        for c in 0..self.ch {
            let old = self.delay[at + c];
            self.delay[at + c] = frame[c];
            frame[c] = (old * self.g).clamp(-self.ceil, self.ceil);
        }
        self.pos = (self.pos + 1) % self.la;
    }
}

/// Die ganze Kette für einen Tonstrom. Arbeitet auf verschachtelten Samples (Kanal für Kanal je Zeitpunkt),
/// gibt je Sample genau eines zurück und verzögert dabei um `latency()` Zeitpunkte.
pub struct Chain {
    ch: usize,
    hp: Vec<[Biquad; 2]>,
    den: Option<Denoise>,
    eq: Vec<Vec<Biquad>>,
    gate: Option<(f32, f32, f32, f32)>, // Schwelle, tiefste Absenkung, Hüllkurve, Verstärkung
    comp: Option<(f32, f32, f32)>,     // Schwelle dB, Verhältnis, Ausgleich
    env: f32,
    gain: f32,
    lim: Option<Limiter>,
    a_fast: f32, a_slow: f32, a_open: f32, a_close: f32,
}

impl Chain {
    pub fn new(p: &Params, rate: u32, channels: usize) -> Chain {
        let r = rate.max(8000) as f64;
        let ch = channels.max(1);
        let hp = if p.lowcut > 1.0 { (0..ch).map(|_| [Biquad::highpass(r, p.lowcut as f64, 0.541), Biquad::highpass(r, p.lowcut as f64, 1.307)]).collect() } else { Vec::new() };
        let den = if p.denoise > 0.001 { profile(&p.profile).map(|pr| Denoise::new(&pr, p.denoise, r, ch)) } else { None };
        let mut bands: Vec<Biquad> = Vec::new();
        if p.voice {
            bands.push(Biquad::shelf(r, 170.0, -2.0, false));
            bands.push(Biquad::peak(r, 3200.0, 0.9, 3.5));
            bands.push(Biquad::shelf(r, 9000.0, 2.0, true));
        }
        if p.bass.abs() > 0.05 { bands.push(Biquad::shelf(r, 220.0, p.bass as f64, false)); }
        if p.treble.abs() > 0.05 { bands.push(Biquad::shelf(r, 5000.0, p.treble as f64, true)); }
        let eq = if bands.is_empty() { Vec::new() } else { (0..ch).map(|_| bands.clone()).collect() };
        let gain = lin(p.gain);
        // Pausen: alles unterhalb „Grundrauschen plus etwas Luft“ wird abgesenkt
        let gate = if p.gate > 0.001 {
            let thr = lin((p.floor + 9.0).clamp(-70.0, -30.0));
            Some((thr, lin(-(6.0 + 20.0 * p.gate)), 0.0, 1.0))
        } else { None };
        let comp = if p.compress > 0.001 {
            let level = (p.loud + p.gain).clamp(-50.0, -6.0);
            Some((level + 2.0, 1.5 + 2.5 * p.compress, lin(4.0 * p.compress)))
        } else { None };
        Chain { ch, hp, den, eq, gate, comp, env: 0.0, gain, lim: if p.limit { Some(Limiter::new(r, ch)) } else { None },
            a_fast: coef(r, 0.003), a_slow: coef(r, 0.12), a_open: coef(r, 0.004), a_close: coef(r, 0.18) }
    }

    pub fn latency(&self) -> usize { self.den.as_ref().map(|_| N).unwrap_or(0) + self.lim.as_ref().map(|l| l.la).unwrap_or(0) }

    pub fn process(&mut self, data: &mut [f32]) {
        let ch = self.ch;
        for frame in data.chunks_exact_mut(ch) {
            for c in 0..ch {
                let mut x = frame[c];
                if let Some(h) = self.hp.get_mut(c) { x = h[0].run(x); x = h[1].run(x); }
                if let Some(d) = self.den.as_mut() { x = d.run(c, x); }
                if let Some(e) = self.eq.get_mut(c) { for b in e.iter_mut() { x = b.run(x); } }
                frame[c] = x;
            }
            if let Some((thr, low, env, g)) = self.gate.as_mut() {
                let peak = frame.iter().fold(0f32, |m, v| m.max(v.abs()));
                *env += (peak - *env) * if peak > *env { self.a_fast } else { self.a_slow };
                let want = if *env >= *thr { 1.0 } else { (*env / *thr).max(*low) };
                *g += (want - *g) * if want > *g { self.a_open } else { self.a_close };
                for v in frame.iter_mut() { *v *= *g; }
            }
            if self.gain != 1.0 { for v in frame.iter_mut() { *v *= self.gain; } }
            if let Some((thr, ratio, makeup)) = self.comp {
                let peak = frame.iter().fold(0f32, |m, v| m.max(v.abs()));
                self.env += (peak - self.env) * if peak > self.env { self.a_fast } else { self.a_slow };
                let level = 20.0 * self.env.max(1e-9).log10();
                let over = level - thr;
                // weiches Knie, 6 dB breit
                let cut = if over <= -3.0 { 0.0 } else if over >= 3.0 { over * (1.0 - 1.0 / ratio) } else { (over + 3.0) * (over + 3.0) / 12.0 * (1.0 - 1.0 / ratio) };
                let g = lin(-cut) * makeup;
                for v in frame.iter_mut() { *v *= g; }
            }
            if let Some(l) = self.lim.as_mut() { l.run(frame); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17;
            ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        }
    }
    fn rms(x: &[f32]) -> f32 { 10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).max(1e-20).log10() }

    /// Testton: Rauschen durchgehend, „Sprache“ (mehrere Töne) in jeder zweiten Sekunde
    fn signal(seconds: usize, noise_amp: f32) -> (Vec<f32>, Vec<f32>) {
        let mut rng = Rng(0x1234_5678_9abc_def1);
        let n = seconds * 48000;
        let mut clean = vec![0f32; n];
        let mut mix = vec![0f32; n];
        for i in 0..n {
            let t = i as f64 / 48000.0;
            let on = (t as usize) % 2 == 1;
            let fade = if on { let f = t.fract(); (f * 40.0).min(1.0).min((1.0 - f) * 40.0) } else { 0.0 };
            let s = (0.12 * (2.0 * std::f64::consts::PI * 220.0 * t).sin() + 0.08 * (2.0 * std::f64::consts::PI * 660.0 * t).sin() + 0.05 * (2.0 * std::f64::consts::PI * 2300.0 * t).sin()) * fade;
            clean[i] = s as f32;
            mix[i] = s as f32 + rng.next() * noise_amp;
        }
        (clean, mix)
    }

    #[test]
    fn fft_roundtrip() {
        let f = Fft::new(64);
        let mut re: Vec<f32> = (0..64).map(|i| ((i * 7) % 13) as f32 - 6.0).collect();
        let orig = re.clone();
        let mut im = vec![0f32; 64];
        f.run(&mut re, &mut im, false);
        f.run(&mut re, &mut im, true);
        for i in 0..64 { assert!((re[i] - orig[i]).abs() < 1e-4); }
    }

    #[test]
    fn denoise_removes_noise_and_keeps_signal() {
        let (clean, mix) = signal(12, 0.01);
        let mut a = Analyzer::new();
        a.push(&mix);
        let prof = a.finish().unwrap();
        assert!(prof.steady, "Grundrauschen sollte als gleichmäßig erkannt werden");
        assert!((prof.floor - (-44.8)).abs() < 2.0, "Grundrauschen {}", prof.floor);
        set_profile("test", &prof);
        let p = Params { denoise: 0.6, limit: false, profile: "test".into(), ..Default::default() };
        let mut c = Chain::new(&p, 48000, 1);
        assert_eq!(c.latency(), N);
        let mut out = mix.clone();
        out.extend(std::iter::repeat(0.0).take(N));
        c.process(&mut out);
        let out = &out[N..];
        // Pause (Sekunde 4): nur Rauschen
        let before = rms(&mix[4 * 48000 + 4800..5 * 48000 - 4800]);
        let after = rms(&out[4 * 48000 + 4800..5 * 48000 - 4800]);
        assert!(before - after > 15.0, "Rauschen nur um {} dB gesenkt", before - after);
        // Signal (Sekunde 5): bleibt erhalten und passt zeitlich genau
        let seg = 5 * 48000 + 4800..6 * 48000 - 4800;
        let lvl = rms(&out[seg.clone()]) - rms(&clean[seg.clone()]);
        assert!(lvl.abs() < 1.0, "Pegel des Nutzsignals um {lvl} dB verändert");
        let err: Vec<f32> = out[seg.clone()].iter().zip(&clean[seg.clone()]).map(|(a, b)| a - b).collect();
        assert!(rms(&err) < rms(&clean[seg]) - 20.0, "Nutzsignal verfälscht oder verschoben");
    }

    #[test]
    fn chain_keeps_alignment_and_limits() {
        let p = Params { gain: 30.0, lowcut: 80.0, voice: true, compress: 0.5, ..Default::default() };
        let mut c = Chain::new(&p, 48000, 2);
        let la = c.latency();
        assert!(la > 0 && la < 200);
        let mut data: Vec<f32> = (0..48000 * 2).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * (i / 2) as f32 / 48000.0).sin()).collect();
        c.process(&mut data);
        assert!(data.iter().all(|v| v.abs() <= lin(CEIL_DB) + 1e-6));
        assert!(rms(&data[48000..]) > -8.0);
        let q = Params::parse(&p.to_text());
        assert_eq!(p, q);
    }
}
