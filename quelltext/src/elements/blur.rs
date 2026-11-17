//! `wsblur`: macht rechteckige Bereiche eines Videobilds unkenntlich (Weichzeichner).
//! Bereiche als Text: "x,y,w,h,t0,t1,stärke;…" (Anteile 0..1 der Bildgröße, Zeiten in Quellsekunden).
use gst::glib;
use gst::subclass::prelude::*;
use gst_video::prelude::*;
use gst_video::subclass::prelude::*;
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Debug, PartialEq)]
struct Region { x: f64, y: f64, w: f64, h: f64, t0: f64, t1: f64, strength: f64, pixelate: bool, size: f64 }

fn parse(s: &str) -> Vec<Region> {
    s.split(';').filter_map(|part| {
        let v: Vec<f64> = part.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if v.len() >= 7 { Some(Region { x: v[0], y: v[1], w: v[2], h: v[3], t0: v[4], t1: v[5], strength: v[6], pixelate: v.get(7).map(|p| *p > 0.5).unwrap_or(false), size: v.get(8).copied().unwrap_or(1.0).clamp(0.2, 30.0) }) } else { None }
    }).collect()
}

/// Verpixeln: Der Ausschnitt wird in Kacheln der Kantenlänge `bs` zerlegt, jede bekommt ihre mittlere Farbe.
fn pixel_rect(data: &mut [u8], stride: usize, x0: usize, y0: usize, w: usize, h: usize, bs: usize) {
    let bs = bs.max(2);
    let mut by = 0;
    while by < h {
        let bh = bs.min(h - by);
        let mut bx = 0;
        while bx < w {
            let bw = bs.min(w - bx);
            let mut sum = [0u32; 3];
            for y in 0..bh {
                let row = (y0 + by + y) * stride + (x0 + bx) * 4;
                for x in 0..bw { for c in 0..3 { sum[c] += data[row + x * 4 + c] as u32; } }
            }
            let n = (bw * bh) as u32;
            let avg = [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8];
            for y in 0..bh {
                let row = (y0 + by + y) * stride + (x0 + bx) * 4;
                for x in 0..bw { data[row + x * 4..row + x * 4 + 3].copy_from_slice(&avg); }
            }
            bx += bs;
        }
        by += bs;
    }
}

/// Box-Weichzeichner (zwei Durchgänge) auf einem Ausschnitt eines 4-Kanal-Bilds.
fn blur_rect(data: &mut [u8], stride: usize, x0: usize, y0: usize, w: usize, h: usize, r: usize) {
    if w < 2 || h < 2 || r == 0 { return; }
    let mut buf = vec![0u8; w * h * 3];
    let mut tmp = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y0 + y) * stride + (x0 + x) * 4;
            let o = (y * w + x) * 3;
            buf[o..o + 3].copy_from_slice(&data[i..i + 3]);
        }
    }
    for _ in 0..2 {
        // horizontal
        for y in 0..h {
            for ch in 0..3 {
                let mut sum: u32 = 0;
                let mut n: u32 = 0;
                for i in 0..=r.min(w - 1) { sum += buf[(y * w + i) * 3 + ch] as u32; n += 1; }
                for x in 0..w {
                    tmp[(y * w + x) * 3 + ch] = (sum / n) as u8;
                    if x + r + 1 < w { sum += buf[(y * w + x + r + 1) * 3 + ch] as u32; n += 1; }
                    if x >= r { sum -= buf[(y * w + x - r) * 3 + ch] as u32; n -= 1; }
                }
            }
        }
        // vertikal
        for x in 0..w {
            for ch in 0..3 {
                let mut sum: u32 = 0;
                let mut n: u32 = 0;
                for i in 0..=r.min(h - 1) { sum += tmp[(i * w + x) * 3 + ch] as u32; n += 1; }
                for y in 0..h {
                    buf[(y * w + x) * 3 + ch] = (sum / n) as u8;
                    if y + r + 1 < h { sum += tmp[((y + r + 1) * w + x) * 3 + ch] as u32; n += 1; }
                    if y >= r { sum -= tmp[((y - r) * w + x) * 3 + ch] as u32; n -= 1; }
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let i = (y0 + y) * stride + (x0 + x) * 4;
            let o = (y * w + x) * 3;
            data[i..i + 3].copy_from_slice(&buf[o..o + 3]);
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WsBlur {
        regions: Mutex<(String, Vec<Region>)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WsBlur {
        const NAME: &'static str = "WsBlur";
        type Type = super::WsBlur;
        type ParentType = gst_video::VideoFilter;
    }
    impl ObjectImpl for WsBlur {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| vec![glib::ParamSpecString::builder("regions").build()]);
            PROPS.as_ref()
        }
        fn set_property(&self, _id: usize, value: &glib::Value, _pspec: &glib::ParamSpec) {
            let s: Option<String> = value.get().unwrap();
            let s = s.unwrap_or_default();
            let parsed = parse(&s);
            *self.regions.lock().unwrap() = (s, parsed);
        }
        fn property(&self, _id: usize, _pspec: &glib::ParamSpec) -> glib::Value {
            self.regions.lock().unwrap().0.to_value()
        }
    }
    impl GstObjectImpl for WsBlur {}
    impl ElementImpl for WsBlur {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new("WS.Video Weichzeichner", "Filter/Effect/Video", "Bereiche unkenntlich machen", "WS.Video")
            });
            Some(&*META)
        }
        fn pad_templates() -> &'static [gst::PadTemplate] {
            static T: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
                let caps = gst_video::VideoCapsBuilder::new().format_list([gst_video::VideoFormat::Bgra, gst_video::VideoFormat::Bgrx]).build();
                vec![
                    gst::PadTemplate::new("src", gst::PadDirection::Src, gst::PadPresence::Always, &caps).unwrap(),
                    gst::PadTemplate::new("sink", gst::PadDirection::Sink, gst::PadPresence::Always, &caps).unwrap(),
                ]
            });
            T.as_ref()
        }
    }
    impl BaseTransformImpl for WsBlur {
        const MODE: gst_base::subclass::BaseTransformMode = gst_base::subclass::BaseTransformMode::AlwaysInPlace;
        const PASSTHROUGH_ON_SAME_CAPS: bool = false;
        const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;
    }
    impl VideoFilterImpl for WsBlur {
        fn transform_frame_ip(&self, frame: &mut gst_video::VideoFrameRef<&mut gst::BufferRef>) -> Result<gst::FlowSuccess, gst::FlowError> {
            let regions = self.regions.lock().unwrap().1.clone();
            if regions.is_empty() { return Ok(gst::FlowSuccess::Ok); }
            let t = frame.buffer().pts().map(|p| p.nseconds() as f64 / 1e9).unwrap_or(0.0);
            let (w, h) = (frame.width() as usize, frame.height() as usize);
            let stride = frame.plane_stride()[0] as usize;
            let data = frame.plane_data_mut(0).map_err(|_| gst::FlowError::Error)?;
            for r in &regions {
                if r.t1 > r.t0 && (t < r.t0 || t >= r.t1) { continue; }
                let x0 = (r.x * w as f64).round().clamp(0.0, w as f64) as usize;
                let y0 = (r.y * h as f64).round().clamp(0.0, h as f64) as usize;
                let x1 = ((r.x + r.w) * w as f64).round().clamp(0.0, w as f64) as usize;
                let y1 = ((r.y + r.h) * h as f64).round().clamp(0.0, h as f64) as usize;
                if x1 <= x0 + 1 || y1 <= y0 + 1 { continue; }
                let rad = ((0.004 + r.strength.clamp(0.0, 1.0) * 0.04) * h as f64 * r.size).round().max(1.0) as usize;
                if r.pixelate {
                    // Kachelgröße an der Bildhöhe ausgerichtet, damit Vorschau und Export gleich aussehen
                    let bs = ((0.008 + r.strength.clamp(0.0, 1.0) * 0.05) * h as f64 * r.size).round().max(2.0) as usize;
                    pixel_rect(data, stride, x0, y0, x1 - x0, y1 - y0, bs);
                } else {
                    blur_rect(data, stride, x0, y0, x1 - x0, y1 - y0, rad);
                }
            }
            Ok(gst::FlowSuccess::Ok)
        }
    }
}

glib::wrapper! {
    pub struct WsBlur(ObjectSubclass<imp::WsBlur>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register() -> Result<(), glib::BoolError> {
    gst::Element::register(None, "wsblur", gst::Rank::NONE, WsBlur::static_type())
}
