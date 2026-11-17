//! `wsroll`: rollt ein Bild von links auf und wieder zusammen (über die Transparenz).
//! Die Beschreibung steht in der Eigenschaft `roll`: "ein_start,ein_dauer,aus_start,aus_dauer,x0,x1"
//! (Zeiten in Sekunden der Quelle, x0/x1 als Anteil der Bildbreite, zwischen denen gerollt wird).
use gst::glib;
use gst::subclass::prelude::*;
use gst_video::prelude::*;
use gst_video::subclass::prelude::*;
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Roll {
    pub on: bool,
    pub in_start: f64,
    pub in_len: f64,
    pub out_start: f64,
    pub out_len: f64,
    pub x0: f64,
    pub x1: f64,
}

pub fn parse(s: &str) -> Roll {
    let v: Vec<f64> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if v.len() < 6 { return Roll::default(); }
    Roll { on: true, in_start: v[0], in_len: v[1], out_start: v[2], out_len: v[3], x0: v[4], x1: v[5] }
}

fn ease(k: f64) -> f64 { let k = k.clamp(0.0, 1.0); k * k * (3.0 - 2.0 * k) }

/// Sichtbarer Anteil (0..1) zum Zeitpunkt t
pub fn amount(r: &Roll, t: f64) -> f64 {
    let mut a: f64 = 1.0;
    if r.in_len > 0.0 { a = a.min(ease((t - r.in_start) / r.in_len)); }
    if r.out_len > 0.0 { a = a.min(ease(1.0 - (t - r.out_start) / r.out_len)); }
    a
}

/// Alles rechts der Rollkante durchsichtig machen (BGRA, Alpha im vierten Byte).
pub fn apply(r: &Roll, t: f64, data: &mut [u8], stride: usize, w: usize, h: usize) {
    if !r.on { return; }
    let a = amount(r, t);
    if a >= 0.9999 { return; }
    let edge = (r.x0 + (r.x1 - r.x0) * a) * w as f64;
    let full = edge.floor().clamp(0.0, w as f64) as usize;
    let frac = (edge - edge.floor()).clamp(0.0, 1.0);
    for y in 0..h {
        let row = &mut data[y * stride..y * stride + w * 4];
        if full < w {
            let p = full * 4 + 3;
            row[p] = (row[p] as f64 * frac) as u8;
            for x in full + 1..w { row[x * 4 + 3] = 0; }
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WsRoll {
        pub roll: Mutex<(String, Roll)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WsRoll {
        const NAME: &'static str = "WsRoll";
        type Type = super::WsRoll;
        type ParentType = gst_video::VideoFilter;
    }

    impl ObjectImpl for WsRoll {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| vec![glib::ParamSpecString::builder("roll").build()]);
            PROPS.as_ref()
        }
        fn set_property(&self, _id: usize, value: &glib::Value, _pspec: &glib::ParamSpec) {
            let s = value.get::<Option<String>>().ok().flatten().unwrap_or_default();
            let r = parse(&s);
            *self.roll.lock().unwrap() = (s, r);
        }
        fn property(&self, _id: usize, _pspec: &glib::ParamSpec) -> glib::Value {
            self.roll.lock().unwrap().0.to_value()
        }
    }
    impl GstObjectImpl for WsRoll {}
    impl ElementImpl for WsRoll {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new("WS.Video Rollen", "Filter/Effect/Video", "Rollt ein Bild auf und zusammen", "WS.Video")
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
    impl BaseTransformImpl for WsRoll {
        const MODE: gst_base::subclass::BaseTransformMode = gst_base::subclass::BaseTransformMode::AlwaysInPlace;
        const PASSTHROUGH_ON_SAME_CAPS: bool = false;
        const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;
    }
    impl VideoFilterImpl for WsRoll {
        fn transform_frame_ip(&self, frame: &mut gst_video::VideoFrameRef<&mut gst::BufferRef>) -> Result<gst::FlowSuccess, gst::FlowError> {
            let r = self.roll.lock().unwrap().1;
            let t = frame.buffer().pts().map(|p| p.nseconds() as f64 / 1e9).unwrap_or(0.0);
            let (w, h) = (frame.width() as usize, frame.height() as usize);
            let stride = frame.plane_stride()[0] as usize;
            let data = frame.plane_data_mut(0).map_err(|_| gst::FlowError::Error)?;
            apply(&r, t, data, stride, w, h);
            Ok(gst::FlowSuccess::Ok)
        }
    }
}

glib::wrapper! {
    pub struct WsRoll(ObjectSubclass<imp::WsRoll>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register() -> Result<(), glib::BoolError> {
    gst::Element::register(None, "wsroll", gst::Rank::NONE, WsRoll::static_type())
}
