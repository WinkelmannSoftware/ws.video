//! Mausklicks über evdev mitlesen (nur Maustasten; braucht Leserechte auf /dev/input, Gruppe „input“).
use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

const EV_KEY: u16 = 1;
const BTN_LEFT: u16 = 0x110;
const BTN_MIDDLE: u16 = 0x112;

fn has_mouse_buttons(f: &File) -> bool {
    // EVIOCGBIT(EV_KEY, 96)
    let mut bits = [0u8; 96];
    let req: libc::c_ulong = (2u64 << 30 | (96u64 << 16) | ((b'E' as u64) << 8) | (0x20 + EV_KEY as u64)) as libc::c_ulong;
    let r = unsafe { libc::ioctl(f.as_raw_fd(), req, bits.as_mut_ptr()) };
    if r < 0 { return false; }
    let bit = BTN_LEFT as usize;
    bits[bit / 8] & (1 << (bit % 8)) != 0
}

/// Öffnet alle lesbaren Eingabegeräte mit Maustasten.
pub fn open_mice() -> Vec<File> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/dev/input") else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("event") { continue; }
        if let Ok(f) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(e.path()) {
            if has_mouse_buttons(&f) { out.push(f); }
        }
    }
    out
}

/// Wartet bis zu `timeout_ms` auf Ereignisse und liefert gedrückte Maustasten (0 links, 1 rechts, 2 Mitte).
pub fn poll_clicks(files: &[File], timeout_ms: i32) -> Vec<u8> {
    let mut clicks = Vec::new();
    if files.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
        return clicks;
    }
    let mut fds: Vec<libc::pollfd> = files.iter().map(|f| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
    let r = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
    if r <= 0 { return clicks; }
    for (i, p) in fds.iter().enumerate() {
        if p.revents & libc::POLLIN == 0 { continue; }
        let mut buf = [0u8; 24 * 32];
        loop {
            let n = unsafe { libc::read(files[i].as_raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 { break; }
            for ev in buf[..n as usize].chunks_exact(24) {
                let typ = u16::from_ne_bytes([ev[16], ev[17]]);
                let code = u16::from_ne_bytes([ev[18], ev[19]]);
                let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                if typ == EV_KEY && (BTN_LEFT..=BTN_MIDDLE).contains(&code) && value == 1 {
                    clicks.push((code - BTN_LEFT) as u8);
                }
            }
        }
    }
    clicks
}
