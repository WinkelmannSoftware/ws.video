#!/usr/bin/env bash
# WS.Video – Deinstallation (alte Python-Version und Rust-Version). Aufnahmen und Projekte bleiben erhalten.
set -u
pkill -x ws-video 2>/dev/null || true
rm -rf "$HOME/.local/share/ws-video"
rm -f "$HOME/.local/bin/ws-video"
rm -f "$HOME/.local/share/applications/ws-video.desktop" "$HOME/.local/share/applications/app.wsvideo.WSVideo.desktop"
rm -f "$HOME/.local/share/icons/hicolor/scalable/apps/ws-video.svg"
rm -rf "$HOME/.cache/ws-video"
rm -f "$HOME/.local/share/mime/packages/app.wsvideo.WSVideo.xml"
update-mime-database "$HOME/.local/share/mime" 2>/dev/null || true
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
if [ "${1:-}" = "--alles" ]; then
  rm -rf "$HOME/.config/WS.Video" "$HOME/.config/ws-video"
  echo "Einstellungen entfernt."
fi
echo "WS.Video ist entfernt. Deine Aufnahmen und Projekte in ~/Videos/WS.Video wurden nicht gelöscht."
