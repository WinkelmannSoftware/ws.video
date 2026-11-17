#!/usr/bin/env bash
# WS.Video – Installation für Arch / Omarchy. Entfernt zuerst die alte Version.
set -euo pipefail
SRC="$(cd "$(dirname "$0")" && pwd)"
BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/scalable/apps"
PAKETE=(libpulse gtk4 gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav
        gst-plugin-pipewire gst-editing-services gst-plugin-va pipewire-pulse xdg-desktop-portal xdg-desktop-portal-hyprland inter-font)

echo "==> 1/4 Alte Version entfernen (Aufnahmen und Projekte bleiben erhalten)"
pkill -x ws-video 2>/dev/null || true
rm -rf "$HOME/.local/share/ws-video"
rm -f "$BIN/ws-video" "$APPS/ws-video.desktop" "$APPS/app.wsvideo.WSVideo.desktop" "$ICONS/ws-video.svg"

echo "==> 2/4 Pakete prüfen"
if command -v pacman >/dev/null; then
  FEHLEN=()
  for p in "${PAKETE[@]}"; do pacman -Qq "$p" >/dev/null 2>&1 || FEHLEN+=("$p"); done
  if [ ${#FEHLEN[@]} -gt 0 ]; then
    echo "    Es fehlen: ${FEHLEN[*]}"
    sudo pacman -S --needed "${FEHLEN[@]}"
  else
    echo "    Alles vorhanden."
  fi
else
  echo "    Kein pacman gefunden. Bitte GTK 4, GStreamer (base, good, bad, ugly, libav, pipewire, editing-services) selbst installieren."
fi

echo "==> 3/4 Programm installieren"
mkdir -p "$BIN" "$APPS" "$ICONS"
if [ "${1:-}" != "--bauen" ] && "$SRC/bin/ws-video" --version >/dev/null 2>&1; then
  install -m755 "$SRC/bin/ws-video" "$BIN/ws-video"
  echo "    Fertiges Programm installiert: $("$BIN/ws-video" --version)"
else
  echo "    Das fertige Programm startet hier nicht (oder --bauen gewählt). Ich baue es aus dem Quelltext."
  if ! command -v cargo >/dev/null; then sudo pacman -S --needed rust; fi
  sudo pacman -S --needed base-devel pkgconf clang 2>/dev/null || true
  (cd "$SRC/quelltext" && cargo build --release)
  install -m755 "$SRC/quelltext/target/release/ws-video" "$BIN/ws-video"
  echo "    Selbst gebaut und installiert: $("$BIN/ws-video" --version)"
fi
cp "$SRC/ws-video.svg" "$ICONS/ws-video.svg"
# Ordner für eigene Designs (Beispiel liegt im Paket unter themen/)
mkdir -p "$HOME/.config/ws-video/themen"
if [ -d "$SRC/vorlagen" ]; then
  mkdir -p "$HOME/.local/share/ws-video/vorlagen"
  cp "$SRC/vorlagen/"* "$HOME/.local/share/ws-video/vorlagen/"
fi
cat > "$APPS/app.wsvideo.WSVideo.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=WS.Video
Comment=Bildschirm aufnehmen und Videos bearbeiten
Exec=$BIN/ws-video %F
Icon=ws-video
Terminal=false
Categories=AudioVideo;Video;Recorder;
MimeType=application/x-wsvideo;
StartupWMClass=app.wsvideo.WSVideo
DESKTOP
# Projektdateien (.wsvideo) dem Programm zuordnen: Doppelklick im Dateimanager öffnet das Projekt
MIME="$HOME/.local/share/mime"
mkdir -p "$MIME/packages"
cat > "$MIME/packages/app.wsvideo.WSVideo.xml" <<MIMEXML
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-wsvideo">
    <comment>WS.Video-Projekt</comment>
    <glob pattern="*.wsvideo"/>
    <icon name="ws-video"/>
  </mime-type>
</mime-info>
MIMEXML
update-mime-database "$MIME" 2>/dev/null || true
update-desktop-database "$APPS" 2>/dev/null || true
xdg-mime default app.wsvideo.WSVideo.desktop application/x-wsvideo 2>/dev/null || true
gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

echo "==> 4/4 Prüfen"
case ":$PATH:" in *":$BIN:"*) ;; *) echo "    Hinweis: $BIN ist nicht im PATH. Starte mit $BIN/ws-video oder über das Startmenü.";; esac
if ! id -nG | tr ' ' '\n' | grep -qx input; then
  echo "    Für Klick-Ringe (Mausklicks als Daten): sudo usermod -aG input $USER  und danach neu anmelden."
fi
echo
echo "Fertig. Start: ws-video   (oder im Startmenü: WS.Video)"
echo "Tastenkürzel für Hyprland stehen in LIESMICH.md."
