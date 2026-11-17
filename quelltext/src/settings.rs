//! Einstellungen (Aufnahme-Vorgaben, zuletzt geöffnete Projekte) als JSON in ~/.config/ws-video.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub recent: Vec<String>,
    pub rec_screen: bool,
    pub rec_camera: bool,
    pub rec_mic: bool,
    pub rec_system: bool,
    pub camera_device: String,
    pub mic_device: String,
    pub system_device: String,
    pub rec_fps: u32,
    pub rec_hardware: bool,
    /// Größe der Bildschirmaufnahme: "auto", "original", "1440" oder "1080"
    pub rec_size: String,
    pub rec_cursor_data: bool,
    pub restore_token: String,
    /// Aktivem Bildschirm folgen: zweite Quelle und die Monitore beider Quellen (Namen aus Hyprland, leer = automatisch)
    pub rec_follow: bool,
    pub restore_token2: String,
    pub rec_monitor1: String,
    pub rec_monitor2: String,
    pub export_dir: String,
    pub export_hardware: bool,
    pub export_codec: u32,
    pub export_quality: u32,
    /// Kamera-Hintergrund für neue Aufnahmen (0 Original, 1 entfernen, 2 unscharf, 3 Bild)
    pub cam_bg_mode: u32,
    pub cam_bg_image: String,
    /// AppImage-Datei, für die schon nach dem Eintrag ins Startmenü gefragt wurde
    pub menu_asked: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            recent: Vec::new(),
            rec_screen: true,
            rec_camera: false,
            rec_mic: true,
            rec_system: false,
            camera_device: String::new(),
            mic_device: String::new(),
            system_device: String::new(),
            rec_fps: 30,
            rec_hardware: true,
            rec_size: "auto".to_string(),
            rec_cursor_data: true,
            restore_token: String::new(),
            rec_follow: false,
            restore_token2: String::new(),
            rec_monitor1: String::new(),
            rec_monitor2: String::new(),
            export_dir: String::new(),
            export_hardware: true,
            export_codec: 0,
            export_quality: 1,
            cam_bg_mode: 0,
            cam_bg_image: String::new(),
            menu_asked: String::new(),
        }
    }
}

fn path() -> PathBuf {
    let d = gtk::glib::user_config_dir().join("ws-video");
    let _ = std::fs::create_dir_all(&d);
    d.join("einstellungen.json")
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) { let _ = std::fs::write(path(), s); }
    }
    pub fn add_recent(&mut self, p: &str) {
        self.recent.retain(|x| x != p);
        self.recent.insert(0, p.to_string());
        self.recent.truncate(12);
        self.save();
    }
    pub fn recent_existing(&self) -> Vec<String> {
        self.recent.iter().filter(|p| std::path::Path::new(p).exists()).cloned().collect()
    }
}

pub fn videos_dir() -> PathBuf {
    let base = gtk::glib::user_special_dir(gtk::glib::UserDirectory::Videos).unwrap_or_else(|| gtk::glib::home_dir().join("Videos"));
    let d = base.join("WS.Video");
    let _ = std::fs::create_dir_all(&d);
    d
}
