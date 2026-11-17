//! Mauszeiger-Daten einer Aufnahme (Positionen und Klicks als eigene Datenspur).
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Monitor {
    pub name: String,
    /// Position und Größe im Layout des Compositors (logische Pixel)
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Größe in echten Pixeln
    pub pw: u32,
    pub ph: u32,
    pub scale: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct CursorData {
    pub version: u32,
    pub monitors: Vec<Monitor>,
    /// Vermuteter aufgenommener Monitor (-1 = unbekannt)
    pub monitor: i32,
    pub stream_w: u32,
    pub stream_h: u32,
    /// [Zeit, x, y] in Sekunden bzw. Layout-Koordinaten
    pub samples: Vec<[f64; 3]>,
    /// [Zeit, Taste, x, y]
    pub clicks: Vec<[f64; 4]>,
}

impl CursorData {
    pub fn load(path: &str) -> Option<CursorData> {
        let s = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&s).ok()
    }
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_string(self)?)?;
        Ok(())
    }
    /// Monitor bestimmen: ausdrücklich gewählt, in der Datei vermerkt, oder passend zur Bildgröße.
    pub fn pick_monitor(&self, wanted: i32, frame_w: u32, frame_h: u32) -> Option<&Monitor> {
        if wanted >= 0 { if let Some(m) = self.monitors.get(wanted as usize) { return Some(m); } }
        if self.monitor >= 0 { if let Some(m) = self.monitors.get(self.monitor as usize) { return Some(m); } }
        guess_monitor(&self.monitors, frame_w, frame_h).and_then(|i| self.monitors.get(i))
    }
    /// Zeigerposition (Layout-Koordinaten) zur Zeit t, linear interpoliert.
    pub fn pos_at(&self, t: f64) -> Option<(f64, f64)> {
        let s = &self.samples;
        if s.is_empty() { return None; }
        let i = s.partition_point(|p| p[0] <= t);
        if i == 0 { return Some((s[0][1], s[0][2])); }
        if i >= s.len() { let l = s[s.len() - 1]; return Some((l[1], l[2])); }
        let (a, b) = (s[i - 1], s[i]);
        let k = if b[0] > a[0] { (t - a[0]) / (b[0] - a[0]) } else { 0.0 };
        Some((a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k))
    }
    /// Geglättete Position: Mittel über ein Zeitfenster.
    pub fn smooth_pos_at(&self, t: f64, window: f64) -> Option<(f64, f64)> {
        if window <= 0.001 { return self.pos_at(t); }
        let n = 15;
        let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
        for i in 0..n {
            let k = i as f64 / (n - 1) as f64;
            let tt = t - window / 2.0 + window * k;
            // Dreiecksgewicht: Mitte zählt am meisten
            let wgt = 1.0 - (2.0 * k - 1.0).abs() + 0.15;
            if let Some((x, y)) = self.pos_at(tt) {
                sx += x * wgt;
                sy += y * wgt;
                sw += wgt;
            }
        }
        if sw > 0.0 { Some((sx / sw, sy / sw)) } else { None }
    }
}

/// Monitor anhand der Bildgröße erraten (eindeutige Übereinstimmung der Pixelgröße).
pub fn guess_monitor(monitors: &[Monitor], w: u32, h: u32) -> Option<usize> {
    let exact: Vec<usize> = monitors.iter().enumerate().filter(|(_, m)| m.pw == w && m.ph == h).map(|(i, _)| i).collect();
    if exact.len() == 1 { return Some(exact[0]); }
    if monitors.len() == 1 { return Some(0); }
    if !exact.is_empty() { return Some(exact[0]); }
    None
}
