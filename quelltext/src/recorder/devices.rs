//! Aufnahmegeräte: Kameras (V4L2), Mikrofone und Ausgabegeräte (PulseAudio/PipeWire).
use gst::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Dev {
    pub id: String,
    pub name: String,
    pub default: bool,
}

pub fn cameras() -> Vec<Dev> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/class/video4linux") else { return out };
    let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("video")).collect();
    names.sort_by_key(|n| n.trim_start_matches("video").parse::<u32>().unwrap_or(999));
    for n in names {
        let base = format!("/sys/class/video4linux/{n}");
        // nur der Hauptknoten eines Geräts nimmt Bilder auf (Index 0)
        let index = std::fs::read_to_string(format!("{base}/index")).unwrap_or_default();
        if index.trim() != "0" { continue; }
        let name = std::fs::read_to_string(format!("{base}/name")).unwrap_or_else(|_| n.clone());
        out.push(Dev { id: format!("/dev/{n}"), name: name.trim().to_string(), default: out.is_empty() });
    }
    out
}

/// Ausgabe von `pactl list sources|sinks` lesen: (Name, Beschreibung, ist Monitor)
fn parse_pactl(text: &str) -> Vec<(String, String, bool)> {
    let mut out: Vec<(String, String, bool)> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if let Some(n) = l.strip_prefix("Name: ") {
            out.push((n.trim().to_string(), String::new(), n.trim().ends_with(".monitor")));
        } else if let Some(d) = l.strip_prefix("Description: ").or_else(|| l.strip_prefix("Beschreibung: ")) {
            if let Some(last) = out.last_mut() { if last.1.is_empty() { last.1 = d.trim().to_string(); } }
        } else if l.starts_with("device.class = \"monitor\"") {
            if let Some(last) = out.last_mut() { last.2 = true; }
        }
    }
    out
}

fn pactl(kind: &str) -> Vec<(String, String, bool)> {
    let out = std::process::Command::new("pactl").env("LC_ALL", "C").args(["list", kind]).output();
    match out { Ok(o) if o.status.success() => parse_pactl(&String::from_utf8_lossy(&o.stdout)), _ => Vec::new() }
}

/// (Mikrofone, Ausgabegeräte als Monitor-Quelle für den Systemton)
/// Drei Wege, weil je nach System nur einer alle Geräte liefert: `pactl`, die Geräteliste von GStreamer
/// über PulseAudio und die über PipeWire (dort steht der Gerätename in `node.name`).
pub fn audio_devices() -> (Vec<Dev>, Vec<Dev>) {
    let mut inputs = vec![Dev { id: String::new(), name: "Standard-Mikrofon".into(), default: true }];
    let mut outputs = vec![Dev { id: "@DEFAULT_MONITOR@".into(), name: "Standard-Ausgabe".into(), default: true }];
    let mut add_in = |inputs: &mut Vec<Dev>, id: String, name: String| {
        if id.is_empty() || id.ends_with(".monitor") || inputs.iter().any(|x| x.id == id) { return; }
        inputs.push(Dev { name: if name.is_empty() { id.clone() } else { name }, id, default: false });
    };
    let mut add_out = |outputs: &mut Vec<Dev>, sink: String, name: String| {
        let id = format!("{sink}.monitor");
        if sink.is_empty() || outputs.iter().any(|x| x.id == id) { return; }
        outputs.push(Dev { name: if name.is_empty() { sink.clone() } else { name }, id, default: false });
    };
    for (n, d, monitor) in pactl("sources") { if !monitor { add_in(&mut inputs, n, d); } }
    for (n, d, _) in pactl("sinks") { add_out(&mut outputs, n, d); }

    let mon = gst::DeviceMonitor::new();
    mon.add_filter(Some("Audio/Source"), None);
    mon.add_filter(Some("Audio/Sink"), None);
    // auch Geräte von Anbietern zeigen, die ein anderer Anbieter (PipeWire) sonst verdeckt
    mon.set_show_all_devices(true);
    if mon.start().is_err() { return (inputs, outputs); }
    for d in mon.devices() {
        let class = d.device_class().to_string();
        let props = d.properties();
        let prop = |k: &str| props.as_ref().and_then(|p| p.get::<String>(k).ok());
        let Ok(el) = d.create_element(None) else { continue };
        let factory = el.factory().map(|f| f.name().to_string()).unwrap_or_default();
        let dev: Option<String> = match factory.as_str() {
            "pulsesrc" | "pulsesink" => el.property("device"),
            // PipeWire: Der Knotenname ist zugleich der Gerätename für pulsesrc
            "pipewiresrc" | "pipewiresink" => prop("node.name"),
            _ => None,
        };
        let Some(dev) = dev else { continue };
        let is_monitor = prop("device.class").map(|c| c == "monitor").unwrap_or(false) || dev.ends_with(".monitor");
        let name = prop("node.description").unwrap_or_else(|| d.display_name().to_string());
        if class.contains("Source") {
            if is_monitor || class.contains("Video") { continue; }
            add_in(&mut inputs, dev, name);
        } else if class.contains("Sink") {
            add_out(&mut outputs, dev, name);
        }
    }
    mon.stop();
    (inputs, outputs)
}

#[cfg(test)]
mod tests {
    #[test]
    fn pactl_lesen() {
        let t = "Source #55\n\tState: SUSPENDED\n\tName: alsa_input.usb-RODE_NT-USB-00.analog-stereo\n\tDescription: RODE NT-USB Analog Stereo\n\tProperties:\n\t\tdevice.class = \"sound\"\nSource #56\n\tName: alsa_output.pci.analog-stereo.monitor\n\tDescription: Monitor of Built-in\n\tProperties:\n\t\tdevice.class = \"monitor\"\n";
        let v = super::parse_pactl(t);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], ("alsa_input.usb-RODE_NT-USB-00.analog-stereo".to_string(), "RODE NT-USB Analog Stereo".to_string(), false));
        assert!(v[1].2);
    }
}

/// Bestes Kameraformat wählen: möglichst 720p bis 1080p mit mindestens 24 Bildern je Sekunde.
/// Liefert den Caps-Text und ob es MJPEG ist.
pub fn camera_format(device: &str) -> Option<(String, bool)> {
    let src = gst::ElementFactory::make("v4l2src").property("device", device).build().ok()?;
    src.set_state(gst::State::Ready).ok()?;
    let caps = src.static_pad("src")?.query_caps(None);
    let _ = src.set_state(gst::State::Null);
    let mut best: Option<(i64, String, bool)> = None;
    for s in caps.iter() {
        let raw = s.name() == "video/x-raw";
        let jpeg = s.name() == "image/jpeg";
        if !raw && !jpeg { continue; }
        let (Ok(w), Ok(h)) = (s.get::<i32>("width"), s.get::<i32>("height")) else { continue };
        let mut rates: Vec<gst::Fraction> = Vec::new();
        if let Ok(f) = s.get::<gst::Fraction>("framerate") {
            rates.push(f);
        } else if let Ok(l) = s.get::<gst::List>("framerate") {
            for v in l.iter() { if let Ok(f) = v.get::<gst::Fraction>() { rates.push(f); } }
        } else if let Ok(r) = s.get::<gst::FractionRange>("framerate") {
            rates.push(r.max());
        }
        for f in rates {
            if f.denom() == 0 { continue; }
            let fps = f.numer() as f64 / f.denom() as f64;
            if fps > 61.0 { continue; }
            let mut score: i64 = 0;
            score += if fps >= 24.0 { 100_000 } else { (fps * 1000.0) as i64 };
            score += if h <= 1080 { h as i64 * 10 } else { 5000 - (h as i64 - 1080) };
            score += if raw { 5 } else { 0 };
            score -= ((fps - 30.0).abs() * 2.0) as i64;
            let text = format!("{},width={},height={},framerate={}/{}", s.name(), w, h, f.numer(), f.denom());
            if best.as_ref().map(|b| score > b.0).unwrap_or(true) { best = Some((score, text, jpeg)); }
        }
    }
    best.map(|b| (b.1, b.2))
}
