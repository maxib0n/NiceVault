# Asset sources

Everything the UI ships is bundled (the app runs offline from a USB stick).
The design ("Carta valori", second edition: anthracite Alcantara by default, ivory
paper as a per-volume choice) adds about 0.9 MB to the app (fonts 0.33 MB, material
tiles 0.34 MB, the glyph-decode layer 53 KB minified / 18 KB gzip, everything else is
drawn in code) and 1.7 MB of sample photographs to the in-browser mock (not shipped:
`public/mock/` is dropped from the build).

## Type (`src/assets/fonts/`, SIL Open Font License 1.1)

Each family's license text ships next to its files (`OFL.txt`). Files are the
Google Fonts builds as packaged by Fontsource (`@fontsource-variable/ibm-plex-sans`
5.3.0 for Plex Sans), latin + latin-ext subsets, woff2. No CDN.

| Family | Role | Upstream | License |
|---|---|---|---|
| Newsreader (variable `wght` 200-800, roman + italic; the default optical size, whose strokes stay sturdy) | names and titles, 18 px and up only | https://github.com/productiontype/Newsreader · https://fonts.google.com/specimen/Newsreader (files via @fontsource-variable/newsreader 5.3.0) | OFL 1.1 |
| IBM Plex Sans (variable `wght` 100-700) | all UI text (its figures are tabular by default) | https://github.com/IBM/plex · https://fonts.google.com/specimen/IBM+Plex+Sans | OFL 1.1 |
| IBM Plex Mono (400, 500, 600) | serials, the MRZ, sizes, dates, timecodes, printed labels; the glyphs of the decode | https://github.com/IBM/plex · https://fonts.google.com/specimen/IBM+Plex+Mono | OFL 1.1 |

## Material tiles (`src/assets/textures/`, drawn for NiceVault, CC0)

Generated procedurally (Python + NumPy + Pillow, seeded, so they rebuild
identically): `scripts/gen-textures.py` draws the suede and the paper grain
(`python3 scripts/gen-textures.py`). Periodic by construction (noise synthesised
in the frequency domain on a torus, wrapped strokes, integer thread counts), so
they tile without a seam; band-passed so nothing large enough to be seen repeating
survives (no cloud, no blotch). Mid-grey centred, blended in CSS; drawn at one
texel per device pixel (`--tex-px`), the strength set back on 1x screens (`--tex-k`).

| File | Size | What it is |
|---|---|---|
| `suede-nap.webp` | 512 px, 125 KB | anthracite Alcantara: the nap of a microfibre suede, short fibres leaning away from the top-left light (1-5 px band), sparse fibre tips catching it; the board, the covers, a whisper under the pages |
| `paper-grain.webp` | 512 px, 111 KB | cotton paper for the light theme: a fine grain and 900 short fibres, high-passed so the sheet stays even |
| `felt-board.webp` | 115 KB | the window's board: a dense pressed felt, finer and flatter than the covers' suede (so passports and documents stand off it), same top-left light, no repeat at 1x; procedural (`scripts/gen-textures.py`), CC0 |
| `cloth-weave.webp` | 512 px, 102 KB | plain-weave book cloth (176 × 152 threads, slubs, slight waviness) for the covers on paper; drawn at 256 px |
| `*-aNN.webp` | 512 px, ~180 KB each | the tiles above as they print: the strength (NN %, per `--tex-k`) baked into the alpha, laid in a surface's own background (`scripts/gen-textures.py baked`) |

## Drawn in code (no image assets)

- Guilloché (rosettes cut by two "lathes", frames with corner medallions, running
  bands, the split-fountain security tint), microtext rules, security fibres (paper
  only), the watermark, saddle stitching (`src/ornament/`, canvas at device
  resolution, deterministic). On anthracite the same plates are pressed in (blind
  deboss, inked in the groove) instead of printed raised; the watermark becomes a
  relief pressed into the material.
- Emblem engraving: monograms hatched and outlined, lucide icons cut as double
  hairlines with hatching, user pictures turned into a line-screen engraving
  (`src/components/Emblem.svelte`, `src/ornament/engrave.ts`).
- Foil stripe (CSS gradients and an inline SVG motif), wax seals (SVG), envelopes,
  prints, sheets, keys, fields and stitched seams (CSS).
- `public/favicon.svg` and the app icon (an SVG master and its 1024 px PNG, with `src-tauri/icons/*`
  generated from them with `pnpm tauri icon`): original, CC0.

## Vendored code

- `src/vendor/ascii-reveal/` — the framework-agnostic core (`src/core/*.ts`; one local change in
  `engine.ts`, marked `NiceVault:` — a run's clock starts at its first drawn frame, and the frame
  watchdog ignores single stalls and degrades for 3 s instead of 10)
  of **ascii-reveal** 0.3.0 by Quan Hoang, MIT (`LICENSE` beside it),
  https://github.com/quanhoangindex/ascii-reveal (commit 94b02d7). Copied, not installed:
  no npm dependency. Audited: no network access, no eval. Used through thin Svelte
  actions in `src/lib/reveal.ts` (the glyph decode); its loading mascots are unused.

Icons: `@lucide/svelte` (ISC), one set at one stroke weight; emblem icons are the
same lucide glyphs, engraved.

## Mock photos (`public/mock/photo-01..14.jpg`, CC0, in-browser mock only)

Unsplash uploads of the CC0 era, mirrored on Wikimedia Commons; resized to 960 px,
re-encoded (quality 80, metadata stripped). They replace the earlier procedural
placeholders so the file view is judged with real photographs.

| File | Photograph | Author | Page |
|---|---|---|---|
| photo-01.jpg | Clear lake in snowy mountains | Chris Leipelt | https://commons.wikimedia.org/wiki/File%3AClear_lake_in_snowy_mountains_%28Unsplash%29.jpg |
| photo-02.jpg | Alone in the unspoilt wilderness | David Marcu | https://commons.wikimedia.org/wiki/File%3AAlone_in_the_unspoilt_wilderness_%28Unsplash%29.jpg |
| photo-03.jpg | Flat mountain over a lake | Cameron Kirby | https://commons.wikimedia.org/wiki/File%3AFlat_mountain_over_a_lake_%28Unsplash%29.jpg |
| photo-04.jpg | Clouds mirrored in a mountain lake | Ales Krivec | https://commons.wikimedia.org/wiki/File%3AClouds_mirrored_in_a_mountain_lake_%28Unsplash%29.jpg |
| photo-05.jpg | A black forest walk | Nicolas Picard | https://commons.wikimedia.org/wiki/File%3AA_black_forest_walk_%28Unsplash%29.jpg |
| photo-06.jpg | Brunette woman portrait | Christopher Campbell | https://commons.wikimedia.org/wiki/File%3ABrunette_woman_portrait_%28Unsplash%29.jpg |
| photo-07.jpg | Busy New York City restaurant | Roman Arkhipov | https://commons.wikimedia.org/wiki/File%3ABusy_New_York_City_restaurant_%28Unsplash%29.jpg |
| photo-08.jpg | Blurred woman with bracelet | Thais Muniz | https://commons.wikimedia.org/wiki/File%3ABlurred_woman_with_bracelet_%28Unsplash%29.jpg |
| photo-09.jpg | Dancing in a rocky plain | Jessica Polar | https://commons.wikimedia.org/wiki/File%3ADancing_in_a_rocky_plain_%28Unsplash%29.jpg |
| photo-10.jpg | Beach by the pier | Dakota Corbin | https://commons.wikimedia.org/wiki/File%3ABeach_by_the_pier_%28Unsplash%29.jpg |
| photo-11.jpg | Bondi Beach | Annie Spratt | https://commons.wikimedia.org/wiki/File%3ABondi_Beach_%28Unsplash%29.jpg |
| photo-12.jpg | Cat | Marko Blažević | https://commons.wikimedia.org/wiki/File%3ACat_%28Unsplash%29.jpg |
| photo-13.jpg | Brown-eyed cat | Chinda Sam | https://commons.wikimedia.org/wiki/File%3ABrown-eyed_cat_%28Unsplash%29.jpg |
| photo-14.jpg | Black-eyed Susan flowers in a meadow | Rick Hinchcliff | https://commons.wikimedia.org/wiki/File%3ABlack-eyed_Susan_flowers_in_a_meadow_%28Unsplash%29.jpg |

The other mock files (`card.png`, `scan.jpg`, `clip.webm`, `tone.ogg`, `document.pdf`, the viewer samples in `v/`,
texts) are generated by `scripts/gen-mock-assets.sh` (CC0).
