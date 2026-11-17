//! `wsperson`: erkennt die Person im Kamerabild und ersetzt den Hintergrund
//! (durchsichtig, unscharf oder eigenes Bild). Die Erkennung läuft mit dem frei lizenzierten
//! Selfie-Segmentierungsmodell von MediaPipe (Apache 2.0), eingebettet als ONNX und gerechnet mit tract.
use gst::glib;
use gst::subclass::prelude::*;
use gst_video::prelude::*;
use gst_video::subclass::prelude::*;
use std::sync::{LazyLock, Mutex};
use tract_onnx::prelude::*;

const N: usize = 256;
type Net = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

static NET: LazyLock<Option<Net>> = LazyLock::new(|| {
    let bytes: &[u8] = include_bytes!("person.onnx");
    let mut rd = std::io::Cursor::new(bytes);
    tract_onnx::onnx().model_for_read(&mut rd).and_then(|m| m.into_optimized()).and_then(|m| m.into_runnable()).map_err(|e| eprintln!("Personenerkennung nicht verfügbar: {e}")).ok()
});

#[derive(Default)]
struct State {
    prev: Vec<f32>,
    prev_t: f64,
    /// Hintergrundbild passend zur Bildgröße: (Pfad, Breite, Höhe, BGRA)
    bg: Option<(String, usize, usize, Vec<u8>)>,
}

/// Maske (N*N, 0..1) für ein BGRA-Bild berechnen.
fn mask(data: &[u8], stride: usize, w: usize, h: usize) -> Option<Vec<f32>> {
    let net = NET.as_ref()?;
    let mut inp = vec![0f32; 3 * N * N];
    let (fx, fy) = (w as f64 / N as f64, h as f64 / N as f64);
    for y in 0..N {
        let y0 = ((y as f64 + 0.25) * fy) as usize;
        let y1 = (((y as f64 + 0.75) * fy) as usize).min(h - 1);
        for x in 0..N {
            let x0 = ((x as f64 + 0.25) * fx) as usize;
            let x1 = (((x as f64 + 0.75) * fx) as usize).min(w - 1);
            let mut s = [0u32; 3];
            for (yy, xx) in [(y0, x0), (y0, x1), (y1, x0), (y1, x1)] {
                let p = yy.min(h - 1) * stride + xx.min(w - 1) * 4;
                s[0] += data[p + 2] as u32;
                s[1] += data[p + 1] as u32;
                s[2] += data[p] as u32;
            }
            let i = y * N + x;
            inp[i] = s[0] as f32 / 1020.0;
            inp[N * N + i] = s[1] as f32 / 1020.0;
            inp[2 * N * N + i] = s[2] as f32 / 1020.0;
        }
    }
    let t: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, N, N), inp).ok()?.into();
    let out = net.run(tvec!(t.into())).ok()?;
    let v = out[0].to_array_view::<f32>().ok()?;
    // Kante etwas schärfen
    Some(v.iter().map(|m| ((m - 0.3) / 0.4).clamp(0.0, 1.0)).collect())
}

/// Stark weichgezeichnete Fassung des Bildes (klein gerechnet), als BGRA in voller Größe.
fn blurred(data: &[u8], stride: usize, w: usize, h: usize) -> Vec<u8> {
    let f = (w.max(h) / 96).max(2);
    let (sw, sh) = ((w / f).max(2), (h / f).max(2));
    let mut small = vec![0f32; sw * sh * 3];
    for y in 0..sh {
        for x in 0..sw {
            let mut s = [0u32; 3];
            let mut n = 0u32;
            let (ya, xa) = (y * f, x * f);
            let mut yy = ya;
            while yy < (ya + f).min(h) {
                let mut xx = xa;
                while xx < (xa + f).min(w) {
                    let p = yy * stride + xx * 4;
                    s[0] += data[p] as u32; s[1] += data[p + 1] as u32; s[2] += data[p + 2] as u32;
                    n += 1;
                    xx += 2;
                }
                yy += 2;
            }
            let i = (y * sw + x) * 3;
            for k in 0..3 { small[i + k] = s[k] as f32 / n.max(1) as f32; }
        }
    }
    // dreimal Kastenfilter waagrecht und senkrecht
    let r = 3usize;
    let mut tmp = small.clone();
    for _ in 0..3 {
        for y in 0..sh { for x in 0..sw { for k in 0..3 {
            let (a, b) = (x.saturating_sub(r), (x + r).min(sw - 1));
            let mut s = 0.0; for xx in a..=b { s += small[(y * sw + xx) * 3 + k]; }
            tmp[(y * sw + x) * 3 + k] = s / (b - a + 1) as f32;
        } } }
        for y in 0..sh { for x in 0..sw { for k in 0..3 {
            let (a, b) = (y.saturating_sub(r), (y + r).min(sh - 1));
            let mut s = 0.0; for yy in a..=b { s += tmp[(yy * sw + x) * 3 + k]; }
            small[(y * sw + x) * 3 + k] = s / (b - a + 1) as f32;
        } } }
    }
    let mut out = vec![255u8; w * h * 4];
    for y in 0..h {
        let gy = ((y as f32 + 0.5) / f as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (gy as usize, gy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..w {
            let gx = ((x as f32 + 0.5) / f as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (gx as usize, gx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            for k in 0..3 {
                let a = small[(y0 * sw + x0) * 3 + k] * (1.0 - tx) + small[(y0 * sw + x1) * 3 + k] * tx;
                let b = small[(y1 * sw + x0) * 3 + k] * (1.0 - tx) + small[(y1 * sw + x1) * 3 + k] * tx;
                out[(y * w + x) * 4 + k] = (a * (1.0 - ty) + b * ty) as u8;
            }
        }
    }
    out
}

/// Bilddatei laden und so skalieren, dass sie das Bild ganz füllt (mittig zugeschnitten).
fn load_bg(path: &str, w: usize, h: usize) -> Option<Vec<u8>> {
    use gtk::gdk_pixbuf::{InterpType, Pixbuf};
    let pb = Pixbuf::from_file(path).ok()?;
    let (pw, ph) = (pb.width() as f64, pb.height() as f64);
    if pw < 1.0 || ph < 1.0 { return None; }
    let k = (w as f64 / pw).max(h as f64 / ph);
    let (nw, nh) = (((pw * k).ceil() as i32).max(w as i32), ((ph * k).ceil() as i32).max(h as i32));
    let pb = pb.scale_simple(nw, nh, InterpType::Bilinear)?;
    let (ox, oy) = ((nw as usize - w) / 2, (nh as usize - h) / 2);
    let (ch, rs) = (pb.n_channels() as usize, pb.rowstride() as usize);
    let bytes = pb.read_pixel_bytes();
    let mut out = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let p = (y + oy) * rs + (x + ox) * ch;
            let o = (y * w + x) * 4;
            out[o] = bytes[p + 2];
            out[o + 1] = bytes[p + 1];
            out[o + 2] = bytes[p];
        }
    }
    Some(out)
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WsPerson {
        /// (Art: 0 aus, 1 durchsichtig, 2 unscharf, 3 Bild, 4 einfarbig grau; Bildpfad)
        pub set: Mutex<(i32, String)>,
        pub(super) st: Mutex<State>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WsPerson {
        const NAME: &'static str = "WsPerson";
        type Type = super::WsPerson;
        type ParentType = gst_video::VideoFilter;
    }

    impl ObjectImpl for WsPerson {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| vec![
                glib::ParamSpecInt::builder("bg-mode").minimum(0).maximum(4).build(),
                glib::ParamSpecString::builder("bg-path").build(),
            ]);
            PROPS.as_ref()
        }
        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            let mut s = self.set.lock().unwrap();
            if pspec.name() == "bg-mode" { s.0 = value.get().unwrap_or(0); } else { s.1 = value.get::<Option<String>>().ok().flatten().unwrap_or_default(); }
        }
        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            let s = self.set.lock().unwrap();
            if pspec.name() == "bg-mode" { s.0.to_value() } else { s.1.to_value() }
        }
    }
    impl GstObjectImpl for WsPerson {}
    impl ElementImpl for WsPerson {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new("WS.Video Hintergrund", "Filter/Effect/Video", "Ersetzt den Hintergrund hinter einer Person", "WS.Video")
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
    impl BaseTransformImpl for WsPerson {
        const MODE: gst_base::subclass::BaseTransformMode = gst_base::subclass::BaseTransformMode::AlwaysInPlace;
        const PASSTHROUGH_ON_SAME_CAPS: bool = false;
        const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;
    }
    impl VideoFilterImpl for WsPerson {
        fn transform_frame_ip(&self, frame: &mut gst_video::VideoFrameRef<&mut gst::BufferRef>) -> Result<gst::FlowSuccess, gst::FlowError> {
            let (mode, path) = self.set.lock().unwrap().clone();
            if mode == 0 { return Ok(gst::FlowSuccess::Ok); }
            let t = frame.buffer().pts().map(|p| p.nseconds() as f64 / 1e9).unwrap_or(0.0);
            let (w, h) = (frame.width() as usize, frame.height() as usize);
            let stride = frame.plane_stride()[0] as usize;
            let data = frame.plane_data_mut(0).map_err(|_| gst::FlowError::Error)?;
            if w < 8 || h < 8 { return Ok(gst::FlowSuccess::Ok); }
            let Some(mut m) = mask(data, stride, w, h) else { return Ok(gst::FlowSuccess::Ok) };
            let mut st = self.st.lock().unwrap();
            // gegen Flackern: mit dem vorigen Bild mitteln, solange fortlaufend abgespielt wird
            if st.prev.len() == m.len() && (t - st.prev_t).abs() < 0.2 && t != st.prev_t {
                for (a, b) in m.iter_mut().zip(st.prev.iter()) { *a = *a * 0.7 + *b * 0.3; }
            }
            st.prev = m.clone();
            st.prev_t = t;
            let bg: Option<Vec<u8>> = match mode {
                2 => Some(blurred(data, stride, w, h)),
                3 => {
                    let fresh = match &st.bg { Some((p, bw, bh, _)) => p != &path || *bw != w || *bh != h, None => true };
                    if fresh { st.bg = load_bg(&path, w, h).map(|d| (path.clone(), w, h, d)); }
                    match &st.bg { Some((_, _, _, d)) => Some(d.clone()), None => Some(blurred(data, stride, w, h)) }
                }
                4 => Some([38u8, 34, 32, 255].repeat(w * h)),
                _ => None,
            };
            let last = (N - 1) as f32;
            let xs: Vec<(usize, usize, f32)> = (0..w).map(|x| {
                let g = ((x as f32 + 0.5) * N as f32 / w as f32 - 0.5).clamp(0.0, last);
                (g as usize, (g as usize + 1).min(N - 1), g.fract())
            }).collect();
            for y in 0..h {
                let g = ((y as f32 + 0.5) * N as f32 / h as f32 - 0.5).clamp(0.0, last);
                let (y0, y1, ty) = (g as usize, (g as usize + 1).min(N - 1), g.fract());
                let row = &mut data[y * stride..y * stride + w * 4];
                for (x, (x0, x1, tx)) in xs.iter().enumerate() {
                    let a = m[y0 * N + x0] * (1.0 - tx) + m[y0 * N + x1] * tx;
                    let b = m[y1 * N + x0] * (1.0 - tx) + m[y1 * N + x1] * tx;
                    let k = a * (1.0 - ty) + b * ty;
                    let p = &mut row[x * 4..x * 4 + 4];
                    match &bg {
                        None => p[3] = (p[3] as f32 * k) as u8,
                        Some(bg) => {
                            if k >= 0.996 { continue; }
                            let q = &bg[(y * w + x) * 4..(y * w + x) * 4 + 3];
                            for c in 0..3 { p[c] = (p[c] as f32 * k + q[c] as f32 * (1.0 - k)) as u8; }
                        }
                    }
                }
            }
            Ok(gst::FlowSuccess::Ok)
        }
    }
}

glib::wrapper! {
    pub struct WsPerson(ObjectSubclass<imp::WsPerson>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register() -> Result<(), glib::BoolError> {
    gst::Element::register(None, "wsperson", gst::Rank::NONE, WsPerson::static_type())
}
