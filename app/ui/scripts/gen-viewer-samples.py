#!/usr/bin/env python3
"""Sample files for the in-app viewers (public/mock/v/): one of each kind the viewers read,
hand-written and deterministic (fixed timestamps), CC0. Run by gen-mock-assets.sh; the
JPEG/WebP samples come from ImageMagick there."""
import io, os, sys, tarfile, zipfile, gzip

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), '..', 'public', 'mock', 'v')
os.makedirs(out, exist_ok=True)
W = lambda name, data: open(os.path.join(out, name), 'wb').write(data if isinstance(data, bytes) else data.encode('utf-8'))
T = (2024, 5, 12, 9, 30, 0)


def z(name, files, compress=zipfile.ZIP_DEFLATED):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, 'w', compress) as zf:
        for n, d in files:
            zi = zipfile.ZipInfo(n, T)
            zi.compress_type = zipfile.ZIP_STORED if n.endswith('/') or n == 'mimetype' else compress
            zf.writestr(zi, d)
    W(name, buf.getvalue())


# ---- code -----------------------------------------------------------------------------
W('main.rs', '''//! Conta le parole di un file.
use std::collections::HashMap;

/// Le parole più frequenti, in ordine.
fn top_words(text: &str, n: usize) -> Vec<(&str, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for w in text.split_whitespace() {
        *counts.entry(w).or_insert(0) += 1;
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(n);
    v
}

fn main() {
    let text = "il gatto e il cane e il topo";
    for (w, c) in top_words(text, 3) {
        println!("{w}: {c}"); // stampa
    }
    let _ok = true && 0x1F > 10;
}
''')
W('budget.ts', '''// Somma le spese per categoria.
export interface Spesa {
  categoria: string;
  importo: number;
}

export function totali(spese: Spesa[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const s of spese) {
    out.set(s.categoria, (out.get(s.categoria) ?? 0) + s.importo);
  }
  return out;
}

const esempio = totali([{ categoria: 'casa', importo: 120.5 }, { categoria: 'auto', importo: 40 }]);
console.log(`categorie: ${esempio.size}`, null, undefined);
''')
W('backup.py', '''#!/usr/bin/env python3
"""Copia i file nuovi nella cartella di backup."""
import shutil
from pathlib import Path

SORGENTE = Path("~/Documenti").expanduser()


@staticmethod
def copia(dest: Path, dry_run: bool = False) -> int:
    n = 0
    for f in SORGENTE.rglob("*"):
        if f.is_file() and not (dest / f.name).exists():
            if not dry_run:
                shutil.copy2(f, dest)  # mantiene le date
            n += 1
    return n


if __name__ == "__main__":
    print(copia(Path("/mnt/backup"), dry_run=True), None)
''')
W('impostazioni.json', '''{
  "lingua": "it",
  "tema": "scuro",
  "blocco_automatico_min": 15,
  "unità": ["SH2RE", "Archivio"],
  "anteprime": true,
  "ultimo_backup": null
}
''')
W('stile.css', '''/* carta e inchiostro */
:root {
  --carta: #f3efe6;
  --inchiostro: #1d1b18;
}
.foglio > p:first-child {
  margin: 0 0 1.2em;
  color: var(--inchiostro);
}
@media (prefers-color-scheme: dark) {
  .foglio { background: #22201d; }
}
''')
W('config.toml', '''# Configurazione del servizio
[server]
porta = 8080
host = "127.0.0.1"
attivo = true

[backup]
cartella = "/mnt/backup"
giorni = [1, 3, 5]
''')

# ---- text in other encodings, CSV --------------------------------------------------------
lettera = 'Caro Mario,\nti scrivo da Città di Castello: qui è già autunno, e il caffè è più buono.\nPerché non vieni a trovarci? Saluti a tutti — anche a zia Lucìa.\n\nCiao,\nAnna\n'
W('lettera-1252.txt', lettera.encode('cp1252'))
W('lettera-utf16.txt', b'\xff\xfe' + lettera.encode('utf-16-le'))
W('spese.csv', 'Data;Descrizione;Categoria;Importo\n2024-05-02;Bolletta luce;Casa;84,20\n2024-05-06;"Spesa, supermercato";Alimentari;63,15\n2024-05-11;Benzina;Auto;55,00\n2024-05-18;"Cena da ""Gigi""";Svago;48,50\n2024-05-27;Assicurazione;Auto;312,00\n')

# ---- Markdown (raw HTML, a remote image and a script link must stay inert) ---------------------
W('guida.md', '''# Guida al volume

Questo volume tiene **documenti di casa**, *foto* e ~~vecchie~~ note. Vedi anche il sito
<https://nicevault.app> o [la pagina di aiuto](https://example.org/aiuto "Aiuto").

## Cosa fare se perdi la chiavetta

1. Niente panico: senza password il contenuto è illeggibile.
2. Usa la **busta in cassaforte** (la password di riserva).
3. Crea un nuovo volume e ripristina il backup.

- [x] Backup di maggio
- [ ] Backup di giugno
- Elenco puntato con `codice in linea`

> Le password restano qui dentro,
> non nelle email.

## Comandi utili

```sh
# controlla lo spazio libero
df -h /run/media/$USER
echo "fatto"
```

| Cosa | Dove | Quanto |
|:-----|:----:|-------:|
| Foto | Foto/ | 2,1 GB |
| Documenti | Lavoro/ | 340 MB |

![Una foto remota](https://example.org/tracker.png)

<script>alert("questo non deve girare")</script>
<b>HTML grezzo mostrato come testo</b> e un [link pericoloso](javascript:alert(1)).

---

Fine della guida.
''')

# ---- archives -------------------------------------------------------------------------------
z('archivio.zip', [
    ('Documenti/', b''),
    ('Documenti/contratto.txt', 'Contratto di locazione\n' * 40),
    ('Documenti/ricevute/maggio.txt', 'Ricevuta maggio\n' * 10),
    ('Foto/vacanze.txt', b'\x00' * 2048),
    ('Leggimi.md', '# Archivio 2019\n\nTutto quello che c\'era sul vecchio PC.\n'),
    ('note à accentate.txt', 'nome con accenti\n'),
])
tb = io.BytesIO()
with tarfile.open(fileobj=tb, mode='w', format=tarfile.USTAR_FORMAT) as tf:
    for n, d in [('backup/', None), ('backup/contatti.vcf', b'BEGIN:VCARD\nFN:Anna\nEND:VCARD\n' * 5), ('backup/foto/', None), ('backup/foto/img_001.txt', b'x' * 3000), ('backup/note.txt', b'note\n')]:
        ti = tarfile.TarInfo(n.rstrip('/'))
        ti.mtime = 1715506200
        if d is None:
            ti.type = tarfile.DIRTYPE
            tf.addfile(ti)
        else:
            ti.size = len(d)
            tf.addfile(ti, io.BytesIO(d))
W('backup.tar', tb.getvalue())
# served as .bin: dev servers send .gz with Content-Encoding
W('backup-tar-gz.bin', gzip.compress(tb.getvalue(), mtime=0))

# ---- Office Open XML / OpenDocument ------------------------------------------------------------
CT = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
core = CT + '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{t}</dc:title><dc:creator>Anna Rossi</dc:creator><dcterms:modified xsi:type="dcterms:W3CDTF">2024-05-12T09:30:00Z</dcterms:modified></cp:coreProperties>'
app = CT + '<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Microsoft Office Word</Application></Properties>'
rels = CT + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="{t}"/></Relationships>'
W_NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'


def wp(text, style=None):
    ppr = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>' if style else ''
    return f'<w:p>{ppr}<w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>'


doc = CT + f'<w:document {W_NS}><w:body>' + ''.join([
    wp('Verbale della riunione', 'Title'),
    wp('Presenti', 'Heading1'),
    wp('Anna, Marco, Giulia e il condominio al completo.'),
    wp('Decisioni', 'Heading1'),
    wp('Si approva il preventivo per il tetto: 12.400 € in due rate.'),
    '<w:p><w:r><w:t>Prossima riunione:</w:t></w:r><w:r><w:tab/><w:t>giugno.</w:t></w:r></w:p>',
]) + '</w:body></w:document>'
z('verbale.docx', [
    ('[Content_Types].xml', CT + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>'),
    ('_rels/.rels', rels.format(t='word/document.xml')),
    ('word/document.xml', doc),
    ('docProps/core.xml', core.format(t='Verbale della riunione')),
    ('docProps/app.xml', app),
])
S_NS = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"'
shared = ['Mese', 'Entrate', 'Uscite', 'Gennaio', 'Febbraio', 'Marzo', 'Totale']
sst = CT + f'<sst {S_NS} count="{len(shared)}" uniqueCount="{len(shared)}">' + ''.join(f'<si><t>{s}</t></si>' for s in shared) + '</sst>'
def c(ref, v, s=False):
    return f'<c r="{ref}" t="s"><v>{v}</v></c>' if s else f'<c r="{ref}"><v>{v}</v></c>'
rows = [[c('A1', 0, 1), c('B1', 1, 1), c('C1', 2, 1)], [c('A2', 3, 1), c('B2', 2400), c('C2', 1980.5)], [c('A3', 4, 1), c('B3', 2400), c('C3', 2210)], [c('A4', 5, 1), c('B4', 2650), c('C4', 1875.25)], [c('A6', 6, 1), c('B6', 7450), c('C6', 6065.75)]]
rownums = [1, 2, 3, 4, 6]
sheet = CT + f'<worksheet {S_NS}><sheetData>' + ''.join(f'<row r="{n}">{"".join(r)}</row>' for n, r in zip(rownums, rows)) + '</sheetData></worksheet>'
z('bilancio.xlsx', [
    ('[Content_Types].xml', CT + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>'),
    ('_rels/.rels', rels.format(t='xl/workbook.xml')),
    ('xl/workbook.xml', CT + f'<workbook {S_NS} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Primo trimestre" sheetId="1" r:id="rId1"/><sheet name="Note" sheetId="2" r:id="rId2"/></sheets></workbook>'),
    ('xl/sharedStrings.xml', sst),
    ('xl/worksheets/sheet1.xml', sheet),
    ('docProps/core.xml', core.format(t='Bilancio di casa')),
])
A_NS = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"'
def slide(lines):
    return CT + f'<p:sld {A_NS}><p:cSld><p:spTree>' + ''.join(f'<p:sp><p:txBody><a:p><a:r><a:t>{l}</a:t></a:r></a:p></p:txBody></p:sp>' for l in lines) + '</p:spTree></p:cSld></p:sld>'
z('presentazione.pptx', [
    ('[Content_Types].xml', CT + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>'),
    ('_rels/.rels', rels.format(t='ppt/presentation.xml')),
    ('ppt/presentation.xml', CT + f'<p:presentation {A_NS}/>'),
    ('ppt/slides/slide1.xml', slide(['Risultati del terzo trimestre', 'Ricavi in crescita del 12%'])),
    ('ppt/slides/slide2.xml', slide(['Prossimi passi', 'Nuovo magazzino a settembre', 'Due assunzioni'])),
    ('ppt/slides/slide10.xml', slide(['Domande?'])),
    ('docProps/core.xml', core.format(t='Q3')),
])
ODF = 'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/"'
z('lettera.odt', [
    ('mimetype', 'application/vnd.oasis.opendocument.text'),
    ('content.xml', CT + f'<office:document-content {ODF}><office:body><office:text><text:h text:outline-level="1">Lettera al condominio</text:h><text:p>Gentile amministratore,</text:p><text:p>le scrivo per<text:s text:c="2"/>segnalare il guasto all\'ascensore.<text:line-break/>Distinti saluti.</text:p></office:text></office:body></office:document-content>'),
    ('meta.xml', CT + f'<office:document-meta {ODF}><office:meta><dc:title>Lettera al condominio</dc:title><meta:initial-creator>Anna Rossi</meta:initial-creator><dc:date>2024-05-12T09:30:00</dc:date><meta:generator>LibreOffice/7.6</meta:generator></office:meta></office:document-meta>'),
])

# ---- not a known type, and an image the webview cannot decode -------------------------------
W('dati.bin', bytes((i * 37 + 11) % 256 for i in range(3000)))
W('scatto.heic', b'\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic' + bytes(range(256)) * 4)
print('viewer samples ->', os.path.abspath(out))
