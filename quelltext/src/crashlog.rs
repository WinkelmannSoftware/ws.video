//! Fehlerprotokoll. Alle Meldungen des Programms (auch die von GTK und GStreamer) landen in einer Datei,
//! und bei einem Absturz steht dort, in welchem Thread und an welcher Stelle er passiert ist.
//! Die Dateien liegen in `~/.local/state/ws-video/`; `ws-video --fehlerbericht` fasst sie in einer Datei zusammen.

use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::PathBuf;

static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const END_MARK: &str = "== beendet";
const CRASH_MARK: &str = "== Absturz";

pub fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    base.join("ws-video")
}
fn current() -> PathBuf { dir().join("protokoll.log") }
fn previous() -> PathBuf { dir().join("protokoll-vorher.log") }

fn now() -> String {
    gtk::glib::DateTime::now_local().ok().and_then(|d| d.format("%Y-%m-%d %H:%M:%S").ok()).map(|s| s.to_string()).unwrap_or_default()
}

/// Eine Zeile mit Uhrzeit ins Protokoll: die Spur dessen, was das Programm zuletzt getan hat.
pub fn note(msg: &str) {
    if !ACTIVE.load(std::sync::atomic::Ordering::Relaxed) { return; }
    let t = gtk::glib::DateTime::now_local().ok().and_then(|d| d.format("%H:%M:%S").ok()).map(|s| s.to_string()).unwrap_or_default();
    eprintln!("[{t}] {msg}");
}

fn crash_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir()).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("absturz-")).unwrap_or(false)).collect();
    v.sort();
    v
}

/// Protokoll einrichten. Nur im laufenden Hauptprogramm aufrufen, nicht bei `--stop` und anderen Kurzaufrufen.
/// Liefert die Datei des letzten Absturzes, wenn das Programm beim vorigen Mal nicht sauber beendet wurde.
pub fn install() -> Option<PathBuf> {
    hooks();
    if std::env::var("WS_LOG").map(|v| v == "0").unwrap_or(false) { return None; }
    let d = dir();
    if std::fs::create_dir_all(&d).is_err() { return None; }
    let mut crashed = None;
    if let Ok(old) = std::fs::read_to_string(current()) {
        // Bis 0.8.0 fehlte der Vermerk des ordentlichen Endes immer. Ein solches Protokoll ohne Absturzbericht
        // ist deshalb kein Hinweis auf einen Absturz.
        let old_bug = !old.contains(CRASH_MARK) && old.lines().next().and_then(|l| l.strip_prefix("WS.Video ")).and_then(|l| l.split_whitespace().next())
            .map(|v| { let n: Vec<u32> = v.split('.').filter_map(|x| x.parse().ok()).collect(); n.len() == 3 && (n[0], n[1], n[2]) < (0, 8, 1) }).unwrap_or(false);
        if (!old.contains(END_MARK) || old.contains(CRASH_MARK)) && !old_bug {
            let stamp = std::fs::metadata(current()).ok().and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|t| gtk::glib::DateTime::from_unix_local(t.as_secs() as i64).ok())
                .and_then(|t| t.format("%Y-%m-%d_%H-%M-%S").ok()).map(|s| s.to_string()).unwrap_or_else(|| "unbekannt".into());
            let to = d.join(format!("absturz-{stamp}.log"));
            if std::fs::copy(current(), &to).is_ok() { crashed = Some(to); }
            let files = crash_files();
            for f in files.iter().take(files.len().saturating_sub(5)) { let _ = std::fs::remove_file(f); }
        }
        let _ = std::fs::rename(current(), previous());
    }
    let Ok(f) = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(current()) else { return crashed };
    // Standard-Fehlerausgabe auf die Datei legen: So landen auch die Meldungen der Bibliotheken dort,
    // und zwar sofort, ohne Puffer, der bei einem Absturz verloren ginge.
    unsafe {
        use std::io::IsTerminal;
        if std::io::stderr().is_terminal() { eprintln!("ws-video: Meldungen stehen in {}", current().display()); }
        libc::dup2(f.as_raw_fd(), 2);
    }
    ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    let (maj, min, mic, _) = gst::version();
    eprintln!("WS.Video {} · gestartet {} · GStreamer {maj}.{min}.{mic} · GTK {}.{}.{}", crate::VERSION, now(), gtk::major_version(), gtk::minor_version(), gtk::micro_version());
    eprintln!("Sitzung: {} · {} Kerne", std::env::var("XDG_SESSION_TYPE").unwrap_or_default(), std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0));
    crashed
}

/// Sauberes Ende vermerken.
pub fn finish() {
    // nur im Hauptprogramm, das das Protokoll auch führt (nicht bei Kurzaufrufen wie `--stop`)
    if !ACTIVE.load(std::sync::atomic::Ordering::Relaxed) { return; }
    eprintln!("{END_MARK} {}", now());
    let _ = std::io::stderr().flush();
}

extern "C" {
    fn backtrace(buf: *mut *mut libc::c_void, size: libc::c_int) -> libc::c_int;
    fn backtrace_symbols_fd(buf: *const *mut libc::c_void, size: libc::c_int, fd: libc::c_int);
}

fn raw(s: &[u8]) { unsafe { libc::write(2, s.as_ptr() as *const libc::c_void, s.len()); } }

extern "C" fn on_signal(sig: libc::c_int) {
    // Hier ist nur erlaubt, was ohne Speicheranforderung auskommt.
    raw(b"\n== Absturz: Signal ");
    raw(match sig {
        libc::SIGSEGV => b"SIGSEGV (ungueltiger Speicherzugriff)",
        libc::SIGABRT => b"SIGABRT (Abbruch nach einem schweren Fehler)",
        libc::SIGBUS => b"SIGBUS",
        libc::SIGFPE => b"SIGFPE (Rechenfehler)",
        libc::SIGILL => b"SIGILL",
        _ => b"?",
    });
    raw(b" im Thread ");
    let mut name = [0u8; 17];
    unsafe { libc::prctl(libc::PR_GET_NAME, name.as_mut_ptr()); }
    let len = name.iter().position(|b| *b == 0).unwrap_or(16);
    raw(&name[..len]);
    raw(b"\nAufrufkette:\n");
    let mut frames = [std::ptr::null_mut::<libc::c_void>(); 64];
    unsafe {
        let n = backtrace(frames.as_mut_ptr(), 64);
        backtrace_symbols_fd(frames.as_ptr(), n, 2);
    }
    raw(b"== Ende des Absturzberichts\n");
    // Der Handler ist nur einmal aktiv (SA_RESETHAND): Nach der Rückkehr tritt der Fehler erneut auf,
    // und das System legt seinen eigenen Absturzbericht (coredumpctl) wie gewohnt an.
}

static BEAT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static STALL_FRAMES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(48);

/// Vom Wächter ausgelöst: Der Thread schreibt auf, wo er gerade steht. Ändert nichts am Ablauf.
extern "C" fn on_stall(_sig: libc::c_int) {
    raw(b"-- Thread ");
    let mut name = [0u8; 17];
    unsafe { libc::prctl(libc::PR_GET_NAME, name.as_mut_ptr()); }
    let len = name.iter().position(|b| *b == 0).unwrap_or(16);
    raw(&name[..len]);
    raw(b"\n");
    let mut frames = [std::ptr::null_mut::<libc::c_void>(); 48];
    let max = STALL_FRAMES.load(std::sync::atomic::Ordering::Relaxed).clamp(1, 48);
    unsafe {
        let n = backtrace(frames.as_mut_ptr(), max);
        backtrace_symbols_fd(frames.as_ptr(), n, 2);
    }
}

/// Wächter gegen das Einfrieren. Aus dem Hauptthread aufrufen, sobald die Oberfläche läuft.
///
/// Reagiert die Oberfläche 15 Sekunden lang nicht, schreibt jeder Thread seine Aufrufkette ins Protokoll.
/// So lässt sich nachher sehen, wer auf wen gewartet hat. Das Programm wird dabei nicht beendet.
pub fn watch() {
    use std::sync::atomic::Ordering::Relaxed;
    if !ACTIVE.load(Relaxed) || std::env::var_os("WS_NO_WATCH").is_some() { return; }
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_stall as extern "C" fn(libc::c_int) as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigaction(libc::SIGUSR1, &sa, std::ptr::null_mut());
    }
    let main_tid = unsafe { libc::syscall(libc::SYS_gettid) } as libc::pid_t;
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), || {
        BEAT.fetch_add(1, Relaxed);
        gtk::glib::ControlFlow::Continue
    });
    // Prüfhilfe: WS_TEST_STALL=Sekunden hält die Oberfläche kurz nach dem Start so lange an
    if let Some(secs) = std::env::var("WS_TEST_STALL").ok().and_then(|v| v.parse::<u64>().ok()) {
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || std::thread::sleep(std::time::Duration::from_secs(secs)));
    }
    let _ = std::thread::Builder::new().name("ws-waechter".into()).spawn(move || {
        let pid = unsafe { libc::getpid() };
        let me = unsafe { libc::syscall(libc::SYS_gettid) } as libc::pid_t;
        let (mut last, mut still, mut told) = (0u64, 0u32, false);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let b = BEAT.load(Relaxed);
            if b != last {
                if told { note("Die Oberfläche reagiert wieder."); }
                last = b; still = 0; told = false;
                continue;
            }
            still += 1;
            if still != 15 || told { continue; }
            told = true;
            note("== Die Oberfläche reagiert seit 15 Sekunden nicht. Aufrufketten aller Threads, der Hauptthread zuerst:");
            let kill = |tid: libc::pid_t, frames: i32| {
                STALL_FRAMES.store(frames, Relaxed);
                unsafe { libc::syscall(libc::SYS_tgkill, pid, tid, libc::SIGUSR1); }
                std::thread::sleep(std::time::Duration::from_millis(40));
            };
            kill(main_tid, 48);
            let tids: Vec<libc::pid_t> = std::fs::read_dir("/proc/self/task").map(|d| d.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect()).unwrap_or_default();
            for tid in tids { if tid != main_tid && tid != me { kill(tid, 14); } }
            note("== Ende der Aufrufketten");
        }
    });
}

/// Abmelden, Herunterfahren oder Strg+C sind kein Absturz: im Protokoll als ordentliches Ende vermerken.
extern "C" fn on_quit(sig: libc::c_int) {
    raw(b"== beendet durch ein Signal des Systems\n");
    unsafe { libc::raise(sig); }
}

fn hooks() {
    std::panic::set_hook(Box::new(|info| {
        let t = std::thread::current();
        eprintln!("\n{CRASH_MARK}: Programmfehler im Thread {}: {info}", t.name().unwrap_or("?"));
        eprintln!("Aufrufkette:\n{}", std::backtrace::Backtrace::force_capture());
        eprintln!("== Ende des Absturzberichts");
    }));
    unsafe {
        // eigener Stapel für den Handler, damit er auch nach einem Stapelüberlauf noch schreiben kann
        const SIZE: usize = 64 * 1024;
        let stack = Box::leak(vec![0u8; SIZE].into_boxed_slice());
        let ss = libc::stack_t { ss_sp: stack.as_mut_ptr() as *mut libc::c_void, ss_flags: 0, ss_size: SIZE };
        libc::sigaltstack(&ss, std::ptr::null_mut());
        for sig in [libc::SIGSEGV, libc::SIGABRT, libc::SIGBUS, libc::SIGFPE, libc::SIGILL] {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
            sa.sa_flags = libc::SA_RESETHAND | libc::SA_ONSTACK;
            libc::sigemptyset(&mut sa.sa_mask);
            libc::sigaction(sig, &sa, std::ptr::null_mut());
        }
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_quit as extern "C" fn(libc::c_int) as usize;
            sa.sa_flags = libc::SA_RESETHAND;
            libc::sigemptyset(&mut sa.sa_mask);
            libc::sigaction(sig, &sa, std::ptr::null_mut());
        }
    }
}

/// Alles für eine Fehlermeldung in einer Datei sammeln. Liefert den Pfad der Datei.
pub fn report() -> anyhow::Result<PathBuf> {
    let out = crate::bericht::downloads().join(crate::bericht::NAME);
    let mut s = format!("WS.Video-Fehlerbericht · Format 1\nVersion: {}\nErstellt: {}\n\nHinweis: {}\n", crate::VERSION, now(), crate::bericht::NOTICE);
    s.push_str(&crate::bericht::system());
    let mut files = vec![current(), previous()];
    files.extend(crash_files().into_iter().rev().take(3));
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        // sehr lange Protokolle: Anfang und Ende genügen
        let lines: Vec<&str> = text.lines().collect();
        s.push_str(&format!("\n\n######## {} ({} Zeilen)\n", f.display(), lines.len()));
        if lines.len() > 700 {
            s.push_str(&lines[..100].join("\n"));
            s.push_str("\n[…]\n");
            s.push_str(&lines[lines.len() - 600..].join("\n"));
        } else {
            s.push_str(&text);
        }
    }
    for (title, cmd) in [
        ("Absturzbericht des Systems (coredumpctl)", "coredumpctl info -1 ws-video --no-pager 2>&1 | head -n 260"),
        ("Meldungen des Kerns", "journalctl -b -k --no-pager 2>/dev/null | grep -i -E 'ws-video|segfault|out of memory|oom-kill' | tail -n 30"),
    ] {
        s.push_str(&format!("\n\n######## {title}\n"));
        match std::process::Command::new("sh").arg("-c").arg(cmd).output() {
            Ok(o) => s.push_str(&String::from_utf8_lossy(&o.stdout)),
            Err(e) => s.push_str(&format!("nicht verfügbar: {e}\n")),
        }
    }
    std::fs::write(&out, crate::bericht::anonymize(&s))?;
    Ok(out)
}
