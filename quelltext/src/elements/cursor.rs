//! `wscursor`: zeichnet den Mauszeiger aus den aufgezeichneten Daten ins Bild
//! (Größe, Glättung, Hervorhebung, Klick-Ringe).
use crate::cursordata::CursorData;
use gst::glib;
use gst::subclass::prelude::*;
use gst_video::prelude::*;
use gst_video::subclass::prelude::*;
use gtk::cairo;
use std::sync::{Arc, LazyLock, Mutex};

#[derive(Clone, Debug)]
struct Settings {
    data: String,
    monitor: i32,
    show: bool,
    scale: f64,
    smooth: f64,
    highlight: bool,
    highlight_color: u32,
    clicks: bool,
    click_color: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Settings { data: String::new(), monitor: -1, show: true, scale: 1.0, smooth: 0.0, highlight: false, highlight_color: 0xffd23d, clicks: true, click_color: 0xff8a3d }
    }
}

fn rgb(c: u32) -> (f64, f64, f64) {
    (((c >> 16) & 0xff) as f64 / 255.0, ((c >> 8) & 0xff) as f64 / 255.0, (c & 0xff) as f64 / 255.0)
}

/// Eigener, neutraler Zeiger (Pfeil); Einheit = Zeigerhöhe.
pub fn draw_pointer(cr: &cairo::Context, x: f64, y: f64, size: f64) {
    const P: [(f64, f64); 7] = [(0.0, 0.0), (0.0, 0.80), (0.19, 0.62), (0.33, 0.95), (0.45, 0.90), (0.31, 0.58), (0.57, 0.58)];
    cr.new_path();
    for (i, (px, py)) in P.iter().enumerate() {
        if i == 0 { cr.move_to(x + px * size, y + py * size); } else { cr.line_to(x + px * size, y + py * size); }
    }
    cr.close_path();
    cr.set_line_join(cairo::LineJoin::Round);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.25);
    cr.set_line_width(size * 0.16);
    let _ = cr.stroke_preserve();
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.set_line_width(size * 0.09);
    let _ = cr.stroke_preserve();
    cr.set_source_rgb(0.08, 0.08, 0.1);
    let _ = cr.fill();
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WsCursor {
        settings: Mutex<Settings>,
        data: Mutex<Option<(String, Arc<CursorData>)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WsCursor {
        const NAME: &'static str = "WsCursor";
        type Type = super::WsCursor;
        type ParentType = gst_video::VideoFilter;
    }
    impl ObjectImpl for WsCursor {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| {
                vec![
                    glib::ParamSpecString::builder("cur-data").build(),
                    glib::ParamSpecInt::builder("cur-monitor").minimum(-1).maximum(64).default_value(-1).build(),
                    glib::ParamSpecBoolean::builder("cur-show").default_value(true).build(),
                    glib::ParamSpecDouble::builder("cur-scale").minimum(0.1).maximum(10.0).default_value(1.0).build(),
                    glib::ParamSpecDouble::builder("cur-smooth").minimum(0.0).maximum(5.0).default_value(0.0).build(),
                    glib::ParamSpecBoolean::builder("cur-highlight").default_value(false).build(),
                    glib::ParamSpecUInt::builder("cur-highlight-color").default_value(0xffd23d).build(),
                    glib::ParamSpecBoolean::builder("cur-clicks").default_value(true).build(),
                    glib::ParamSpecUInt::builder("cur-click-color").default_value(0xff8a3d).build(),
                ]
            });
            PROPS.as_ref()
        }
        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            let mut s = self.settings.lock().unwrap();
            match pspec.name() {
                "cur-data" => s.data = value.get::<Option<String>>().unwrap().unwrap_or_default(),
                "cur-monitor" => s.monitor = value.get().unwrap(),
                "cur-show" => s.show = value.get().unwrap(),
                "cur-scale" => s.scale = value.get().unwrap(),
                "cur-smooth" => s.smooth = value.get().unwrap(),
                "cur-highlight" => s.highlight = value.get().unwrap(),
                "cur-highlight-color" => s.highlight_color = value.get().unwrap(),
                "cur-clicks" => s.clicks = value.get().unwrap(),
                "cur-click-color" => s.click_color = value.get().unwrap(),
                _ => {}
            }
        }
        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            let s = self.settings.lock().unwrap();
            match pspec.name() {
                "cur-data" => s.data.to_value(),
                "cur-monitor" => s.monitor.to_value(),
                "cur-show" => s.show.to_value(),
                "cur-scale" => s.scale.to_value(),
                "cur-smooth" => s.smooth.to_value(),
                "cur-highlight" => s.highlight.to_value(),
                "cur-highlight-color" => s.highlight_color.to_value(),
                "cur-clicks" => s.clicks.to_value(),
                "cur-click-color" => s.click_color.to_value(),
                _ => unreachable!(),
            }
        }
    }
    impl GstObjectImpl for WsCursor {}
    impl ElementImpl for WsCursor {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new("WS.Video Mauszeiger", "Filter/Effect/Video", "Mauszeiger aus Aufnahmedaten zeichnen", "WS.Video")
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
    impl BaseTransformImpl for WsCursor {
        const MODE: gst_base::subclass::BaseTransformMode = gst_base::subclass::BaseTransformMode::AlwaysInPlace;
        const PASSTHROUGH_ON_SAME_CAPS: bool = false;
        const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;
    }
    impl VideoFilterImpl for WsCursor {
        fn transform_frame_ip(&self, frame: &mut gst_video::VideoFrameRef<&mut gst::BufferRef>) -> Result<gst::FlowSuccess, gst::FlowError> {
            let s = self.settings.lock().unwrap().clone();
            if s.data.is_empty() || !(s.show || s.highlight || s.clicks) { return Ok(gst::FlowSuccess::Ok); }
            let data = {
                let mut g = self.data.lock().unwrap();
                let reload = match g.as_ref() { Some((p, _)) => *p != s.data, None => true };
                if reload {
                    let d = CursorData::load(&s.data).unwrap_or_default();
                    *g = Some((s.data.clone(), Arc::new(d)));
                }
                g.as_ref().unwrap().1.clone()
            };
            if data.samples.is_empty() { return Ok(gst::FlowSuccess::Ok); }
            let t = frame.buffer().pts().map(|p| p.nseconds() as f64 / 1e9).unwrap_or(0.0);
            let (w, h) = (frame.width() as i32, frame.height() as i32);
            // Der Monitor wird an der Größe der Aufnahme erkannt, nicht an der (vielleicht verkleinerten) Bildgröße hier
            let (gw, gh) = if data.stream_w > 0 && data.stream_h > 0 { (data.stream_w, data.stream_h) } else { (w as u32, h as u32) };
            let Some(mon) = data.pick_monitor(s.monitor, gw, gh) else { return Ok(gst::FlowSuccess::Ok) };
            if mon.w <= 0.0 || mon.h <= 0.0 { return Ok(gst::FlowSuccess::Ok); }
            let map = |gx: f64, gy: f64| -> (f64, f64) { ((gx - mon.x) / mon.w * w as f64, (gy - mon.y) / mon.h * h as f64) };
            let Some((gx, gy)) = data.smooth_pos_at(t, s.smooth) else { return Ok(gst::FlowSuccess::Ok) };
            let (cx, cy) = map(gx, gy);
            let k = h as f64 / mon.h; // Bildpixel je logischem Pixel
            let fmt = if frame.format() == gst_video::VideoFormat::Bgra { cairo::Format::ARgb32 } else { cairo::Format::Rgb24 };
            let stride = frame.plane_stride()[0];
            let plane = frame.plane_data_mut(0).map_err(|_| gst::FlowError::Error)?;
            // Die Cairo-Fläche lebt nur innerhalb dieses Aufrufs und zeigt auf die Bilddaten.
            let surf = unsafe { cairo::ImageSurface::create_for_data_unsafe(plane.as_mut_ptr(), fmt, w, h, stride) }.map_err(|_| gst::FlowError::Error)?;
            {
                let cr = cairo::Context::new(&surf).map_err(|_| gst::FlowError::Error)?;
                let inside = cx >= -50.0 && cy >= -50.0 && cx <= w as f64 + 50.0 && cy <= h as f64 + 50.0;
                if s.highlight && inside {
                    let (r, g, b) = rgb(s.highlight_color);
                    cr.set_source_rgba(r, g, b, 0.38);
                    cr.arc(cx, cy, 26.0 * k * s.scale.max(0.5), 0.0, std::f64::consts::TAU);
                    let _ = cr.fill();
                }
                if s.clicks {
                    let life = 0.5;
                    let start = data.clicks.partition_point(|c| c[0] < t - life);
                    for c in data.clicks[start..].iter() {
                        if c[0] > t { break; }
                        let p = ((t - c[0]) / life).clamp(0.0, 1.0);
                        let (px, py) = if s.smooth > 0.0 { (cx, cy) } else { map(c[2], c[3]) };
                        let (r, g, b) = if c[1] as i32 == 1 { (0.24, 0.55, 0.99) } else { rgb(s.click_color) };
                        cr.set_source_rgba(r, g, b, (1.0 - p) * 0.9);
                        cr.set_line_width((4.0 * k * s.scale.max(0.5)).max(1.5));
                        cr.arc(px, py, (8.0 + 34.0 * p) * k * s.scale.max(0.5), 0.0, std::f64::consts::TAU);
                        let _ = cr.stroke();
                    }
                }
                if s.show && inside {
                    draw_pointer(&cr, cx, cy, 26.0 * k * s.scale);
                }
            }
            surf.flush();
            drop(surf);
            Ok(gst::FlowSuccess::Ok)
        }
    }
}

glib::wrapper! {
    pub struct WsCursor(ObjectSubclass<imp::WsCursor>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
}

pub fn register() -> Result<(), glib::BoolError> {
    gst::Element::register(None, "wscursor", gst::Rank::NONE, WsCursor::static_type())
}
