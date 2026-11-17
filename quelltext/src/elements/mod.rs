//! Eigene GStreamer-Elemente von WS.Video (werden im Prozess registriert).
pub mod audio;
pub mod blur;
pub mod cursor;
pub mod fx;
pub mod image;
pub mod person;
pub mod roll;
pub mod shape;

pub fn register() -> Result<(), gst::glib::BoolError> {
    shape::register()?;
    cursor::register()?;
    blur::register()?;
    image::register()?;
    person::register()?;
    roll::register()?;
    fx::register()?;
    audio::register()?;
    Ok(())
}
