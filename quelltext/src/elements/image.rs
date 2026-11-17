//! `wsimage`: ersetzt das Bild durch eine PNG-Datei mit Transparenz.
//! Damit zeigt die Schnitt-Engine Anmerkungen (Text, Formen, Pfeile) und Schatten: Ändert sich das Bild,
//! wird nur die Eigenschaft `img-path` gesetzt, die Zeitleiste selbst bleibt unverändert.
use gst::glib;
use gst::subclass::prelude::*;
use gst_video::prelude::*;
use gst_video::subclass::prelude::*;
use gtk::cairo;
use std::sync::{LazyLock, Mutex};

struct Cache {
    path: String,
    w: u32,
    h: u32,
    /// BGRA, nicht vormultipliziert, w*h*4 Bytes
    data: Vec<u8>,
}

/// PNG laden und auf die Bildgröße bringen.
fn load(path: &str, w: u32, h: u32) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open(path).ok()?;
    let png = cairo::ImageSurface::create_from_png(&mut f).ok()?;
    let (pw, ph) = (png.width() as f64, png.height() as f64);
    if pw < 1.0 || ph < 1.0 { return None; }
    let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).ok()?;
    {
        let cr = cairo::Context::new(&surf).ok()?;
        cr.scale(w as f64 / pw, h as f64 / ph);
        cr.set_source_surface(&png, 0.0, 0.0).ok()?;
        cr.source().set_filter(cairo::Filter::Good);
        cr.source().set_extend(cairo::Extend::Pad);
        cr.set_operator(cairo::Operator::Source);
        cr.paint().ok()?;
    }
    let stride = surf.stride() as usize;
    let data = surf.data().ok()?;
    let wu = w as usize;
    let mut out = vec![0u8; wu * h as usize * 4];
    for y in 0..h as usize {
        let src = &data[y * stride..y * stride + wu * 4];
        let dst = &mut out[y * wu * 4..(y + 1) * wu * 4];
        for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            let a = s[3] as u32;
            if a == 255 {
                d.copy_from_slice(s);
            } else if a > 0 {
                // Cairo speichert vormultipliziert, GStreamer erwartet die reinen Farbwerte
                d[0] = ((s[0] as u32 * 255 + a / 2) / a).min(255) as u8;
                d[1] = ((s[1] as u32 * 255 + a / 2) / a).min(255) as u8;
                d[2] = ((s[2] as u32 * 255 + a / 2) / a).min(255) as u8;
                d[3] = a as u8;
            }
        }
    }
    Some(out)
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WsImage {
        pub path: Mutex<String>,
        pub roll: Mutex<(String, super::super::roll::Roll)>,
        pub(super) cache: Mutex<Option<Cache>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WsImage {
        const NAME: &'static str = "WsImage";
        type Type = super::WsImage;
        type ParentType = gst_video::VideoFilter;
    }

    impl ObjectImpl for WsImage {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| vec![glib::ParamSpecString::builder("img-path").build(), glib::ParamSpecString::builder("roll").build()]);
            PROPS.as_ref()
        }
        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            let s = value.get::<Option<String>>().ok().flatten().unwrap_or_default();
            if pspec.name() == "roll" {
                let r = super::super::roll::parse(&s);
                *self.roll.lock().unwrap() = (s, r);
            } else {
                *self.path.lock().unwrap() = s;
            }
        }
        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            if pspec.name() == "roll" { self.roll.lock().unwrap().0.to_value() } else { self.path.lock().unwrap().to_value() }
        }
    }
    impl GstObjectImpl for WsImage {}
    impl ElementImpl for WsImage {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new("WS.Video Bild", "Filter/Effect/Video", "Zeigt eine PNG-Datei mit Transparenz", "WS.Video")
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
    impl BaseTransformImpl for WsImage {
        const MODE: gst_base::subclass::BaseTransformMode = gst_base::subclass::BaseTransformMode::AlwaysInPlace;
        const PASSTHROUGH_ON_SAME_CAPS: bool = false;
        const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;
    }
    impl VideoFilterImpl for WsImage {
        fn transform_frame_ip(&self, frame: &mut gst_video::VideoFrameRef<&mut gst::BufferRef>) -> Result<gst::FlowSuccess, gst::FlowError> {
            let path = self.path.lock().unwrap().clone();
            let (w, h) = (frame.width(), frame.height());
            let mut guard = self.cache.lock().unwrap();
            let rebuild = match guard.as_ref() { Some(c) => c.path != path || c.w != w || c.h != h, None => true };
            if rebuild {
                let data = if path.is_empty() { None } else { load(&path, w, h) };
                *guard = Some(Cache { path, w, h, data: data.unwrap_or_else(|| vec![0u8; w as usize * h as usize * 4]) });
            }
            let c = guard.as_ref().unwrap();
            let roll = self.roll.lock().unwrap().1;
            let t = frame.buffer().pts().map(|p| p.nseconds() as f64 / 1e9).unwrap_or(0.0);
            let stride = frame.plane_stride()[0] as usize;
            let data = frame.plane_data_mut(0).map_err(|_| gst::FlowError::Error)?;
            let row = w as usize * 4;
            for y in 0..h as usize {
                data[y * stride..y * stride + row].copy_from_slice(&c.data[y * row..(y + 1) * row]);
            }
            super::super::roll::apply(&roll, t, data, stride, w as usize, h as usize);
            Ok(gst::FlowSuccess::Ok)
        }
    }
}

glib::wrapper! {
    pub struct WsImage(ObjectSubclass<imp::WsImage>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register() -> Result<(), glib::BoolError> {
    gst::Element::register(None, "wsimage", gst::Rank::NONE, WsImage::static_type())
}
