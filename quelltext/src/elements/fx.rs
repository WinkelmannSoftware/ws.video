//! `wsfx`: Farbkorrektur, Spiegeln, Vignette und alle Übergänge, die im Bild selbst gerechnet werden
//! (über Schwarz/Weiß, Zoom, Schwenk, Unschärfe, Störung). Vorschau und Export laufen durch dasselbe Element.
//!
//! Die Beschreibung steht in der Eigenschaft `fx` als Zahlenliste:
//! "art_ein,start_ein,dauer_ein,art_aus,start_aus,dauer_aus,helligkeit,kontrast,sättigung,temperatur,spiegeln,vignette"
//! (Zeiten in Sekunden der Quelle; Arten siehe `K_*`; spiegeln: Bit 0 waagerecht, Bit 1 senkrecht).
use gst::glib;
use gst::subclass::prelude::*;
use gst_video::prelude::*;
use gst_video::subclass::prelude::*;
use std::sync::{LazyLock, Mutex};

pub const K_NONE: u32 = 0;
pub const K_BLACK: u32 = 1;
pub const K_WHITE: u32 = 2;
pub const K_ZOOM: u32 = 3;
pub const K_WHIP_L: u32 = 4;
pub const K_WHIP_R: u32 = 5;
pub const K_WHIP_U: u32 = 6;
pub const K_WHIP_D: u32 = 7;
pub const K_BLUR: u32 = 8;
pub const K_GLITCH: u32 = 9;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Fx {
    pub on: bool,
    pub in_kind: u32,
    pub in_start: f64,
    pub in_len: f64,
    pub out_kind: u32,
    pub out_start: f64,
    pub out_len: f64,
    pub bright: f64,
    pub contrast: f64,
    pub sat: f64,
    pub temp: f64,
    pub flip: u32,
    pub vignette: f64,
}

pub fn parse(s: &str) -> Fx {
    let v: Vec<f64> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if v.len() < 12 { return Fx::default(); }
    Fx {
        on: true, in_kind: v[0] as u32, in_start: v[1], in_len: v[2], out_kind: v[3] as u32, out_start: v[4], out_len: v[5],
        bright: v[6], contrast: v[7], sat: v[8], temp: v[9], flip: v[10] as u32, vignette: v[11],
    }
}

fn threads() -> usize { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4) }

/// Zeilenblöcke eines Bildes auf mehrere Threads verteilen. `f(erste_zeile, block)`
fn par_rows(data: &mut [u8], stride: usize, h: usize, f: impl Fn(usize, &mut [u8]) + Sync) {
    let n = threads();
    if n <= 1 || h < 64 { f(0, data); return; }
    let per = h.div_ceil(n);
    std::thread::scope(|s| {
        for (i, chunk) in data.chunks_mut(stride * per).enumerate() {
            let f = &f;
            s.spawn(move || f(i * per, chunk));
        }
    });
}

/// Index spiegelnd in 0..n falten (das Bild setzt sich an den Rändern gespiegelt fort)
#[inline]
fn mirror(i: i64, n: i64) -> usize {
    let m = i.rem_euclid(2 * n);
    (if m >= n { 2 * n - 1 - m } else { m }) as usize
}

fn smooth(k: f64) -> f64 { let k = k.clamp(0.0, 1.0); k * k * (3.0 - 2.0 * k) }

/// Helligkeit, Kontrast, Temperatur über Tabellen; Sättigung je Bildpunkt (BGRA).
fn color(fx: &Fx, data: &mut [u8], stride: usize, w: usize, h: usize) {
    let c = fx.contrast.clamp(-1.0, 1.0);
    let gain = if c >= 0.0 { 1.0 + 1.5 * c } else { 1.0 + c * 0.9 };
    let b = fx.bright.clamp(-1.0, 1.0) * 0.45;
    let t = fx.temp.clamp(-1.0, 1.0) * 0.22;
    let lut = |chan_gain: f64| -> [u8; 256] {
        let mut l = [0u8; 256];
        for (i, o) in l.iter_mut().enumerate() {
            let x = i as f64 / 255.0;
            let y = ((x - 0.5) * gain + 0.5 + b) * chan_gain;
            *o = (y.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        l
    };
    let (lb, lg, lr) = (lut(1.0 - t), lut(1.0), lut(1.0 + t));
    let sat = ((1.0 + fx.sat.clamp(-1.0, 1.0)) * 256.0).round() as i32;
    par_rows(data, stride, h, |y0, chunk| {
        let rows = (h - y0).min(chunk.len().div_ceil(stride));
        for y in 0..rows {
            let Some(row) = chunk.get_mut(y * stride..y * stride + w * 4) else { break };
            for px in row.chunks_exact_mut(4) {
                let (mut b, mut g, mut r) = (lb[px[0] as usize] as i32, lg[px[1] as usize] as i32, lr[px[2] as usize] as i32);
                if sat != 256 {
                    let l = (29 * b + 150 * g + 77 * r) >> 8;
                    b = (l + (((b - l) * sat) >> 8)).clamp(0, 255);
                    g = (l + (((g - l) * sat) >> 8)).clamp(0, 255);
                    r = (l + (((r - l) * sat) >> 8)).clamp(0, 255);
                }
                px[0] = b as u8; px[1] = g as u8; px[2] = r as u8;
            }
        }
    });
}

fn vignette(amount: f64, data: &mut [u8], stride: usize, w: usize, h: usize) {
    let a = amount.clamp(0.0, 1.0) as f32;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let xs: Vec<f32> = (0..w).map(|x| { let d = (x as f32 + 0.5 - cx) / cx; d * d }).collect();
    par_rows(data, stride, h, |y0, chunk| {
        let rows = (h - y0).min(chunk.len().div_ceil(stride));
        for y in 0..rows {
            let Some(row) = chunk.get_mut(y * stride..y * stride + w * 4) else { break };
            let dy = ((y0 + y) as f32 + 0.5 - cy) / cy;
            let dy2 = dy * dy;
            for (px, dx2) in row.chunks_exact_mut(4).zip(xs.iter()) {
                // 0 in der Mitte, 1 in den Ecken
                let d = ((dx2 + dy2) * 0.5).sqrt();
                let k = ((d - 0.45) / 0.55).clamp(0.0, 1.0);
                let f = 1.0 - a * k * k * (3.0 - 2.0 * k);
                let f = (f * 256.0) as u32;
                px[0] = ((px[0] as u32 * f) >> 8) as u8;
                px[1] = ((px[1] as u32 * f) >> 8) as u8;
                px[2] = ((px[2] as u32 * f) >> 8) as u8;
            }
        }
    });
}

fn flip(bits: u32, data: &mut [u8], stride: usize, w: usize, h: usize) {
    if bits & 1 != 0 {
        for y in 0..h {
            let Some(row) = data.get_mut(y * stride..y * stride + w * 4) else { break };
            for x in 0..w / 2 {
                let (a, b) = (x * 4, (w - 1 - x) * 4);
                for i in 0..4 { row.swap(a + i, b + i); }
            }
        }
    }
    if bits & 2 != 0 {
        for y in 0..h / 2 {
            let (top, bottom) = data.split_at_mut((h - 1 - y) * stride);
            let n = (w * 4).min(bottom.len());
            top[y * stride..y * stride + n].swap_with_slice(&mut bottom[..n]);
        }
    }
}

/// Zur Farbe hin einfärben (0 = unverändert, 1 = ganz die Farbe); die Deckkraft bleibt.
fn tint(k: f64, white: bool, data: &mut [u8], stride: usize, w: usize, h: usize) {
    let a = (smooth(k) * 256.0).round() as u32;
    let add = if white { 255 * a } else { 0 };
    for y in 0..h {
        let Some(row) = data.get_mut(y * stride..y * stride + w * 4) else { break };
        for px in row.chunks_exact_mut(4) {
            for c in px.iter_mut().take(3) { *c = ((*c as u32 * (256 - a) + add) >> 8) as u8; }
        }
    }
}

/// Bild entlang einer Achse verschieben (gespiegelt fortgesetzt) und über `2r+1` Punkte mitteln.
/// Mit `shift = 0` ist das ein Weichzeichner, mit Verschiebung die Bewegungsunschärfe des Schwenks.
fn streak(data: &mut [u8], stride: usize, w: usize, h: usize, vertical: bool, shift: i64, r: usize) {
    if shift == 0 && r == 0 { return; }
    let n = (2 * r + 1) as u32;
    if !vertical {
        par_rows(data, stride, h, |y0, chunk| {
            let rows = (h - y0).min(chunk.len().div_ceil(stride));
            let mut src = vec![0u8; w * 4];
            for y in 0..rows {
                let Some(row) = chunk.get_mut(y * stride..y * stride + w * 4) else { break };
                src.copy_from_slice(row);
                let at = |j: i64| mirror(j - r as i64 - shift, w as i64) * 4;
                let mut sum = [0u32; 4];
                for j in 0..n as i64 { let i = at(j); for c in 0..4 { sum[c] += src[i + c] as u32; } }
                for x in 0..w {
                    for c in 0..4 { row[x * 4 + c] = (sum[c] / n) as u8; }
                    let (add, sub) = (at(x as i64 + n as i64), at(x as i64));
                    for c in 0..4 { sum[c] = sum[c] + src[add + c] as u32 - src[sub + c] as u32; }
                }
            }
        });
    } else {
        let mut src = vec![0u8; h * 4];
        for x in 0..w {
            for y in 0..h { let i = y * stride + x * 4; if i + 4 <= data.len() { src[y * 4..y * 4 + 4].copy_from_slice(&data[i..i + 4]); } }
            let at = |j: i64| mirror(j - r as i64 - shift, h as i64) * 4;
            let mut sum = [0u32; 4];
            for j in 0..n as i64 { let i = at(j); for c in 0..4 { sum[c] += src[i + c] as u32; } }
            for y in 0..h {
                let o = y * stride + x * 4;
                if o + 4 <= data.len() { for c in 0..4 { data[o + c] = (sum[c] / n) as u8; } }
                let (add, sub) = (at(y as i64 + n as i64), at(y as i64));
                for c in 0..4 { sum[c] = sum[c] + src[add + c] as u32 - src[sub + c] as u32; }
            }
        }
    }
}

/// Um die Bildmitte vergrößern (scale > 1) oder verkleinern (scale < 1, Ränder gespiegelt gefüllt).
fn zoom(scale: f64, data: &mut [u8], stride: usize, w: usize, h: usize) {
    if (scale - 1.0).abs() < 1e-4 { return; }
    let src = data.to_vec();
    let inv = (65536.0 / scale) as i64;
    let (cx, cy) = ((w as i64) << 15, (h as i64) << 15);
    par_rows(data, stride, h, |y0, chunk| {
        let rows = (h - y0).min(chunk.len().div_ceil(stride));
        for y in 0..rows {
            let Some(row) = chunk.get_mut(y * stride..y * stride + w * 4) else { break };
            // Festkomma 16.16, Mitte des Bildpunkts
            let sy = cy + ((((y0 + y) as i64) << 16) + 32768 - cy) * inv / 65536 - 32768;
            let (yi, fy) = (sy >> 16, (sy & 0xffff) as u32);
            let (ya, yb) = (mirror(yi, h as i64) * stride, mirror(yi + 1, h as i64) * stride);
            for x in 0..w {
                let sx = cx + (((x as i64) << 16) + 32768 - cx) * inv / 65536 - 32768;
                let (xi, fx) = (sx >> 16, (sx & 0xffff) as u32);
                let (xa, xb) = (mirror(xi, w as i64) * 4, mirror(xi + 1, w as i64) * 4);
                for c in 0..4 {
                    let (p00, p01, p10, p11) = (src[ya + xa + c] as u32, src[ya + xb + c] as u32, src[yb + xa + c] as u32, src[yb + xb + c] as u32);
                    let top = p00 * (65536 - fx) + p01 * fx;
                    let bot = p10 * (65536 - fx) + p11 * fx;
                    row[x * 4 + c] = (((top >> 8) * (65536 - fy) + (bot >> 8) * fy) >> 24) as u8;
                }
            }
        }
    });
}

/// Digitale Störung: Farbkanäle gegeneinander versetzt, dazu einzelne waagerecht verschobene Streifen.
fn glitch(k: f64, t: f64, data: &mut [u8], stride: usize, w: usize, h: usize) {
    let k = k.clamp(0.0, 1.0);
    if k < 0.01 { return; }
    let src = data.to_vec();
    // einfache Zufallsfolge, 12-mal je Sekunde neu
    let mut seed = ((t * 12.0) as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03;
    let mut rnd = || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed >> 11) as f64 / (1u64 << 53) as f64 };
    let d = (k * w as f64 * 0.012 * (0.5 + rnd())).round() as i64;
    let mut bands: Vec<(usize, usize, i64)> = Vec::new();
    for _ in 0..(2.0 + k * 6.0) as usize {
        let y = (rnd() * h as f64) as usize;
        let hh = ((0.02 + rnd() * 0.10) * h as f64) as usize;
        let off = ((rnd() - 0.5) * 2.0 * k * w as f64 * 0.10) as i64;
        bands.push((y, (y + hh).min(h), off));
    }
    for y in 0..h {
        let off = bands.iter().filter(|b| y >= b.0 && y < b.1).map(|b| b.2).sum::<i64>();
        let Some(row) = data.get_mut(y * stride..y * stride + w * 4) else { break };
        let s = &src[y * stride..];
        for x in 0..w {
            let xi = x as i64 + off;
            let (ib, ig, ir) = (mirror(xi - d, w as i64) * 4, mirror(xi, w as i64) * 4, mirror(xi + d, w as i64) * 4);
            row[x * 4] = s[ib];
            row[x * 4 + 1] = s[ig + 1];
            row[x * 4 + 2] = s[ir + 2];
            row[x * 4 + 3] = s[ig + 3];
        }
    }
}

/// Ein Übergang. `k`: 0 = nichts zu sehen, 1 = am Schnitt. `enter`: Anfang des Clips (sonst Ende).
fn transition(kind: u32, k: f64, enter: bool, t: f64, data: &mut [u8], stride: usize, w: usize, h: usize) {
    let k = k.clamp(0.0, 1.0);
    if k <= 0.0 { return; }
    match kind {
        K_BLACK => tint(k, false, data, stride, w, h),
        K_WHITE => tint(k, true, data, stride, w, h),
        K_ZOOM => {
            // hinaus: beschleunigt hinein in das Bild; herein: aus der Ferne ankommen und abbremsen
            let scale = if enter { 1.0 / (1.0 + 1.2 * k * k) } else { 1.0 + 1.6 * k * k };
            zoom(scale, data, stride, w, h);
            let r = (k * k * 0.012 * w as f64) as usize;
            if r > 0 { streak(data, stride, w, h, false, 0, r); streak(data, stride, w, h, true, 0, r); }
        }
        K_WHIP_L | K_WHIP_R | K_WHIP_U | K_WHIP_D => {
            let vertical = kind == K_WHIP_U || kind == K_WHIP_D;
            let dir: f64 = if kind == K_WHIP_L || kind == K_WHIP_U { -1.0 } else { 1.0 };
            let len = if vertical { h } else { w } as f64;
            // Das Bild bewegt sich in Richtung `dir`: am Ende fährt es beschleunigt hinaus, am Anfang kommt es
            // von der Gegenseite und bremst ab.
            let dist = k * k * len;
            let shift = if enter { -dir * dist } else { dir * dist };
            let r = (k * len * 0.10) as usize;
            streak(data, stride, w, h, vertical, shift.round() as i64, r);
        }
        K_BLUR => {
            let r = (smooth(k) * 0.03 * w as f64).round() as usize;
            for _ in 0..2 { streak(data, stride, w, h, false, 0, r); streak(data, stride, w, h, true, 0, r); }
        }
        K_GLITCH => glitch(k, t, data, stride, w, h),
        _ => {}
    }
}

/// Alles anwenden, was zur Quellzeit `t` gilt.
pub fn apply(fx: &Fx, t: f64, data: &mut [u8], stride: usize, w: usize, h: usize) {
    if !fx.on || w < 2 || h < 2 || data.len() < (h - 1) * stride + w * 4 { return; }
    if fx.flip != 0 { flip(fx.flip, data, stride, w, h); }
    if fx.bright != 0.0 || fx.contrast != 0.0 || fx.sat != 0.0 || fx.temp != 0.0 { color(fx, data, stride, w, h); }
    if fx.vignette > 0.001 { vignette(fx.vignette, data, stride, w, h); }
    if fx.in_kind != K_NONE && fx.in_len > 0.0 && t < fx.in_start + fx.in_len {
        transition(fx.in_kind, 1.0 - (t - fx.in_start) / fx.in_len, true, t, data, stride, w, h);
    }
    if fx.out_kind != K_NONE && fx.out_len > 0.0 && t > fx.out_start {
        transition(fx.out_kind, (t - fx.out_start) / fx.out_len, false, t, data, stride, w, h);
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WsFx {
        pub fx: Mutex<(String, Fx)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WsFx {
        const NAME: &'static str = "WsFx";
        type Type = super::WsFx;
        type ParentType = gst_video::VideoFilter;
    }

    impl ObjectImpl for WsFx {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| vec![glib::ParamSpecString::builder("fx").build()]);
            PROPS.as_ref()
        }
        fn set_property(&self, _id: usize, value: &glib::Value, _pspec: &glib::ParamSpec) {
            let s = value.get::<Option<String>>().ok().flatten().unwrap_or_default();
            let f = parse(&s);
            *self.fx.lock().unwrap() = (s, f);
        }
        fn property(&self, _id: usize, _pspec: &glib::ParamSpec) -> glib::Value {
            self.fx.lock().unwrap().0.to_value()
        }
    }
    impl GstObjectImpl for WsFx {}
    impl ElementImpl for WsFx {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new("WS.Video Effekte", "Filter/Effect/Video", "Farbkorrektur und Übergänge im Bild", "WS.Video")
            });
            Some(&*META)
        }
        fn pad_templates() -> &'static [gst::PadTemplate] {
            static T: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
                let caps = gst_video::VideoCapsBuilder::new().format(gst_video::VideoFormat::Bgra).build();
                vec![
                    gst::PadTemplate::new("src", gst::PadDirection::Src, gst::PadPresence::Always, &caps).unwrap(),
                    gst::PadTemplate::new("sink", gst::PadDirection::Sink, gst::PadPresence::Always, &caps).unwrap(),
                ]
            });
            T.as_ref()
        }
    }
    impl BaseTransformImpl for WsFx {
        const MODE: gst_base::subclass::BaseTransformMode = gst_base::subclass::BaseTransformMode::AlwaysInPlace;
        const PASSTHROUGH_ON_SAME_CAPS: bool = false;
        const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;
    }
    impl VideoFilterImpl for WsFx {
        fn transform_frame_ip(&self, frame: &mut gst_video::VideoFrameRef<&mut gst::BufferRef>) -> Result<gst::FlowSuccess, gst::FlowError> {
            let fx = self.fx.lock().unwrap().1;
            if !fx.on { return Ok(gst::FlowSuccess::Ok); }
            let t = frame.buffer().pts().map(|p| p.nseconds() as f64 / 1e9).unwrap_or(0.0);
            let (w, h) = (frame.width() as usize, frame.height() as usize);
            let stride = frame.plane_stride()[0] as usize;
            let data = frame.plane_data_mut(0).map_err(|_| gst::FlowError::Error)?;
            apply(&fx, t, data, stride, w, h);
            Ok(gst::FlowSuccess::Ok)
        }
    }
}

glib::wrapper! {
    pub struct WsFx(ObjectSubclass<imp::WsFx>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register() -> Result<(), glib::BoolError> {
    gst::Element::register(None, "wsfx", gst::Rank::NONE, WsFx::static_type())
}
