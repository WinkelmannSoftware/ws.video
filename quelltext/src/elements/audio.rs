//! `wsaudio`: Tonbearbeitung in der Schnittkette (Rauschen entfernen, Klang, Lautstärke, Begrenzer).
//! Die Einstellungen stehen als Text in der Eigenschaft `afx` (siehe `audiofx::Params`).
//!
//! Die Rechenkette verzögert den Ton um ein festes Stück. Damit Bild und Ton zusammenbleiben, gibt das Element
//! jedes Sample mit dem Zeitstempel seiner Stelle im Eingang weiter: Der Vorlauf der Kette wird verworfen,
//! am Ende (oder vor einem Sprung) wird der Rest nachgeschoben.
use crate::audiofx::{Chain, Params};
use gst::glib;
use gst::prelude::*;
use gst::subclass::prelude::*;
use std::sync::{LazyLock, Mutex};

mod imp {
    use super::*;

    #[derive(Default)]
    struct State {
        desc: String,
        params: Params,
        dirty: bool,
        rate: u32,
        channels: usize,
        chain: Option<Chain>,
        /// Zeitstempel des ersten Samples seit dem letzten Neubeginn und wie viele Zeitpunkte schon hinausgingen
        base: Option<gst::ClockTime>,
        emitted: u64,
        received: u64,
        skip: usize,
        latency: usize,
    }

    pub struct WsAudio {
        sink: gst::Pad,
        src: gst::Pad,
        state: Mutex<State>,
    }

    impl WsAudio {
        /// Samples durch die Kette schicken und als Puffer mit passendem Zeitstempel verpacken.
        fn run(st: &mut State, mut data: Vec<f32>) -> Option<gst::Buffer> {
            let ch = st.channels.max(1);
            let chain = st.chain.as_mut()?;
            chain.process(&mut data);
            let frames = data.len() / ch;
            let drop = st.skip.min(frames);
            st.skip -= drop;
            let out = &data[drop * ch..];
            let n = (out.len() / ch) as u64;
            if n == 0 { return None; }
            let mut buf = gst::Buffer::with_size(out.len() * 4).ok()?;
            {
                let b = buf.get_mut()?;
                {
                    let mut map = b.map_writable().ok()?;
                    for (dst, v) in map.chunks_exact_mut(4).zip(out) { dst.copy_from_slice(&v.to_le_bytes()); }
                }
                let rate = st.rate.max(1) as u64;
                if let Some(base) = st.base {
                    let at = |frames: u64| gst::ClockTime::from_nseconds(frames.saturating_mul(1_000_000_000) / rate);
                    b.set_pts(base + at(st.emitted));
                    b.set_duration(at(st.emitted + n) - at(st.emitted));
                }
            }
            st.emitted += n;
            Some(buf)
        }

        /// Was noch in der Kette steckt, hinausgeben (am Ende, vor einem Sprung, vor neuen Einstellungen).
        fn drain(st: &mut State) -> Option<gst::Buffer> {
            if st.chain.is_none() || st.received == 0 || st.latency == 0 { st.received = st.emitted; return None; }
            let out = Self::run(st, vec![0.0; st.latency * st.channels.max(1)]);
            st.received = st.emitted;
            out
        }

        fn restart(st: &mut State) {
            st.chain = None;
            st.base = None;
            st.emitted = 0;
            st.received = 0;
            st.skip = 0;
            st.dirty = false;
        }

        fn chain(&self, _pad: &gst::Pad, buffer: gst::Buffer) -> Result<gst::FlowSuccess, gst::FlowError> {
            let mut tail = None;
            let out = {
                let mut st = self.state.lock().unwrap();
                if st.rate == 0 || st.channels == 0 { return Err(gst::FlowError::NotNegotiated); }
                if st.chain.is_some() && (st.dirty || buffer.flags().contains(gst::BufferFlags::DISCONT)) {
                    tail = Self::drain(&mut st);
                    Self::restart(&mut st);
                }
                if st.chain.is_none() {
                    let c = Chain::new(&st.params, st.rate, st.channels);
                    st.skip = c.latency();
                    st.latency = c.latency();
                    st.chain = Some(c);
                    st.base = buffer.pts();
                    st.emitted = 0;
                    st.received = 0;
                    st.dirty = false;
                }
                let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                let data: Vec<f32> = map.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                st.received += (data.len() / st.channels.max(1)) as u64;
                Self::run(&mut st, data)
            };
            if let Some(t) = tail { self.src.push(t)?; }
            match out { Some(b) => self.src.push(b), None => Ok(gst::FlowSuccess::Ok) }
        }

        fn sink_event(&self, pad: &gst::Pad, event: gst::Event) -> bool {
            let mut tail = None;
            match event.view() {
                gst::EventView::Caps(c) => {
                    let mut st = self.state.lock().unwrap();
                    if let Ok(info) = gst_audio::AudioInfo::from_caps(c.caps()) {
                        if info.rate() != st.rate || info.channels() as usize != st.channels {
                            tail = Self::drain(&mut st);
                            Self::restart(&mut st);
                            st.rate = info.rate();
                            st.channels = info.channels() as usize;
                        }
                    }
                }
                gst::EventView::Segment(_) | gst::EventView::Eos(_) => {
                    let mut st = self.state.lock().unwrap();
                    tail = Self::drain(&mut st);
                    Self::restart(&mut st);
                }
                gst::EventView::FlushStop(_) => {
                    let mut st = self.state.lock().unwrap();
                    Self::restart(&mut st);
                }
                _ => {}
            }
            if let Some(t) = tail { let _ = self.src.push(t); }
            gst::Pad::event_default(pad, Some(&*self.obj()), event)
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WsAudio {
        const NAME: &'static str = "WsAudio";
        type Type = super::WsAudio;
        type ParentType = gst::Element;

        fn with_class(klass: &Self::Class) -> Self {
            let sink = gst::Pad::builder_from_template(&klass.pad_template("sink").unwrap())
                .chain_function(|pad, parent, buffer| WsAudio::catch_panic_pad_function(parent, || Err(gst::FlowError::Error), |imp| imp.chain(pad, buffer)))
                .event_function(|pad, parent, event| WsAudio::catch_panic_pad_function(parent, || false, |imp| imp.sink_event(pad, event)))
                .flags(gst::PadFlags::PROXY_CAPS)
                .build();
            let src = gst::Pad::builder_from_template(&klass.pad_template("src").unwrap()).flags(gst::PadFlags::PROXY_CAPS).build();
            WsAudio { sink, src, state: Mutex::new(State::default()) }
        }
    }

    impl ObjectImpl for WsAudio {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.add_pad(&self.sink).unwrap();
            obj.add_pad(&self.src).unwrap();
        }
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| vec![glib::ParamSpecString::builder("afx").build()]);
            PROPS.as_ref()
        }
        fn set_property(&self, _id: usize, value: &glib::Value, _pspec: &glib::ParamSpec) {
            let s: String = value.get::<Option<String>>().ok().flatten().unwrap_or_default();
            let mut st = self.state.lock().unwrap();
            if st.desc != s {
                st.params = Params::parse(&s);
                st.desc = s;
                st.dirty = true;
            }
        }
        fn property(&self, _id: usize, _pspec: &glib::ParamSpec) -> glib::Value { self.state.lock().unwrap().desc.to_value() }
    }

    impl GstObjectImpl for WsAudio {}

    impl ElementImpl for WsAudio {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new("WS.Video Tonbearbeitung", "Filter/Effect/Audio", "Rauschen entfernen, Klang, Lautstärke und Begrenzer", "WS.Video")
            });
            Some(&META)
        }
        fn pad_templates() -> &'static [gst::PadTemplate] {
            static TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
                let caps = gst::Caps::builder("audio/x-raw").field("format", "F32LE").field("layout", "interleaved").build();
                vec![
                    gst::PadTemplate::new("sink", gst::PadDirection::Sink, gst::PadPresence::Always, &caps).unwrap(),
                    gst::PadTemplate::new("src", gst::PadDirection::Src, gst::PadPresence::Always, &caps).unwrap(),
                ]
            });
            TEMPLATES.as_ref()
        }
        fn change_state(&self, transition: gst::StateChange) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
            let r = self.parent_change_state(transition)?;
            if transition == gst::StateChange::PausedToReady {
                let mut st = self.state.lock().unwrap();
                Self::restart(&mut st);
                st.rate = 0;
                st.channels = 0;
            }
            Ok(r)
        }
    }
}

glib::wrapper! {
    pub struct WsAudio(ObjectSubclass<imp::WsAudio>) @extends gst::Element, gst::Object;
}

pub fn register() -> Result<(), glib::BoolError> {
    gst::Element::register(None, "wsaudio", gst::Rank::NONE, WsAudio::static_type())
}

#[cfg(test)]
mod tests {
    use gst::prelude::*;

    /// Durch das Element muss genau so viel Ton hinaus wie hinein, lückenlos gestempelt und ohne Versatz.
    #[test]
    fn element_keeps_length_and_time() {
        gst::init().unwrap();
        super::register().unwrap();
        let prof = crate::audiofx::Profile { noise: vec![-90.0; crate::audiofx::BINS], floor: -60.0, loud: -20.0, peak: -6.0, steady: true };
        crate::audiofx::set_profile("elementtest", &prof);
        for afx in ["denoise=0.6;lowcut=80;compress=0.4;gain=3;limit=1;profile=elementtest", "voice=1;limit=0;profile="] {
            let desc = format!("audiotestsrc num-buffers=100 samplesperbuffer=1000 wave=sine freq=440 volume=0.3 ! audio/x-raw,format=F32LE,rate=48000,channels=2 ! wsaudio afx=\"{afx}\" ! appsink name=sink sync=false");
            let pipeline = gst::parse::launch(&desc).unwrap();
            let sink = pipeline.clone().downcast::<gst::Bin>().unwrap().by_name("sink").unwrap().downcast::<gst_app::AppSink>().unwrap();
            pipeline.set_state(gst::State::Playing).unwrap();
            let mut frames = 0u64;
            let mut next = 0u64;
            let mut all: Vec<f32> = Vec::new();
            while let Ok(sample) = sink.pull_sample() {
                let buf = sample.buffer().unwrap();
                let pts = buf.pts().unwrap().nseconds();
                assert!((pts as i64 - next as i64).abs() < 30_000, "Zeitstempel {pts} statt {next}");
                let map = buf.map_readable().unwrap();
                all.extend(map.chunks_exact(8).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
                frames += (map.len() / 8) as u64;
                next = frames * 1_000_000_000 / 48000;
            }
            pipeline.set_state(gst::State::Null).unwrap();
            assert_eq!(frames, 100_000, "{afx}");
            // Der Ton beginnt sofort (kein Vorlauf aus Stille) und der Sinus liegt an derselben Stelle wie im Eingang
            let early = all[2400..4800].iter().map(|v| v * v).sum::<f32>() / 2400.0;
            assert!(early > 0.005, "Anfang zu leise: {early} ({afx})");
            let cross = (48000..49000).find(|&i| all[i - 1] < 0.0 && all[i] >= 0.0).unwrap();
            // 440 Hz: Nulldurchgänge alle 109,09 Samples
            let phase = (cross as f64) % (48000.0 / 440.0);
            let off = phase.min(48000.0 / 440.0 - phase);
            if afx.starts_with("denoise") { assert!(off < 12.0, "Versatz {off} Samples ({afx})"); }
        }
    }
}
