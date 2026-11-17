//! `wsshape`: Zuschnitt, Form (Rechteck, abgerundet, Kreis/Ellipse) und Rahmen für ein Videobild.
//! Arbeitet auf BGRA in Quellauflösung; alles außerhalb der Form wird transparent.
use gst::glib;
use gst::subclass::prelude::*;
use gst_video::prelude::*;
use gst_video::subclass::prelude::*;
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub shape: i32, // 0 Rechteck, 1 abgerundet, 2 Ellipse/Kreis
    pub radius: f64, // 0..1 relativ zur halben kürzeren Seite
    pub crop_l: f64,
    pub crop_t: f64,
    pub crop_r: f64,
    pub crop_b: f64,
    pub border: f64, // relativ zur kürzeren Seite des sichtbaren Bereichs
    pub border_color: u32, // 0xRRGGBB
    pub chroma: bool,
    pub chroma_color: u32,
    pub chroma_tolerance: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Settings { shape: 0, radius: 0.2, crop_l: 0.0, crop_t: 0.0, crop_r: 0.0, crop_b: 0.0, border: 0.0, border_color: 0xffffff, chroma: false, chroma_color: 0x00d000, chroma_tolerance: 0.3 }
    }
}

struct Cache {
    settings: Settings,
    w: u32,
    h: u32,
    mask: Vec<u8>,   // Deckkraft der Form
    border: Vec<u8>, // Deckkraft des Rahmens
}

/// Abstand-basierte Deckkraft einer abgerundeten Form (0..1) am Punkt (px,py).
fn coverage(px: f64, py: f64, x0: f64, y0: f64, x1: f64, y1: f64, r: f64, inset: f64) -> f64 {
    let (x0, y0, x1, y1) = (x0 + inset, y0 + inset, x1 - inset, y1 - inset);
    if x1 <= x0 || y1 <= y0 { return 0.0; }
    let r = (r - inset).max(0.0).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
    let cx = px.clamp(x0 + r, x1 - r);
    let cy = py.clamp(y0 + r, y1 - r);
    let d = if r > 0.0 {
        ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r
    } else {
        (x0 - px).max(px - x1).max(y0 - py).max(py - y1)
    };
    (0.5 - d).clamp(0.0, 1.0)
}

fn ellipse_cov(px: f64, py: f64, x0: f64, y0: f64, x1: f64, y1: f64, inset: f64) -> f64 {
    let rx = (x1 - x0) / 2.0 - inset;
    let ry = (y1 - y0) / 2.0 - inset;
    if rx <= 0.0 || ry <= 0.0 { return 0.0; }
    let dx = (px - (x0 + x1) / 2.0) / rx;
    let dy = (py - (y0 + y1) / 2.0) / ry;
    let k = (dx * dx + dy * dy).sqrt();
    // Näherung des Abstands zum Ellipsenrand in Pixeln
    let d = (k - 1.0) * rx.min(ry);
    (0.5 - d).clamp(0.0, 1.0)
}

fn build_cache(s: &Settings, w: u32, h: u32) -> Cache {
    let (wf, hf) = (w as f64, h as f64);
    let x0 = (s.crop_l * wf).clamp(0.0, wf - 1.0);
    let y0 = (s.crop_t * hf).clamp(0.0, hf - 1.0);
    let x1 = (wf - s.crop_r * wf).clamp(x0 + 1.0, wf);
    let y1 = (hf - s.crop_b * hf).clamp(y0 + 1.0, hf);
    let short = (x1 - x0).min(y1 - y0);
    let r = match s.shape { 1 => s.radius.clamp(0.0, 1.0) * short / 2.0, _ => 0.0 };
    let bw = (s.border * short).max(0.0);
    let mut mask = vec![0u8; (w * h) as usize];
    let mut border = vec![0u8; (w * h) as usize];
    for y in 0..h {
        let py = y as f64 + 0.5;
        for x in 0..w {
            let px = x as f64 + 0.5;
            let i = (y * w + x) as usize;
            let outer = if s.shape == 2 { ellipse_cov(px, py, x0, y0, x1, y1, 0.0) } else { coverage(px, py, x0, y0, x1, y1, r, 0.0) };
            mask[i] = (outer * 255.0 + 0.5) as u8;
            if bw > 0.0 && outer > 0.0 {
                let inner = if s.shape == 2 { ellipse_cov(px, py, x0, y0, x1, y1, bw) } else { coverage(px, py, x0, y0, x1, y1, r, bw) };
                border[i] = ((outer * (1.0 - inner)) * 255.0 + 0.5) as u8;
            }
        }
    }
    Cache { settings: s.clone(), w, h, mask, border }
}

/// Farbe freistellen (Greenscreen): Abstand im Farbton (U/V), weicher Übergang, leichte Entsättigung des Farbstichs.
fn chroma_key(data: &mut [u8], w: usize, h: usize, stride: usize, color: u32, tolerance: f64) {
    let uv = |r: i32, g: i32, b: i32| -> (i32, i32) {
        ((-43 * r - 85 * g + 128 * b) / 256 + 128, (128 * r - 107 * g - 21 * b) / 256 + 128)
    };
    let (kr, kg, kb) = (((color >> 16) & 0xff) as i32, ((color >> 8) & 0xff) as i32, (color & 0xff) as i32);
    let (ku, kv) = uv(kr, kg, kb);
    let sim = (tolerance.clamp(0.01, 1.0) * 0.5) * 255.0 * std::f64::consts::SQRT_2;
    let blend = 0.08 * 255.0 * std::f64::consts::SQRT_2;
    let sim2 = (sim * sim) as i32;
    let outer2 = ((sim + blend) * (sim + blend)) as i32;
    let spill2 = ((sim + 3.0 * blend) * (sim + 3.0 * blend)) as i32;
    let green_key = kg > kr && kg > kb;
    let blue_key = kb > kr && kb > kg;
    for y in 0..h {
        let row = &mut data[y * stride..y * stride + w * 4];
        for p in row.chunks_exact_mut(4) {
            let (b, g, r) = (p[0] as i32, p[1] as i32, p[2] as i32);
            let (u, v) = uv(r, g, b);
            let d2 = (u - ku) * (u - ku) + (v - kv) * (v - kv);
            if d2 <= sim2 {
                p[3] = 0;
                continue;
            }
            if d2 < outer2 {
                let d = (d2 as f64).sqrt();
                let a = ((d - sim) / blend).clamp(0.0, 1.0);
                p[3] = (p[3] as f64 * a) as u8;
            }
            if d2 < spill2 {
                if green_key && g > r.max(b) { p[1] = ((g + r.max(b)) / 2) as u8; }
                if blue_key && b > r.max(g) { p[0] = ((b + r.max(g)) / 2) as u8; }
            }
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WsShape {
        pub settings: Mutex<Settings>,
        cache: Mutex<Option<Cache>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WsShape {
        const NAME: &'static str = "WsShape";
        type Type = super::WsShape;
        type ParentType = gst_video::VideoFilter;
    }

    impl ObjectImpl for WsShape {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| {
                let f = |n: &str, d: f64| glib::ParamSpecDouble::builder(n).minimum(0.0).maximum(1.0).default_value(d).build();
                vec![
                    glib::ParamSpecInt::builder("shape").minimum(0).maximum(2).default_value(0).build(),
                    f("radius", 0.2),
                    f("crop-left", 0.0),
                    f("crop-top", 0.0),
                    f("crop-right", 0.0),
                    f("crop-bottom", 0.0),
                    f("border", 0.0),
                    glib::ParamSpecUInt::builder("border-color").default_value(0xffffff).build(),
                    glib::ParamSpecBoolean::builder("chroma").default_value(false).build(),
                    glib::ParamSpecUInt::builder("chroma-color").default_value(0x00d000).build(),
                    f("chroma-tolerance", 0.3),
                ]
            });
            PROPS.as_ref()
        }
        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            let mut s = self.settings.lock().unwrap();
            match pspec.name() {
                "shape" => s.shape = value.get().unwrap(),
                "radius" => s.radius = value.get().unwrap(),
                "crop-left" => s.crop_l = value.get().unwrap(),
                "crop-top" => s.crop_t = value.get().unwrap(),
                "crop-right" => s.crop_r = value.get().unwrap(),
                "crop-bottom" => s.crop_b = value.get().unwrap(),
                "border" => s.border = value.get().unwrap(),
                "border-color" => s.border_color = value.get().unwrap(),
                "chroma" => s.chroma = value.get().unwrap(),
                "chroma-color" => s.chroma_color = value.get().unwrap(),
                "chroma-tolerance" => s.chroma_tolerance = value.get().unwrap(),
                _ => {}
            }
        }
        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            let s = self.settings.lock().unwrap();
            match pspec.name() {
                "shape" => s.shape.to_value(),
                "radius" => s.radius.to_value(),
                "crop-left" => s.crop_l.to_value(),
                "crop-top" => s.crop_t.to_value(),
                "crop-right" => s.crop_r.to_value(),
                "crop-bottom" => s.crop_b.to_value(),
                "border" => s.border.to_value(),
                "border-color" => s.border_color.to_value(),
                "chroma" => s.chroma.to_value(),
                "chroma-color" => s.chroma_color.to_value(),
                "chroma-tolerance" => s.chroma_tolerance.to_value(),
                _ => unreachable!(),
            }
        }
    }
    impl GstObjectImpl for WsShape {}
    impl ElementImpl for WsShape {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new("WS.Video Form", "Filter/Effect/Video", "Zuschnitt, Form und Rahmen", "WS.Video")
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
    impl BaseTransformImpl for WsShape {
        const MODE: gst_base::subclass::BaseTransformMode = gst_base::subclass::BaseTransformMode::AlwaysInPlace;
        const PASSTHROUGH_ON_SAME_CAPS: bool = false;
        const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;
    }
    impl VideoFilterImpl for WsShape {
        fn transform_frame_ip(&self, frame: &mut gst_video::VideoFrameRef<&mut gst::BufferRef>) -> Result<gst::FlowSuccess, gst::FlowError> {
            let s = self.settings.lock().unwrap().clone();
            let (w, h) = (frame.width(), frame.height());
            let mut guard = self.cache.lock().unwrap();
            let rebuild = match guard.as_ref() { Some(c) => c.settings != s || c.w != w || c.h != h, None => true };
            if rebuild { *guard = Some(build_cache(&s, w, h)); }
            let c = guard.as_ref().unwrap();
            let stride = frame.plane_stride()[0] as usize;
            let data = frame.plane_data_mut(0).map_err(|_| gst::FlowError::Error)?;
            let (br, bg, bb) = (((s.border_color >> 16) & 0xff) as u32, ((s.border_color >> 8) & 0xff) as u32, (s.border_color & 0xff) as u32);
            let has_border = s.border > 0.0;
            if s.chroma {
                chroma_key(data, w as usize, h as usize, stride, s.chroma_color, s.chroma_tolerance);
            }
            for y in 0..h as usize {
                let row = &mut data[y * stride..y * stride + w as usize * 4];
                let m = &c.mask[y * w as usize..(y + 1) * w as usize];
                let b = &c.border[y * w as usize..(y + 1) * w as usize];
                for x in 0..w as usize {
                    let p = &mut row[x * 4..x * 4 + 4];
                    let mv = m[x] as u32;
                    if mv == 0 { p[3] = 0; continue; }
                    if has_border {
                        let bv = b[x] as u32;
                        if bv > 0 {
                            // Rahmenfarbe über das Bild legen (Deckkraft relativ zur Formdeckkraft)
                            let k = (bv * 255 / mv).min(255);
                            p[0] = ((bb * k + p[0] as u32 * (255 - k)) / 255) as u8;
                            p[1] = ((bg * k + p[1] as u32 * (255 - k)) / 255) as u8;
                            p[2] = ((br * k + p[2] as u32 * (255 - k)) / 255) as u8;
                            p[3] = ((255 * k + p[3] as u32 * (255 - k)) / 255) as u8;
                        }
                    }
                    if mv < 255 { p[3] = ((p[3] as u32 * mv) / 255) as u8; }
                }
            }
            Ok(gst::FlowSuccess::Ok)
        }
    }
}

glib::wrapper! {
    pub struct WsShape(ObjectSubclass<imp::WsShape>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register() -> Result<(), glib::BoolError> {
    gst::Element::register(None, "wsshape", gst::Rank::NONE, WsShape::static_type())
}
