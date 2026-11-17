# WS.Video installieren

Von der Datei bis zur ersten Aufnahme, Version 0.9.0. Dauert etwa fünf Minuten.

1. [Welcher Weg passt?](#1-welcher-weg-passt)
2. [AppImage](#2-appimage-ubuntu-mint-debian-fedora-rocky)
3. [Arch und Omarchy](#3-arch-und-omarchy-mit-hyprland)
4. [Einmal einrichten (Hyprland)](#4-einmal-einrichten-hyprland)
5. [Aktualisieren](#5-auf-eine-neue-version-aktualisieren)
6. [Wenn es klemmt](#6-wenn-es-klemmt)
7. [Entfernen](#7-ws-video-entfernen)

## 1. Welcher Weg passt?

| Dein System | Empfohlen |
| --- | --- |
| Ubuntu ab 24.04, Linux Mint 22, Debian 13, Fedora ab 43, Rocky Linux 10 | AppImage |
| Arch, Omarchy mit Hyprland | Paket mit `install.sh` (AppImage geht auch) |

Welches System du hast, zeigt `grep PRETTY_NAME /etc/os-release`. WS.Video braucht einen 64-Bit-Rechner mit Intel- oder AMD-Prozessor (x86_64).

## 2. AppImage: Ubuntu, Mint, Debian, Fedora, Rocky

Ein AppImage ist eine einzige Datei, die alles mitbringt. Es wird nichts ins System installiert, und du brauchst kein Administratorpasswort.

**Schritt 1: Herunterladen.** Auf der [Release-Seite](../../releases/latest) die Datei `WS.Video-0.9.0-x86_64.AppImage` laden (etwa 105 MB). Tipp: Leg sie an einen festen Platz, zum Beispiel `~/Programme`, denn der Eintrag im Startmenü zeigt später genau dorthin.

**Schritt 2: Ausführbar machen.**

- **GNOME (Ubuntu, Debian, Fedora, Rocky):** Rechtsklick auf die Datei, „Eigenschaften“, Schalter „Als Programm ausführbar“ einschalten.
- **Linux Mint (Nemo):** Rechtsklick, „Eigenschaften“, Reiter „Zugriffsrechte“, Haken bei „Datei als Programm ausführen“.
- **KDE (Dolphin):** Rechtsklick, „Eigenschaften“, Reiter „Berechtigungen“, Haken bei „Ausführbar“.
- **Terminal:**
  ```bash
  chmod +x ~/Downloads/WS.Video-0.9.0-x86_64.AppImage
  ```

**Schritt 3: Starten.** Doppelklick auf die Datei, oder im Terminal `~/Downloads/WS.Video-0.9.0-x86_64.AppImage`.

Beim ersten Start fragt WS.Video, ob es sich ins **Startmenü** eintragen soll. Sag ja: Dann steht WS.Video bei deinen Programmen, Projekte öffnen sich per Doppelklick, und im Terminal gibt es den Befehl `ws-video`. Nachholen oder entfernen:

```bash
~/Downloads/WS.Video-0.9.0-x86_64.AppImage --startmenue
~/Downloads/WS.Video-0.9.0-x86_64.AppImage --startmenue-entfernen
```

**Was auf deinem System schon geht:**

| System | Schnitt, Effekte, Ton, Export | Aufnahme | Mauszeiger-Effekte |
| --- | --- | --- | --- |
| Arch mit Hyprland | ja | ja | ja |
| Ubuntu, Debian, Fedora, Rocky 10 (GNOME, KDE mit Wayland) | ja | ja, Zeiger fest im Bild | in Arbeit |
| Linux Mint (Cinnamon, X11) | ja | in Arbeit | in Arbeit |

Unter GNOME und KDE fragt das System bei der Aufnahme, welchen Bildschirm oder welches Fenster WS.Video aufnehmen darf. Das ist normal.

## 3. Arch und Omarchy mit Hyprland

Hier läuft WS.Video mit allen Funktionen. `ws-video-0.9.0.tar.gz` von der [Release-Seite](../../releases/latest) laden, dann:

```bash
cd ~/Downloads
tar xzf ws-video-0.9.0.tar.gz
cd ws-video-0.9.0
./install.sh
```

Das Skript fragt einmal nach deinem Passwort (für pacman), entfernt eine alte Version und legt das Programm nach `~/.local/bin/ws-video`. Aufnahmen und Projekte bleiben erhalten. Startet das mitgelieferte Programm nicht, baut das Skript es automatisch aus dem Quelltext (einige Minuten, erzwingen mit `./install.sh --bauen`). Aus diesem Repository heraus baut `./install.sh` immer aus dem Quelltext und braucht dafür `cargo`.

Starten über den App-Starter (Omarchy: **SUPER + Leertaste**, „WS.Video“ tippen) oder im Terminal mit `ws-video`.

## 4. Einmal einrichten (Hyprland)

**Mausklicks aufzeichnen** (für Klick-Ringe), danach einmal ab- und anmelden:

```bash
sudo usermod -aG input $USER
```

Die Mausposition wird unter Hyprland auch ohne diesen Schritt aufgezeichnet.

**Tastenkürzel für die Aufnahme:**

```bash
ws-video --kuerzel-einrichten
```

| Taste | Wirkung |
| --- | --- |
| SUPER + ALT + R | Aufnahme starten, nochmal drücken beendet sie |
| SUPER + ALT + P | Pause und Fortsetzen |

`ws-video --kuerzel` zeigt die Zeilen nur an, `ws-video --kuerzel-entfernen` nimmt sie wieder heraus.

## 5. Auf eine neue Version aktualisieren

- **AppImage:** neue Datei laden, ausführbar machen, alte löschen und einmal `--startmenue` mit der neuen Datei ausführen.
- **Arch und Omarchy:** neues Paket entpacken und `./install.sh` ausführen.

Projekte, Aufnahmen und Einstellungen bleiben in beiden Fällen erhalten.

## 6. Wenn es klemmt

**Das AppImage startet per Doppelklick nicht.** Meist fehlt `fusermount`. Im Terminal starten, so siehst du auch die Meldung:

```bash
~/Downloads/WS.Video-0.9.0-x86_64.AppImage --appimage-extract-and-run
```

Oder FUSE nachinstallieren: Ubuntu, Mint, Debian `sudo apt install fuse3` · Fedora, Rocky `sudo dnf install fuse3` · Arch `sudo pacman -S fuse3`.

**Doppelklick öffnet einen Texteditor oder nichts.** Die Datei ist noch nicht ausführbar, Schritt 2 wiederholen.

**Die Grafikkarte kodiert den Export nicht.** WS.Video wiederholt den Export dann automatisch auf dem Prozessor. Schneller wird es mit den VA-API-Treibern deines Systems (zum Beispiel `intel-media-driver` oder `mesa-va-drivers`).

**Fehlerbericht erstellen:**

```bash
ws-video --fehlerbericht
# ohne Startmenü-Eintrag:
~/Downloads/WS.Video-0.9.0-x86_64.AppImage --fehlerbericht
```

Die Datei `ws-video-fehlerbericht.txt` landet in deinem Ordner Downloads. Sie enthält Version, Angaben zu System und Hardware, die Aufnahme-Umgebung und die Programmprotokolle. Benutzer- und Rechnername werden ersetzt, Inhalte deiner Aufnahmen sind nicht enthalten, verschickt wird nichts automatisch. Nach einem Absturz bietet WS.Video den Bericht beim nächsten Start selbst an. Häng die Datei an ein [Issue](../../issues) an und beschreib kurz, was du gerade gemacht hast.

## 7. WS.Video entfernen

**AppImage:**

```bash
~/Downloads/WS.Video-0.9.0-x86_64.AppImage --startmenue-entfernen
rm ~/Downloads/WS.Video-0.9.0-x86_64.AppImage
```

**Arch und Omarchy** (im entpackten Ordner):

```bash
./uninstall.sh          # Programm entfernen
./uninstall.sh --alles  # dazu auch die Einstellungen
```

Deine Aufnahmen und Projekte in `~/Videos/WS.Video/` werden in keinem Fall gelöscht.

---

Hilfe bei Einrichtung, Schulung oder Anpassung: [winkelmann.software](https://winkelmann.software)
