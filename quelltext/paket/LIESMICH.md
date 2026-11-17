# WS.Video 0.9.0

Bildschirmrekorder und Videoeditor für Linux. Entwickelt für Omarchy (Arch + Hyprland), seit 0.9.0 auch als
AppImage für Ubuntu ab 24.04, Linux Mint 22, Debian 13, Fedora ab 43 und Rocky Linux 10.
Technik: Rust, GTK 4, GStreamer und GStreamer Editing Services. WS.Video ist freie Software unter der
GNU General Public License Version 3 oder später (siehe `LIZENZ.txt`).

## AppImage (für alle genannten Linux-Systeme)

Das AppImage ist eine einzelne Datei, die alles Nötige mitbringt. Es muss nichts installiert werden.

1. Die Datei `WS.Video-0.9.0-x86_64.AppImage` herunterladen, zum Beispiel in den Ordner Downloads.
2. Die Datei ausführbar machen: im Dateimanager Rechtsklick, „Eigenschaften“, „Als Programm ausführen“
   erlauben. Im Terminal geht es mit `chmod +x ~/Downloads/WS.Video-0.9.0-x86_64.AppImage`.
3. Doppelklick auf die Datei startet WS.Video.

Beim ersten Start bietet WS.Video an, sich ins Startmenü einzutragen. Danach steht es bei den anderen
Programmen, Projektdateien öffnen sich per Doppelklick, und im Terminal gibt es den Befehl `ws-video`.
Die AppImage-Datei muss dafür an ihrem Platz bleiben. Nachholen: `./WS.Video-0.9.0-x86_64.AppImage --startmenue`,
wieder entfernen: `--startmenue-entfernen`.

Was auf welchem System geht:

| System | Schnitt, Effekte, Ton, Export | Aufnahme | Mauszeiger-Daten für Effekte |
| --- | --- | --- | --- |
| Arch mit Hyprland (Omarchy) | ja | ja | ja |
| Ubuntu, Debian, Fedora, Rocky 10 (GNOME, KDE mit Wayland) | ja | ja, Zeiger fest im Bild | noch nicht, in Arbeit |
| Linux Mint (Cinnamon mit X11) | ja | noch nicht, in Arbeit | noch nicht, in Arbeit |

Startet das AppImage nicht per Doppelklick, fehlt dem System meist FUSE. Dann im Terminal starten mit
`./WS.Video-0.9.0-x86_64.AppImage --appimage-extract-and-run`.

## Fehlerbericht

```
ws-video --fehlerbericht
```

(ohne Startmenü-Eintrag: `./WS.Video-0.9.0-x86_64.AppImage --fehlerbericht`)

Der Befehl schreibt die Datei `ws-video-fehlerbericht.txt` in den Ordner Downloads. Sie enthält die Version von
WS.Video, Angaben zum Betriebssystem (Distribution, Kern, Desktop, Wayland oder X11), zur Hardware (Prozessor,
Arbeitsspeicher, Grafik, Monitore, Namen der Kameras), zur Aufnahme-Umgebung (Bildschirmportal, PipeWire),
die vorhandenen Videobausteine, die Einstellungen von WS.Video und die Programmprotokolle.
Benutzername und Rechnername werden ersetzt, Inhalte von Aufnahmen sind nicht enthalten. Die Datei wird nicht
automatisch versendet. Wer sie weitergibt, ist damit einverstanden, dass die Angaben zur Fehlersuche verarbeitet
werden. Dieser Hinweis steht auch am Anfang der Datei.

Nach einem Absturz bietet WS.Video den Fehlerbericht beim nächsten Start von selbst an.

## Installieren unter Arch und Omarchy (ohne AppImage)

```bash
tar xzf ws-video-0.9.0.tar.gz
cd ws-video-0.9.0
./install.sh
```

Das Skript entfernt zuerst die alte Version, installiert fehlende Pakete mit pacman
und legt das Programm nach `~/.local/bin/ws-video`. Aufnahmen und Projekte bleiben erhalten.

Startet das mitgelieferte Programm auf deinem Rechner nicht, baut das Skript es automatisch aus dem
Quelltext (dauert einige Minuten). Das lässt sich auch erzwingen: `./install.sh --bauen`

Entfernen: `./uninstall.sh` (mit `--alles` auch die Einstellungen).

## AppImage selbst bauen

Auf Ubuntu 24.04 mit Rust, den GStreamer-Paketen, `patchelf` und `squashfs-tools`:

```bash
cd quelltext
cargo build --release
python3 paket/appimage/bauen.py
```

Das Ergebnis liegt in `target/appimage/`. GTK, GStreamer, die Kodierer und PipeWire kommen ins Paket.
Bibliotheken, die eng am System hängen (Grafik, Fenster, Schrift, Ton), nimmt WS.Video vom System und bringt
sie nur als Reserve mit. Die Lizenztexte aller mitgelieferten Pakete liegen im AppImage unter
`usr/share/licenses`.

## Einmalig einrichten (empfohlen)

**Mausklicks aufzeichnen** (für Klick-Ringe): `sudo usermod -aG input $USER`, danach neu anmelden.
Die Mausposition wird unter Hyprland auch ohne diesen Schritt aufgezeichnet.

**Tastenkürzel für die Aufnahme** (gelten auf jeder Arbeitsumgebung und jedem Monitor):

```
ws-video --kuerzel-einrichten
```

Der Befehl sucht zwei freie Tasten, legt eine Sicherungskopie deiner Hyprland-Datei an (bei Omarchy
`~/.config/hypr/bindings.lua`, bei älteren Einrichtungen `bindings.conf` oder `hyprland.conf`), trägt zwei Zeilen ein und sagt dir, welche Tasten es
geworden sind. Normalerweise sind das:

| Taste | Wirkung |
| --- | --- |
| SUPER + ALT + R | Aufnahme starten mit den letzten Einstellungen, noch einmal drücken beendet sie |
| SUPER + ALT + P | Aufnahme pausieren und fortsetzen |

`ws-video --kuerzel` zeigt die Zeilen nur an, `ws-video --kuerzel-entfernen` nimmt sie wieder heraus.
Eine Fensterregel für das kleine Aufnahmefenster ist nicht mehr nötig, WS.Video richtet es selbst ein.
Alte Zeilen mit `ws-video --stop` oder `ws-video --pause` aus früheren Anleitungen gelten weiter.

## Neu in 0.9.0

**AppImage für andere Linux-Systeme.** WS.Video gibt es jetzt als einzelne Datei, die auf Ubuntu ab 24.04,
Linux Mint 22, Debian 13, Fedora ab 43, Rocky Linux 10 und Arch läuft, ohne dass Pakete installiert werden müssen.
Beim ersten Start bietet es den Eintrag ins Startmenü an.

**Fehlerbericht mit Systemanalyse.** `ws-video --fehlerbericht` schreibt eine Datei mit Angaben zu Programm,
System, Hardware und Aufnahme-Umgebung in den Ordner Downloads (auch wenn er anders heißt). Der alte Befehl
`--bericht` gilt weiter. Nach einem Absturz gibt es dafür einen Knopf.

**Freie Software.** WS.Video steht ab dieser Version unter der GNU General Public License 3 oder später.
Der Quelltext liegt jedem Paket bei.

## Neu in 0.8.2

**Tastenkürzel für Hyprland mit Lua-Dateien.** Neuere Omarchy-Versionen führen die Einstellungen von Hyprland
in Lua-Dateien. `ws-video --kuerzel-einrichten` hat bisher in `bindings.conf` geschrieben, die dort gar nicht
mehr gelesen wird, die Tasten blieben deshalb ohne Wirkung. Jetzt erkennt der Befehl die Lua-Dateien, trägt die
Tasten in `~/.config/hypr/bindings.lua` ein und nimmt den wirkungslosen alten Eintrag aus `bindings.conf`
wieder heraus. Belegte Tasten erkennt er auch an den Dateien von Omarchy.

Hinweis zu Omarchy 4.5: Zeigt jedes Fenster oben einen zusätzlichen Balken mit drei Knöpfen, steht die
Arbeitsumgebung auf „schwebend“. Das Symbol dafür sitzt in der oberen Leiste, ein Klick darauf wechselt die
Anordnung.

## Neu in 0.8.1

**Falsche Meldung „unerwartet beendet“ behoben.** Das Programm hat beim Schließen nie vermerkt, dass es
ordentlich beendet wurde. Deshalb kam die Meldung bei jedem Start, auch wenn nichts passiert war. Schließen
über das X oben rechts war und ist richtig. Neu: Strg + Q schließt das Fenster ebenfalls. Die Meldung erscheint
jetzt nur noch nach einem echten Absturz oder Einfrieren.

## Neu in 0.8.0

**Ton verbessern mit einem Klick.** Links in der Leiste die Seite „Ton“ öffnen und „Ton verbessern (alles mit
einem Klick)“ anklicken. WS.Video misst dann die Aufnahme, entfernt das Rauschen (zum Beispiel den Lüfter),
nimmt tiefes Poltern heraus, gleicht laute und leise Stellen an und bringt alles auf eine übliche Lautstärke.
Du musst dafür keine Tonspur einzeln bearbeiten und nichts in ein anderes Programm geben:

- Ist kein Clip ausgewählt, gilt der Klick für allen Ton im Projekt.
- Ist ein Clip ausgewählt, gilt er für diese Aufnahme, und zwar für alle ihre Teile, auch wenn du sie schon
  zerschnitten hast. Was du später abschneidest, behält die Einstellung.
- Die Vorschau spielt den bearbeiteten Ton sofort, der Export enthält genau dasselbe.

**So wird das Rauschen gefunden.** WS.Video sucht die leisesten Stellen der Aufnahme, also die Pausen, in
denen nur der Lüfter zu hören ist, und merkt sich, wie dieses Rauschen klingt. Genau das wird dann aus der
ganzen Aufnahme herausgerechnet. Das klappt am besten, wenn irgendwo ein, zwei Sekunden lang niemand spricht.
Tipp: Lass am Anfang der Aufnahme kurz Ruhe. Die Messung läuft im Hintergrund und dauert bei einer Stunde
Aufnahme ungefähr eine halbe Minute. Oben erscheint dann „Ton gemessen“.

**Alle Regler** stehen rechts im Abschnitt „Ton“, sobald ein Clip mit Ton ausgewählt ist:

| Regler | Wirkung |
| --- | --- |
| Ton verbessern | setzt alles auf sinnvolle Werte (Rauschen entfernen, Brummen und Poltern heraus, Lautstärke angleichen, normale Lautstärke) |
| Rauschen | wie stark das Rauschen entfernt wird, ganz links ist aus. Klingt die Stimme dünn oder blechern: etwas zurücknehmen |
| Brummen und Poltern entfernen | nimmt alles unter 80 Hertz heraus (Tischstöße, Trittschall, tiefes Brummen) |
| Angleichen | leise und laute Stellen rücken zusammen (Kompressor) |
| Normale Lautstärke | bringt die Aufnahme auf ein übliches Maß, damit alle Aufnahmen ähnlich laut sind |
| Stimme klarer | hebt den Bereich an, in dem Sprache verständlich wird |
| Pausen | senkt den Ton zwischen den Sätzen ab |
| Tiefen, Höhen | Klang nach Geschmack |
| Tonbearbeitung zurücksetzen | alles wieder aus |

Ein Begrenzer läuft immer mit und verhindert, dass der Ton übersteuert. Für jede dieser Bearbeitungen gibt es
links auf der Seite „Ton“ auch einen Knopf zum An- und Ausschalten.

Bei Musik oder Systemton ohne gleichmäßiges Rauschen meldet WS.Video das und entrauscht nur ganz vorsichtig,
damit leise Töne nicht verschwinden.

## Neu in 0.7.0

**Aufnahme von überall steuern.** Mit den Tastenkürzeln oben startest, pausierst und beendest du die
Aufnahme, egal auf welcher Arbeitsumgebung du gerade bist und ob WS.Video zu sehen ist. Der Start per Taste
nimmt genau die Einstellungen der letzten Aufnahme (Quellen, Kamera, Mikrofon, Bildrate). Läuft WS.Video
noch nicht, startet die Taste es im Hintergrund. Dazu gibt es die Befehle `ws-video --start` (nur starten)
und `ws-video --toggle` (starten oder beenden).

**Wechsel der Arbeitsumgebung.** Wer einen ganzen Monitor aufnimmt, nimmt alles auf, was dieser Monitor
zeigt, auch wenn du die Arbeitsumgebung wechselst. Das war schon so. Nur wer ein einzelnes Fenster als Quelle
wählt, bekommt nur dieses Fenster.

**Aktivem Bildschirm folgen.** Im Fenster „Neue Aufnahme“ steht in der Karte „Bildschirm“ der neue Haken
„Aktivem Bildschirm folgen“. Damit nimmt WS.Video zwei Monitore auf und zeigt im Video immer den, auf dem
dein Mauszeiger gerade ist. Wechselst du mit der Maus auf den anderen großen Monitor, wechselt das Bild nach
einem Wimpernschlag mit. Steht der Zeiger auf einem dritten Monitor, zum Beispiel dem Laptop mit WS.Video,
bleibt das Bild beim letzten. Am Ende liegt eine einzige Bildschirmspur im Projekt.

So richtest du es einmal ein:

1. „Quelle wählen …“ anklicken und im Dialog des Systems den ersten großen Monitor wählen.
2. Darunter bei „Das ist Monitor“ denselben Monitor aus der Liste wählen. In der Liste steht der Name, die
   Größe und ob er links, in der Mitte oder rechts steht.
3. Haken bei „Aktivem Bildschirm folgen“ setzen, „Zweiten wählen …“ anklicken und den zweiten großen
   Monitor wählen.
4. Auch dort bei „Das ist Monitor“ den passenden Monitor wählen.

Die Zuordnung von Hand ist nötig, weil das System nicht verrät, welchen Monitor du im Dialog gewählt hast,
und deine beiden großen Monitore gleich groß sind. WS.Video merkt sich alles. Ob der Dialog des Systems beim
nächsten Start noch einmal fragt, hängt von Hyprland ab. Sind die beiden Monitore unterschiedlich groß, wird
das Video so groß wie der größere, das Bild des kleineren wird auf diese Größe vergrößert.

**Kleines Aufnahmefenster.** Das Fenster mit Zeit, Pause und Stopp lässt sich jetzt verschieben: an einer
freien Stelle mit der Maus anfassen und ziehen. Unter Hyprland macht WS.Video es von selbst schwebend, zeigt
es auf allen Arbeitsumgebungen und legt es beim Start auf einen Monitor, der nicht aufgenommen wird, wenn es
einen gibt. Dafür muss bei „Das ist Monitor“ ein Monitor gewählt sein. Beim Folgen steht im Fenster, welcher
Bildschirm gerade im Video ist.

## Neu in 0.6.2

**Schere zum Schneiden.** Ganz links in der Leiste über der Zeitleiste sitzt der Knopf „Schneiden“ mit der
Schere (Taste X). Solange er an ist, folgt eine rote Linie der Maus über alle Ebenen, im Zeitlineal steht die
genaue Zeit. In der Nähe des Abspielkopfs rastet die Linie auf ihn ein.

- **Klick** schneidet an der Linie.
- **Alle Ebenen oder eine:** Neben der Schere erscheint der Schalter „Alle Ebenen“. Ist er an, geht der Schnitt
  durch alle Ebenen. Ist er aus, wird nur die Ebene unter der Maus geschnitten. Umschalt beim Klick kehrt das
  für diesen einen Schnitt um. Die Linie ist auf den Ebenen kräftig, die geschnitten werden, auf den anderen
  dünn. Gesperrte Ebenen (Schloss im Spurkopf) bleiben immer unberührt, so lässt sich jede Auswahl treffen.
- **Bereich herausnehmen:** Mit der Schere ziehen statt klicken markiert einen Bereich, wieder auf allen Ebenen
  oder nur auf der einen. „Lücke schließen“ (Umschalt + Entf) nimmt ihn heraus und fügt die Teile zusammen.
  Entf nimmt ihn nur heraus und lässt die Lücke stehen.
- **Esc** oder noch ein Klick auf die Schere beendet das Werkzeug. „Teilen“ (Taste S) am Abspielkopf gibt es
  weiterhin.

## Neu in 0.6.1

**Effekte direkt in der Zeitleiste ziehen.**

- **Griffe für Anfang und Ende:** Jeder Clip hat oben links und oben rechts einen kleinen runden Griff. Ziehst du
  ihn in den Clip hinein, entsteht das Einblenden bzw. Ausblenden, und je weiter du ziehst, desto länger (also
  langsamer) läuft es. Ganz zurück an die Kante geschoben ist es wieder aus. Das gilt genauso für alle anderen
  Arten am Anfang und Ende: Über Schwarz, Zoom, Schwenk, Gleiten, Aufspringen und so weiter. Bei Tonspuren
  blendet der Griff die Lautstärke ein und aus.
- **Bewegungen als Balken:** Zoom und eigene Bewegungen liegen als gelber Balken unten im Clip. Den Balken
  ziehen verschiebt den Zeitpunkt, am rechten Ende ziehen macht die Bewegung schneller oder langsamer. Ein Klick
  auf den Balken springt an sein Ende, dort stellst du das Ziel in der Vorschau ein.
- **Kacheln ziehen:** Jede Kachel aus Übergänge, Animationen und Effekte lässt sich auf einen Clip in der
  Zeitleiste ziehen. Zoom und Bewegung beginnen an der Stelle, an der du loslässt. Anklicken geht weiterhin.
- **Anzeige der Einstellungen:** Beim Ziehen erscheint über dem Clip ein Feld mit Art und Dauer in Sekunden
  (bei Bewegungen Beginn und Dauer). Gleichzeitig rollt die rechte Leiste zu „Anfang und Ende“ bzw. zur Liste
  der Bewegungen und zeigt dieselben Werte, dort kannst du sie auch genau eintippen. Die Vorschau zieht mit.
- Rückgängig nimmt einen ganzen Ziehvorgang als einen Schritt zurück.

## Neu in 0.6.0

**Mehr Übergänge, Animationen und Effekte.** Die drei Menüs links zeigen jetzt Kacheln mit einem Bild, das
vormacht, was passiert. Fährst du mit der Maus über eine Kachel, läuft das Bild als kleine Vorschau ab.

- **Übergänge** (zwischen zwei Clips, die auf einer Spur aneinanderstoßen): Über Schwarz, Über Weiß, Lichtblitz,
  Zoom, Schwenk nach links und nach rechts, Unschärfe, Bildstörung (Glitch). Clip auswählen, Kachel anklicken:
  der Übergang wirkt am Anfang des gewählten Clips und am Ende des Clips davor. Weiches Überblenden, Ein- und
  Ausblenden und Aufrollen gibt es weiterhin.
- **Animationen**: Hineingleiten und Hinausgleiten aus vier Richtungen, Aufspringen (mit kurzem Überschwingen)
  und Schrumpfen. Gilt für Videos, Bilder, Text und Anmerkungen.
- **Effekte**: Farbe (Helligkeit, Kontrast, Sättigung, Farbtemperatur, Schwarzweiß), Vignette, Spiegeln, Auf den
  Kopf, und Verpixeln als Variante des Weichzeichner-Bereichs. Die Kacheln stellen einen sinnvollen Wert ein,
  die Feineinstellung steht rechts unter „Farbe und Bild“ und „Anfang und Ende“.
- Vorschau und Export rechnen diese Effekte mit demselben Baustein, sie sehen deshalb gleich aus. Alles wird im
  Projekt gespeichert, ältere Projekte lassen sich weiter öffnen, Rückgängig funktioniert.

**Zweites Design und Umschalter.** Oben rechts neben den Fensterknöpfen sitzt ein Knopf mit einer Farbpalette.
Dort wählst du zwischen „Grün“ und „Electric Blue“. Der Wechsel wirkt sofort und wird gemerkt.

**Eigene Designs.** Jede JSON-Datei in `~/.config/ws-video/themen/` erscheint als weiteres Design in der Auswahl.
Ein Beispiel liegt im Paket unter `themen/beispiel-orange.json`:

```bash
cp themen/beispiel-orange.json ~/.config/ws-video/themen/
ws-video --thema-beispiel > ~/.config/ws-video/themen/meins.json   # das blaue Design als Vorlage
```

Die Datei hat drei Einträge: `name`, `farben` und (wahlweise) `css` für eigene Regeln. Farben, die fehlen,
kommen aus dem grünen Design. Die Namen der Farben:

- Flächen: `grund`, `feld`, `flaeche`, `leiste`, `karte_aus`, `buehne` (Vorschau), `inaktiv`
- Linien und Ränder: `linie`, `rand`, `rinne`, `schalter`, `haken_rand`, `rollbalken`
- Knöpfe: `knopf`, `knopf_maus`, `flach_maus`, `zeile_maus`
- Akzent: `akzent`, `akzent_hell`, `auf_akzent` (Schrift auf Akzentfläche), `auswahl`, `auswahl_rand`,
  `auswahl_ring`
- Schrift: `text`, `text_matt`, `text_leise`, `text_aus`, `text_fenster`
- Zeichen „W.“ und Lichtstreifen: `leuchten`, `logo_hell`, `logo_mitte`, `logo_dunkel`, `logo_punkt`, `logo_glanz`

Außerdem: Weichzeichner und Verpixeln wirken jetzt auch auf klein dargestellte Bilder so stark, wie eingestellt.

## Neu in 0.5.2

Behebt das Einfrieren und den Absturz beim Positionieren (Fehlerbericht vom 15.11.).

- Absturz: Hatte ein Clip eine Ein- oder Ausblendung oder eine Zoom-Bewegung, hat das Programm bei jeder
  Mausbewegung dessen Steuerkurve entfernt und neu angelegt. Die Wiedergabe liest diese Kurven ohne Sperre
  und griff dabei ab und zu ins Leere. Jetzt bleibt eine Kurve bestehen und bekommt nur neue Werte.
- Einfrieren: Beim Setzen von Breite und Höhe konnten sich zwei Threads der Schnittbibliothek gegenseitig
  blockieren, wenn die Vorschau gerade umschaltete. Die Meldung, an der beide hingen, ist abgeschaltet.
  Außerdem wird nur noch gesetzt, was sich wirklich geändert hat.
- Verschieben und Trimmen in der Zeitleiste lädt den Clip nicht mehr bei jeder Mausbewegung neu. Das ist
  deutlich flüssiger und braucht bei langen Zügen nicht mehr immer mehr Arbeitsspeicher.
- Das Protokoll erkennt jetzt auch ein Einfrieren: Reagiert die Oberfläche 15 Sekunden nicht, schreibt das
  Programm auf, wo jeder Thread gerade steht. `ws-video --bericht` nimmt das mit.

## Neu in 0.5.1

Das Designpaket von Winkelmann.Software ist eingebaut.

- Das „W.“ kommt jetzt aus der gelieferten Logo-Datei: als Symbol im Startmenü, für Projektdateien und in der
  Kopfzeile.
- Die Kopfzeile trägt den grünen Lichtstreifen aus der Vorlage. Er liegt hinter Titel und Knöpfen und endet vor
  den Fensterknöpfen.
- Farben genau nach der Übergabe: Hintergrund `#041318`, Flächen `#0B2028`, Akzent `#00E676`, Text `#F5F8FA`.
- Schrift Inter. `install.sh` installiert das Paket `inter-font`, wenn es fehlt. Ohne Inter nimmt das Programm
  die Systemschrift, sonst ändert sich nichts.
- Die Intro-Akzente liegen als durchsichtiges Bild bei (`vorlagen/intro-akzente.png`, 1920 × 1080) und werden
  nach `~/.local/share/ws-video/vorlagen/` kopiert. Du holst sie wie jedes Bild über „Medien“ ins Projekt und
  legst sie auf eine Spur über dem Video. Das Programm legt sie nie von selbst auf dein Material.
- An Aufnahme, Schnitt, Export und am Projektformat hat sich nichts geändert.

## Neu in 0.5.0

Neues Erscheinungsbild und Projekte als Ordner.

- Design nach dem Entwurf von Winkelmann.Software: dunkles Blaugrün mit grünem Akzent, eigene Kopfzeile mit
  dem „W.“ und Fensterknöpfen, neues Symbol im Startmenü.
- Linke Leiste nur noch mit Symbolen (der Name erscheint, wenn die Maus darauf steht).
- Rechte Leiste schmaler und fest in der Breite, alle Felder gleich lang. Die Vorschau in der Mitte bekommt den
  gewonnenen Platz.
- Ein Projekt ist ein Ordner. Darin liegen die Projektdatei (`Name.wsvideo`, der Bearbeitungsstand), die
  Aufnahmen getrennt nach Quelle (`bildschirm.mkv`, `kamera.mkv`, `mikrofon.mka`, `systemton.mka`,
  `mauszeiger.json`) und unter `medien/` alles, was du importierst (Logos, Bilder, Videos, Musik,
  Hintergrundbild der Kamera). Importierte Dateien werden dorthin kopiert, sehr große nach Möglichkeit nur
  verknüpft, damit sie keinen zweiten Platz belegen.
- Die Projektdatei merkt sich die Dateien relativ zum Ordner. Du kannst den Ordner verschieben, umbenennen,
  sichern oder auf einen anderen Rechner kopieren, das Projekt öffnet sich trotzdem vollständig.
- Öffnen: über „Projekt öffnen“, über „Zuletzt geöffnet“, per Doppelklick auf die `.wsvideo`-Datei im
  Dateimanager oder mit `ws-video /pfad/zum/ordner`. Der Bearbeitungsstand wird alle 5 Sekunden und beim
  Schließen gespeichert.
- „Projekt speichern unter …“ legt einen neuen Projektordner mit dem gewählten Namen an und nimmt alles mit.
- Ältere Projekte lassen sich weiter öffnen. Ihre Dateien bleiben, wo sie sind.

## Neu in 0.4.4

Absturz beim Bearbeiten behoben, dazu ein eigenes Fehlerprotokoll.

- Absturz: Beim Ziehen an der Größe eines Bildes baute WS.Video den Clip mehrfach neu auf und gab den alten
  sofort frei. Die Schnitt-Bibliothek lud dessen Bild aber noch und griff dann ins Leere. Entfernte Clips
  bleiben jetzt am Leben, bis die Vorschau die Änderung sicher übernommen hat. Das betrifft jede Änderung, die
  einen Clip neu aufbaut (verschieben und kürzen in der Zeitleiste, Schatten, Form, Rückgängig).
- Beim Ziehen an der Größe wird nur noch selten neu aufgebaut.
- Fehlerprotokoll: Alle Meldungen des Programms stehen in `~/.local/state/ws-video/protokoll.log`. Nach einem
  Absturz steht dort, in welchem Thread und an welcher Stelle er passiert ist, und WS.Video weist beim nächsten
  Start darauf hin. Der Befehl `ws-video --bericht` schreibt alles Nötige (Protokolle, Absturzbericht des
  Systems, Versionen) in die Datei `~/Downloads/ws-video-fehlerbericht.txt`.

## Neu in 0.4.3

Fehler aus 0.4.2 behoben: Bilder (Logo, Icon) und Videos ohne weiteren Effekt ließen sich nicht einfügen.

- Ursache: Der neue Schritt, der Bilder für die Vorschau vorab verkleinert, erzeugte für Clips ohne weiteren
  Effekt eine ungültige Effektkette. Der Clip wurde als „Datei fehlt“ markiert, blieb aber halb angelegt in der
  Spur liegen: formatfüllend, unscharf und nicht mehr zu bewegen. Betroffen waren Bilder, importierte Videos
  und Aufnahmen ohne Mauszeiger-Daten, sowohl in der Vorschau als auch im Export.
- Die Effektkette ist jetzt gültig. Schlägt das Anlegen eines Clips trotzdem einmal fehl, bleibt nichts
  Halbfertiges zurück, und kleine Bilder werden gar nicht erst verkleinert.
- Kleine Objekte lassen sich in der Vorschau besser greifen: Die Mitte gehört dem Verschieben, die Griffe für
  die Größe greifen bei kleinen Objekten nur noch außen. Für feine Arbeit hilft Strg + Mausrad (Vorschau zoomen).
- Ein Bild wird in seiner Originalgröße mittig eingefügt (ein Icon mit 84 × 75 Pixeln ist in einem
  Full-HD-Projekt also klein). Größer ziehen geht an den Eckgriffen; beim Vergrößern wird ein kleines Bild
  naturgemäß unscharf, dann besser eine größere Vorlage verwenden.
- Die Zeitleiste unterscheidet jetzt „Datei fehlt“ (Datei nicht mehr am Ort) und „Nicht ladbar“.

## Neu in 0.4.2

Behebt ruckelnde Aufnahmen und ein Bild, das hinter dem Ton herläuft.

- Vorschau im Editor: Das Bild bleibt jetzt am Ton. Schafft der Rechner die volle Bildrate nicht, zeigt die Vorschau weniger Bilder je Sekunde, statt immer weiter zurückzufallen. Neben der Zeitanzeige steht dann zum Beispiel "Vorschau 15 Bilder/s". Der Export ist davon nicht betroffen.
- Die Vorschau rechnet Effekte in der Größe der Vorschau statt in voller 4K-Größe. Das spart den größten Teil der Rechenzeit.
- Aufnahme: Das Video wird im üblichen Farbformat 4:2:0 und mit fester Qualität gespeichert (vorher 4:4:4 mit sehr hoher Datenrate). Eine 4K-Aufnahme mit Kamera braucht dadurch nur noch etwa ein Drittel der Rechenzeit, und die Dateien lassen sich beim Bearbeiten mehrfach schneller lesen.
- Kommt der Rechner beim Aufnehmen trotzdem nicht mit, fehlen einzelne Bilder. Bild und Ton bleiben aber gleich lang und zeitgleich, und die Datei wird sauber abgeschlossen. Vorher staute sich das Bild auf, und das Ende der Aufnahme fehlte.
- Neue Auswahl "Größe" im Aufnahmefenster: Automatisch, Original, höchstens 1440p, höchstens 1080p.
- In jedem Aufnahmeordner liegt jetzt eine Datei `diagnose.txt`. Sie zeigt, wie viele Bilder ankamen, ob welche fehlen und ob der Rechner mitkam. Bei Problemen bitte diese Datei mitschicken.
- Export: H.264 und HEVC werden im Farbformat 4:2:0 geschrieben und laufen damit auf allen Geräten.
- Aufnahmen aus älteren Versionen bleiben schwer zu lesen (4K in 4:4:4). Die neue Vorschau gleicht das aus; am leichtesten laufen neue Aufnahmen.

Fehlersuche: `WS_DEBUG=1 ws-video` im Terminal starten. Das Programm schreibt dann mit, wann und warum die Vorschau ihre Bildrate ändert und wie die Aufnahme aufgebaut ist.

## Neu in 0.4.1

- Bildschirmvorschau im Aufnahmefenster: zeigt jetzt das ganze Bild statt eines schmalen Streifens (Hyprland, 4K-Monitor).
- Mikrofone und Ausgabegeräte: Die Liste wird jetzt zusätzlich über `pactl` und über PipeWire gelesen, damit auch USB-Mikrofone erscheinen.

## Neu in 0.4.0

- Kamera-Hintergrund ersetzen ohne grüne Wand: Das Programm erkennt die Person im Kamerabild.
  - Vor der Aufnahme: im Aufnahmefenster bei "Kamera" den Hintergrund wählen (Original, entfernen, unscharf, eigenes Bild). Die kleine Vorschau zeigt das Ergebnis.
  - Nachträglich: Kamera-Clip auswählen, rechts "Hintergrund ersetzen" oder links "Effekte". Die Aufnahme selbst bleibt unverändert, du kannst jederzeit umstellen oder abschalten.
  - "Entfernen" macht den Hintergrund durchsichtig: Dann steht die Person frei über der Bildschirmaufnahme.
- Die Erkennung nutzt das frei lizenzierte Selfie-Segmentierungsmodell von MediaPipe (Apache 2.0). Es ist im Programm enthalten und läuft auf deinem Rechner, es wird nichts ins Internet geschickt.

## Neu in 0.3.4

- Einzelne Buchstaben färben: Text auswählen, rechts im Textfeld die Buchstaben markieren, Farbe im Farbfeld wählen, "Markierung färben" drücken. "Zurücksetzen" nimmt die Farbe wieder weg.
- Google-Schriften: rechts unter dem Text den Namen der Schrift eintippen (zum Beispiel Poppins, Open Sans, Montserrat) und "Laden" drücken. Die Schrift wird nach ~/.local/share/fonts/ws-video installiert, für den Text gewählt und steht danach auch unter "Schrift" bereit. Dafür braucht es Internet und das Programm curl.

## Neu in 0.3.3

- Roll-Effekt: Balken, Texte, Bilder und Videos lassen sich von links aufrollen und am Ende wieder zusammenrollen. Clip auswählen, links "Übergänge" öffnen, "Auf- und zusammenrollen" wählen. Die Dauer und die Art (weich blenden oder rollen) stellst du rechts unter "Ein- und Ausblenden" ein.
- Kapitelbalken mit grünem Hintergrund: importieren, auswählen, links "Effekte", "Greenscreen an / aus", danach den Roll-Effekt wählen.

## Neu in 0.3.2

- Links gibt es eine senkrechte Kategorienleiste: Medien, Anmerkungen, Übergänge, Animationen, Effekte, Mauszeiger und Ton. Clip auswählen, Kategorie öffnen, Aktion anklicken.
- Oben gibt es einen eigenen Knopf "Text". Er legt einen Text an und setzt die Eingabe sofort ins Textfeld.
- Texte haben jetzt eine Kontur (Stärke und Farbe) und sechs fertige Stile: Schlicht, Kontur, Titel, Untertitel, Hinweis, Marker.
- Medien einfügen: Knopf "Importieren" oben, Kategorie "Medien" links oder Dateien aus dem Dateimanager ins Fenster ziehen.

## Bedienen

1. **Neue Aufnahme**: Bildschirm, Kamera, Mikrofon und Systemton einzeln einschalten, Gerät wählen.
   „Quelle wählen“ öffnet die Auswahl von Hyprland (Monitor, Fenster oder Bereich); alle drei Monitore
   stehen dort zur Wahl. Jede Quelle landet in einer eigenen Datei und auf einer eigenen Spur.
2. **Steuerleiste**: Zeit, Pause, Neustart, Stopp, Kamerabild, Pegel. Nach „Stopp“ öffnet sich der Editor.
3. **Editor**
   - Vorschau: Clip anklicken, ziehen, an den Griffen skalieren (Umschalt = frei verzerren), Strg+Mausrad zoomt.
   - Werkzeuge: **V** Auswahl, **H** Ansicht verschieben, **C** Zuschneiden.
   - Kamera: rechts unter „Form und Rahmen“ Rechteck, abgerundet oder Kreis, Rahmen, Schatten, Greenscreen.
   - Logos und Bilder: aus dem Dateimanager in die Vorschau oder die Zeitleiste ziehen.
   - Zeitleiste: **S** teilen, **Entf** löschen, **Umschalt+Entf** löschen und Lücke schließen,
     Clipränder ziehen zum Trimmen, Umschalt+Ziehen im Zeitlineal markiert einen Bereich.
   - Überblenden: Clip über das Ende des Vorgängers ziehen oder „Überblenden“ drücken.
   - Anmerkungen (links): Text, Rechteck, Ellipse, Pfeil, Linie, Weichzeichner. Doppelklick auf Text zum Tippen.
   - Zoom und Schwenk: Clip wählen, Abspielkopf setzen, „Hineinzoomen“ (zoomt auf den Mauszeiger), später „Zurück“.
   - Mauszeiger: Größe, Glätten, Hervorheben, Klick-Ringe.
   - **Leertaste** spielt ab, **Strg+Z / Strg+Y** rückgängig und wiederholen, **Strg+S** speichern, **Strg+E** exportieren.
4. **Exportieren**: MP4 oder MKV in HD, Full HD, 2K oder 4K, H.264, HEVC oder AV1. Mit Haken kodiert die
   Grafikkarte (VA-API oder NVENC); klappt das nicht, wiederholt WS.Video den Export automatisch auf der CPU.

## Wo liegt was

- Aufnahmen und Projekte: `~/Videos/WS.Video/`, je Projekt ein Ordner (`Aufnahme_Datum_Zeit/` oder
  `Projekt_Datum_Zeit/`) mit Projektdatei (`.wsvideo`), Aufnahmen und `medien/`
- Fehlerprotokoll: `~/.local/state/ws-video/`
- Einstellungen: `~/.config/ws-video/`, eigene Designs in `~/.config/ws-video/themen/`
- Zwischenspeicher: `~/.cache/ws-video/` (kann gelöscht werden)

## Wenn etwas nicht geht

Führe im Terminal `ws-video --bericht` aus und schicke mir die Datei `~/Downloads/ws-video-fehlerbericht.txt`.
Sie enthält die Meldungen des Programms, nach einem Absturz die Absturzstelle und nach einem Einfrieren (ab
15 Sekunden) den Stand aller Threads. Hilfreich ist außerdem:

```bash
gst-inspect-1.0 pipewiresrc | head -3      # Bildschirmaufnahme
gst-inspect-1.0 | grep -E "vah264enc|nvh264enc|x264enc"   # Encoder
```

## Stand der Tests

**0.9.0, AppImage.** Geprüft in Systemabbildern ohne Bildschirm: Debian 13, Ubuntu 26.04, Fedora 43, Rocky Linux 10
und Arch, jeweils ohne ein einziges Zusatzpaket (dann kommen alle Reserve-Bibliotheken zum Einsatz). Dort liefen
Aufnahme mit Testquellen, Export nach MP4 mit H.264, Start der Oberfläche und der Fehlerbericht. In Ubuntu 26.04
zusätzlich mit den Grafikbibliotheken des Systems: Oberfläche mit OpenGL unter X11 und unter Wayland (Weston ohne
Bildschirm). Ubuntu 24.04 ist das Bausystem, Linux Mint 22 hat dieselbe Grundlage.
Nicht geprüft, weil hier nicht möglich: echte Desktops (GNOME, KDE, Cinnamon), das Bildschirmportal und PipeWire
dieser Systeme, Kamera, Mikrofon, Kodierung über den Grafikchip, Debian 13 und Fedora mit den Grafikbibliotheken
des Systems, Fedora 44 und 45, Start per Doppelklick über FUSE.

Im Testlabor (ohne echten Bildschirm, mit Testquellen) geprüft: Aufnahme mit vier Quellen, Pause, Stopp,
Übergabe an den Editor, Auswahl und Verschieben, Text und Anmerkungen, Zoom auf den Mauszeiger, Export.
Für 0.6.0 dazu: alle neuen Übergänge, Animationen und Effekte im Export Bild für Bild, das Anklicken der
Kacheln, der Wechsel des Designs und ein Dauertest mit 4500 Änderungen ohne Fehler.
Für 0.7.0 dazu: eine Aufnahme, die zwischen zwei Testbildern dem Mauszeiger folgt, sowie Start, Pause und
Stopp per Befehl aus einem zweiten Aufruf.
Für 0.8.0 dazu: ein Testton aus Lüfterrauschen und einer nachgebildeten Stimme. Im Export war das Rauschen
in den Pausen um 21 dB leiser, die Stimme gleich laut, der Ton nicht verschoben und an Schnittstellen ohne Lücke.
Mit echter Stimme und echtem Lüfter konnte ich es nicht anhören, das Urteil über den Klang hast du.
Nur auf deinem Rechner prüfbar: die Bildschirmauswahl über das Portal von Hyprland, echte Kamera und
Mikrofone, Kodieren auf der Grafikkarte, Klickdaten, das Folgen mit echten Monitoren, das Eintragen der
Tastenkürzel in Hyprland und das Verschieben des kleinen Aufnahmefensters.
