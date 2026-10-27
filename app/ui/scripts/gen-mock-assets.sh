#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../public/mock"

magick -size 320x440 xc:'#f3f1ec' -fill '#d9d4c7' -draw 'rectangle 24,40 296,120' \
  -fill '#c9c2b2' -draw 'rectangle 24,150 200,170' -draw 'rectangle 24,190 260,210' \
  -draw 'rectangle 24,230 230,250' -draw 'rectangle 24,270 280,290' -quality 70 scan.jpg
magick -size 428x270 xc:'#dfe7ea' -fill '#b8c8ce' -draw 'roundrectangle 16,16 412,254 14,14' \
  -fill '#8fa6ae' -draw 'rectangle 36,60 150,190' -fill '#6f878f' -draw 'rectangle 170,70 390,86' \
  -draw 'rectangle 170,104 340,118' -draw 'rectangle 170,134 370,148' card.png

ffmpeg -loglevel error -y -f lavfi -i "mandelbrot=s=320x180:rate=24" -t 4 \
  -c:v libvpx-vp9 -b:v 180k -pix_fmt yuv420p clip.webm
ffmpeg -loglevel error -y -f lavfi -i "sine=frequency=330:duration=3" -f lavfi -i "sine=frequency=495:duration=3" \
  -filter_complex "amix=inputs=2,volume=0.4,afade=t=out:st=2.4:d=0.6" -c:a libopus -b:a 24k tone.ogg

python3 - <<'PY'
# Minimal two-page text PDF with a correct xref table.
pages = [
    ["Contratto di locazione", "", "Tra le parti sottoscritte si conviene", "quanto segue.", "", "Art. 1 - Oggetto", "Art. 2 - Durata", "Art. 3 - Canone"],
    ["Art. 4 - Deposito cauzionale", "", "Il conduttore versa al locatore", "una somma pari a tre mensilita.", "", "Letto, confermato e sottoscritto."],
]
objs = []
def add(s): objs.append(s); return len(objs)
font = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
pages_id_placeholder = len(objs) + 1
add("PAGES")
kids = []
for lines in pages:
    y = 760
    ops = ["BT", "/F1 20 Tf", f"72 {y} Td", f"({lines[0]}) Tj", "/F1 12 Tf"]
    for ln in lines[1:]:
        ops.append("0 -22 Td"); ops.append(f"({ln}) Tj")
    ops.append("ET")
    stream = "\n".join(ops)
    c = add(f"<< /Length {len(stream)} >>\nstream\n{stream}\nendstream")
    p = add(f"<< /Type /Page /Parent {pages_id_placeholder} 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 {font} 0 R >> >> /Contents {c} 0 R >>")
    kids.append(p)
objs[pages_id_placeholder-1] = f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {len(kids)} >>"
cat = add(f"<< /Type /Catalog /Pages {pages_id_placeholder} 0 R >>")
out = b"%PDF-1.4\n"; offs = []
for i, o in enumerate(objs, 1):
    offs.append(len(out)); out += f"{i} 0 obj\n{o}\nendobj\n".encode()
x = len(out)
out += f"xref\n0 {len(objs)+1}\n0000000000 65535 f \n".encode()
for o in offs: out += f"{o:010d} 00000 n \n".encode()
out += f"trailer\n<< /Size {len(objs)+1} /Root {cat} 0 R >>\nstartxref\n{x}\n%%EOF\n".encode()
open("document.pdf", "wb").write(out)
PY

cat > notes.md <<'MD'
# Note riunione

- Rivedere il preventivo entro venerdì
- Spostare l'archivio 2019 sulla chiavetta nuova
- Chiedere a Giulia le foto del compleanno

> Le password restano qui dentro, non nelle email.
MD
cat > readme.txt <<'TXT'
Questo volume contiene i documenti di casa.

Per aprirlo serve la password principale oppure quella di riserva
custodita nella busta in cassaforte.

Se la chiavetta si perde, il contenuto resta illeggibile.
TXT
cat > recipe.txt <<'TXT'
Ragù della nonna

500 g di carne macinata mista
1 carota, 1 costa di sedano, 1 cipolla
1 bicchiere di vino rosso
700 g di passata di pomodoro

Soffriggere, rosolare, sfumare, cuocere piano per tre ore.
TXT
printf 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMockKeyNotRealOnlyForTheDemoUi0123456789 utente@portatile\n' > key.pub
cat > LICENSE.txt <<'TXT'
All files in this directory were generated procedurally by scripts/gen-mock-assets.sh
(ImageMagick plasma fractals, ffmpeg test sources, hand-written text) for the
NiceVault in-browser mock. They are dedicated to the public domain under CC0 1.0.
TXT

python3 -I ../../scripts/gen-viewer-samples.py v
magick photo-01.jpg -resize 480x -strip -quality 72 v/ruotata.jpg
python3 -I - v/ruotata.jpg <<'PY'
import struct, sys
p = sys.argv[1]; b = open(p, 'rb').read()
tiff = b'II*\x00' + struct.pack('<I', 8) + struct.pack('<H', 1) + struct.pack('<HHII', 0x0112, 3, 1, 6) + struct.pack('<I', 0)
app1 = b'Exif\x00\x00' + tiff
open(p, 'wb').write(b[:2] + b'\xff\xe1' + struct.pack('>H', len(app1) + 2) + app1 + b[2:])
PY
magick photo-04.jpg -resize 640x -quality 70 v/lago.webp
