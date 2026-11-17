#!/usr/bin/env python3
"""WS.Video als AppImage packen. Aufruf im Quelltext-Ordner nach `cargo build --release`:
    python3 paket/appimage/bauen.py [zielordner]
Braucht: patchelf, squashfs-tools, die GStreamer-Plugins des Bausystems (gebaut und geprüft auf Ubuntu 24.04).
Grundsatz: GTK, GStreamer, Kodierer und PipeWire kommen ins Paket. Bibliotheken, die eng am System hängen
(Grafik, Fenster, Schrift, Ton, Grundbibliotheken), nimmt das Programm vom System; sie liegen nur als Reserve bei.
"""
import fnmatch, os, re, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.dirname(os.path.dirname(HERE))
OUT = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.join(SRC, "target", "appimage")
MULTI = "/usr/lib/x86_64-linux-gnu"
VERSION = re.search(r'^version = "(.+?)"', open(os.path.join(SRC, "Cargo.toml")).read(), re.M).group(1)

PLUGINS = """coreelements typefindfunctions app playback autodetect audioconvert audioresample audiorate audiomixer
audiotestsrc videotestsrc videoconvertscale videorate videofilter videocrop videobox compositor volume level audiofx
audiofxbad audioparsers videoparsersbad rawparse pbtypes encoding gio alpha alphacolor deinterlace imagefreeze multifile
pango overlaycomposition gdkpixbuf png jpeg jpegformat webp rsvg cairo interleave adder equalizer replaygain shapewipe
smpte geometrictransform debug debugutilsbad autoconvert cutter subparse soundtouch nle
matroska isomp4 avi flv ogg mpegtsdemux mpegtsmux mpegpsdemux asf wavparse wavenc aiff auparse id3demux id3tag apetag
icydemux mxf libav x264 x265 openh264 svtav1 aom vpx opus opusparse vorbis theora flac lame mpg123 voaacenc faad wavpack
va nvcodec qsv pipewire pulseaudio video4linux2 ximagesrc""".split()

# Kommt immer vom System (hängt am Kern oder am Grafiktreiber)
NEVER = "ld-linux* libc.so* libm.so* libdl.so* libpthread.so* librt.so* libresolv.so* libutil.so* libnss* libstdc++* libgcc_s* libnvidia* libcuda* linux-vdso*".split()
# Kommt vom System, wenn es dort vorhanden ist; sonst aus dem Paket (Ordner reserve)
RESERVE = """libGL* libEGL* libGLES* libOpenGL* libdrm* libgbm* libvulkan* libX* libxcb* libxkb* libwayland-* libva* libvdpau* libasound* libfontconfig* libfreetype* libharfbuzz*
libfribidi* libgraphite2* libexpat* libz.so* libzstd* liblzma* libbz2* libffi* libelf* libxml2* libpcre2* libcap.so*
libselinux* libmount* libblkid* libuuid* libudev* libsystemd* libdbus-1* libgcrypt* libgpg-error* liblz4* libbrotli*
libpng16* libgnutls* libnettle* libhogweed* libgmp* libtasn1* libp11-kit* libidn2* libunistring* libssl* libcrypto*
libcom_err* libkrb5* libgssapi* libk5crypto* libkeyutils* libnuma* libOpenCL* libICE* libSM* libusb* libbsd* libmd.so*
libatomic* libgomp* libquadmath* libgfortran* libcrypt.so* libnsl* libtirpc* libsasl2* libldap* liblber* libcurl*
libsqlite3* libpsl* libnghttp2* libssh* librtmp* libdw* libunwind* libapparmor* libaudit* libcap-ng*""".split()

def match(name, pats): return any(fnmatch.fnmatch(name, p) for p in pats)

def run(*a, **k): return subprocess.run(a, check=True, text=True, capture_output=True, **k).stdout

def deps(path):
    out = {}
    for line in subprocess.run(["ldd", path], text=True, capture_output=True).stdout.splitlines():
        m = re.match(r"\s*(\S+) => (\S+) \(", line)
        if m: out[m.group(1)] = m.group(2)
    return out

def rpath(path, value):
    subprocess.run(["patchelf", "--set-rpath", value, path], check=True)

def main():
    app = os.path.join(OUT, "WS.Video.AppDir")
    shutil.rmtree(app, ignore_errors=True)
    lib = os.path.join(app, "usr/lib")
    for d in ["usr/bin", "usr/libexec", "usr/lib/gstreamer-1.0", "usr/lib/reserve", "usr/lib/gio-modules", "usr/lib/gdk-pixbuf-loaders",
              "usr/share/glib-2.0/schemas", "usr/share/fonts", "usr/share/licenses", "usr/share/applications", "usr/share/icons/hicolor/scalable/apps"]:
        os.makedirs(os.path.join(app, d))
    roots = []   # (Datei im Paket, Suchpfad zu usr/lib)

    def put(src, rel, origin):
        dst = os.path.join(app, rel)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copy2(os.path.realpath(src), dst)
        os.chmod(dst, 0o755)
        roots.append((src, dst, origin))

    put(os.path.join(SRC, "target/release/ws-video"), "usr/bin/ws-video", "$ORIGIN/../lib")
    put(f"{MULTI}/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner", "usr/libexec/gst-plugin-scanner", "$ORIGIN/../lib")
    missing = []
    for p in PLUGINS:
        f = f"{MULTI}/gstreamer-1.0/libgst{p}.so"
        if os.path.exists(f): put(f, f"usr/lib/gstreamer-1.0/libgst{p}.so", "$ORIGIN/..")
        else: missing.append(p)
    if missing: sys.exit("Diese GStreamer-Plugins fehlen auf dem Bausystem: " + " ".join(missing))
    for base, dirs, files in os.walk(f"{MULTI}/pipewire-0.3"):
        for f in files:
            if f.endswith(".so"):
                rel = os.path.relpath(os.path.join(base, f), MULTI)
                put(os.path.join(base, f), "usr/lib/" + rel, "$ORIGIN/" + "/".join([".."] * rel.count("/")))
    for base, dirs, files in os.walk(f"{MULTI}/spa-0.2"):
        # nur was ein Programm als Gegenstelle braucht; Gerätetreiber (ALSA, Bluetooth, Kameras) laufen im PipeWire-Dienst des Systems
        if not any(x in base for x in ("/support", "/videoconvert", "/audioconvert")): continue
        for f in files:
            if f.endswith(".so"):
                rel = os.path.relpath(os.path.join(base, f), MULTI)
                put(os.path.join(base, f), "usr/lib/" + rel, "$ORIGIN/" + "/".join([".."] * rel.count("/")))
    loaders = f"{MULTI}/gdk-pixbuf-2.0/2.10.0/loaders"
    for n in ["svg", "gif", "bmp", "tiff", "ico"]:
        put(f"{loaders}/libpixbufloader-{n}.so", f"usr/lib/gdk-pixbuf-loaders/libpixbufloader-{n}.so", "$ORIGIN/..")

    # Abhängigkeiten einsammeln
    need = {}
    for src, dst, origin in roots: need.update(deps(src))
    bundled, reserve, skipped = [], [], []
    for name, path in sorted(need.items()):
        if match(name, NEVER): skipped.append(name); continue
        if name.startswith("libpython"):
            # Python wird nicht mitgeliefert, siehe pystub.c
            dst = os.path.join(lib, name)
            run("gcc", "-shared", "-fPIC", "-O1", f"-Wl,-soname,{name}", "-o", dst, os.path.join(HERE, "pystub.c"))
            have = set(l.split()[-1] for l in run("nm", "-D", "--defined-only", dst).splitlines())
            real = set(l.split()[-1] for l in run("nm", "-D", "--defined-only", path).splitlines())
            for user in [p for n, p in need.items() if n.startswith("libges")]:
                want = set(l.split()[-1] for l in run("nm", "-D", "--undefined-only", user).splitlines()) & real
                if want - have: sys.exit("pystub.c fehlen Funktionen: " + " ".join(sorted(want - have)))
            continue
        if match(name, RESERVE):
            d = os.path.join(lib, "reserve", name)
            os.makedirs(d)
            dst = os.path.join(d, name)
            shutil.copy2(os.path.realpath(path), dst); os.chmod(dst, 0o755)
            rpath(dst, "$ORIGIN/../..")
            reserve.append((name, path))
        else:
            dst = os.path.join(lib, name)
            shutil.copy2(os.path.realpath(path), dst); os.chmod(dst, 0o755)
            rpath(dst, "$ORIGIN")
            bundled.append((name, path))
    for src, dst, origin in roots: rpath(dst, origin)

    # Bildlader-Liste mit Platzhalter für den Ort im Paket
    q = run(f"{MULTI}/gdk-pixbuf-2.0/gdk-pixbuf-query-loaders", *[os.path.join(lib, "gdk-pixbuf-loaders", f) for f in sorted(os.listdir(os.path.join(lib, "gdk-pixbuf-loaders")))])
    open(os.path.join(lib, "gdk-pixbuf-loaders/loaders.cache.in"), "w").write(q.replace(lib, "@L@"))
    # Einstellungsschemata von GTK
    sch = os.path.join(app, "usr/share/glib-2.0/schemas")
    for f in os.listdir("/usr/share/glib-2.0/schemas"):
        if f.startswith("org.gtk.gtk4.") and f.endswith(".xml"): shutil.copy2(os.path.join("/usr/share/glib-2.0/schemas", f), sch)
    run("glib-compile-schemas", sch)
    # Schrift, Symbol, Starter
    for f in os.listdir(os.path.join(HERE, "schrift")): shutil.copy2(os.path.join(HERE, "schrift", f), os.path.join(app, "usr/share/fonts", "Inter-LICENSE.txt" if f == "LICENSE.txt" else f))
    icon = os.path.join(SRC, "paket/ws-video.svg")
    shutil.copy2(icon, os.path.join(app, "ws-video.svg"))
    shutil.copy2(icon, os.path.join(app, "usr/share/icons/hicolor/scalable/apps/ws-video.svg"))
    os.symlink("ws-video.svg", os.path.join(app, ".DirIcon"))
    desktop = "[Desktop Entry]\nType=Application\nName=WS.Video\nComment=Bildschirm aufnehmen und Videos bearbeiten\nExec=ws-video %F\nIcon=ws-video\nTerminal=false\nCategories=AudioVideo;Video;Recorder;\nMimeType=application/x-wsvideo;\nStartupWMClass=app.wsvideo.WSVideo\nX-AppImage-Version=" + VERSION + "\n"
    open(os.path.join(app, "app.wsvideo.WSVideo.desktop"), "w").write(desktop)
    open(os.path.join(app, "usr/share/applications/app.wsvideo.WSVideo.desktop"), "w").write(desktop)
    shutil.copy2(os.path.join(HERE, "AppRun"), os.path.join(app, "AppRun")); os.chmod(os.path.join(app, "AppRun"), 0o755)

    # Lizenzen: Texte aller Pakete, aus denen etwas mitgeliefert wird, und die Liste mit Versionen
    lic = os.path.join(app, "usr/share/licenses")
    files = [p for _, p in bundled + reserve] + [s for s, _, _ in roots if s.startswith("/usr")]
    pkgs = set()
    for i in range(0, len(files), 50):
        r = subprocess.run(["dpkg", "-S"] + [os.path.realpath(f) for f in files[i:i + 50]], text=True, capture_output=True).stdout
        for line in r.splitlines(): pkgs.add(line.split(":")[0])
    rows = []
    for p in sorted(pkgs):
        c = f"/usr/share/doc/{p}/copyright"
        if os.path.exists(c):
            os.makedirs(os.path.join(lic, p)); shutil.copy2(os.path.realpath(c), os.path.join(lic, p, "copyright"))
        rows.append(run("dpkg-query", "-W", "-f=${Package} ${Version} (Quellpaket ${source:Package} ${source:Version})\n", p).strip())
    distro = re.search(r'PRETTY_NAME="(.+?)"', open("/etc/os-release").read()).group(1)
    shutil.copy2(os.path.join(SRC, "paket/LIZENZ.txt"), os.path.join(lic, "WS.Video-LIZENZ.txt"))
    if os.path.exists("/usr/share/common-licenses/GPL-3"): shutil.copy2("/usr/share/common-licenses/GPL-3", os.path.join(lic, "GPL-3.txt"))
    open(os.path.join(lic, "MITGELIEFERT.txt"), "w").write(
        f"WS.Video {VERSION} – mitgelieferte Software\n\nDieses AppImage enthält unveränderte Bibliotheken aus {distro}. Die Lizenztexte liegen je Paket in diesem Ordner.\n"
        "Der Quelltext dieser Pakete ist über die Paketquellen von Ubuntu erhältlich (https://packages.ubuntu.com, Quellpaket und Version siehe unten).\n"
        "Die Schrift Inter steht unter der SIL Open Font License (usr/share/fonts/Inter-LICENSE.txt).\n\n" + "\n".join(rows) + "\n")

    # Zusammenpacken: Laufzeit + komprimiertes Dateisystem
    name = f"WS.Video-{VERSION}-x86_64.AppImage"
    sq = os.path.join(OUT, "fs.squashfs")
    if os.path.exists(sq): os.remove(sq)
    run("mksquashfs", app, sq, "-root-owned", "-noappend", "-comp", "zstd", "-Xcompression-level", "19", "-b", "1M", "-mem", "1G", "-processors", "2", "-quiet", "-no-progress")
    target = os.path.join(OUT, name)
    with open(target, "wb") as o:
        o.write(open(os.path.join(HERE, "runtime-x86_64"), "rb").read())
        o.write(open(sq, "rb").read())
    os.chmod(target, 0o755); os.remove(sq)
    print(f"{target}\n{os.path.getsize(target) / 1048576:.0f} MB · im Paket {len(bundled)} Bibliotheken, als Reserve {len(reserve)}, vom System {len(skipped)}, {len(PLUGINS)} GStreamer-Plugins, {len(pkgs)} Pakete mit Lizenztext")
    print("Vom System erwartet: " + " ".join(skipped))

if __name__ == "__main__":
    main()
