# karez brand assets: prompts for Gemini

_Internal note, 2026-11-09. Paste-ready prompts for Gemini's image generation to make the karez
logo, the GitHub social preview and the icon set, in the same visual family as simoom and
Cameleer. First drafts of the key visual, icon, logo and social preview are saved in
`reference/`; the sections below say what to fix in each._

## How to use this file

1. Open **one new Gemini chat** and keep every asset in it, so the style stays consistent.
2. Paste **Part A (the brief)** first. Attach the three simoom images from
   `E:\Work\DataHQ\Code\simoom\assets\` (`simoom-logo.png`, `simoom-social.png`,
   `simoom-icon.png`) as style references and say: "These are the sibling project's assets. Match
   their look, not their content."
3. Paste the prompts in **Part B** one at a time, in order: icon, logo, social preview, then the
   icon variants. Ask for 3 variations of the first icon and pick one. Then say "use this mark for
   everything that follows" and attach the winner.
4. Save the picks in `assets/` (the same names as simoom: `karez-icon.png`, `karez-logo.png`,
   `karez-social.png`).

Things Gemini gets wrong, and how to handle them:

- **Spelling.** It often garbles text. The only words that should appear in any image are
  `KAREZ` and the one short tagline given in each prompt. If a letter is wrong, ask for a
  regeneration with "spell it exactly K-A-R-E-Z". If it still fails, generate the art without
  text and add the words in an editor.
- **Sizes.** Gemini does not guarantee exact pixel sizes. Ask for the aspect ratio, then resize.
  GitHub wants the social preview at 1280 x 640 (2:1), under 1 MB, with the important content at
  least 40 px from every edge. Export as PNG and compress.
- **The watermark.** Gemini puts a small sparkle in the bottom-right corner (simoom's images have
  it). Crop it out of the final files.
- **No performance numbers in the art.** The plan has not measured anything yet. Do not put
  claims like "10x faster" in any image.

---

## The key visual (already generated)

[`reference/karez-key-visual.webp`](reference/karez-key-visual.webp) is the first image Gemini
produced, and it is the direction to build on. It shows five sand dunes, each with a vertical
shaft dropping into a long, gently sloping tunnel. A cyan stream of small data blocks runs down
the tunnel and fans out into fine circuit lines at the low end, with `KAREZ` and a tagline
underneath. It already matches the concept and the simoom palette. **Attach it to every later
prompt** so the icon, logo and social image share its dunes, stream and glow.

Four things to fix before it becomes the official image:

| issue | why it matters | fix (prompt B5 does all four) |
|---|---|---|
| The dune labels read JDBC, ODBC, ADBC, **R2DBC**, SQL | The plan does not ship an R2DBC driver. It only ports R2DBC's test suite (plan.md §3). "SQL" is vague. | Label the shafts **JDBC, ODBC, ADBC, Python, Arrow**, which are the exports the plan commits to. |
| The tagline says "Open-source data bridge" | "Bridge" fights the channel metaphor, and it omits Rust. | Use **Database connectivity for Rust**. |
| The source end has no database and the Cameleer badge is missing | Without a source, the water starts from nothing. It also lacks the family link that simoom has. | Add a small glowing database cylinder at the high, left end of the tunnel, and the Cameleer camel badge in the lower left. |
| The Gemini sparkle sits in the bottom-right corner, and the dune labels are tiny | Both disappear or look odd at 1280 x 640. | Crop the sparkle. Make the labels about 1.5 times larger. |

Also decide whether the final social image keeps the dune labels at all. They add meaning on the
repository page but hurt a link-preview thumbnail, so make one version with labels and one
without.

## The icon (already generated)

[`reference/karez-icon-v1.webp`](reference/karez-icon-v1.webp) is Gemini's first icon, and it
follows prompt B1 closely. It is a bold amber K in a rounded tile with a thin amber border. The
stem is a vertical shaft with a small cap at the top. A glowing cyan stream drops down the stem,
turns through the join, and runs out along the lower arm, where it fans into fine fibre-like
lines that end in small nodes. Three tiny rings on the upper arm are the shafts seen from above.
It reads as a K, as a channel, and as data leaving a fibre, and it matches the key visual's
colours. Use it as the base.

What to fix, mostly for small sizes:

- **The details will vanish below about 64 px.** The shaft cap, the double outlines on every
  stroke, the three tiny rings and the fan of hair-thin lines turn to noise at 32 px. This is
  what prompt B4 is for: a flat, two-colour version with solid strokes, one stream and a short
  fan of three or four lines. Use the full version above 128 px only.
- **The K sits slightly left of centre.** The fan on the right makes the whole mark balance, but
  ask Gemini to centre the K and fan together in the tile.
- **The stem's inner pipe line is hard to read.** Ask for the shaft to be a solid amber bar with
  a small cap, and for the stream to enter it from the top.
- **Crop the Gemini sparkle** in the bottom-right corner.

To get the cleaner version, attach the icon to Gemini and use this:

> Revise the attached karez icon. Keep the rounded tile, the dark navy background, the amber K
> and the glowing cyan stream that drops down the stem and out along the lower arm. Make these
> changes only: (1) make the stem a solid amber bar with a small round cap at the top, with no
> inner pipe line; (2) reduce the fan at the end of the stream to four clean lines, each ending in
> a small round node; (3) keep only one ring on the upper arm; (4) centre the K and the fan
> together in the tile; (5) thicken all strokes by about 20 percent so the mark stays readable
> at 32 px. Leave the bottom-right corner empty and add no watermark and no text.

## The logo (already generated)

[`reference/karez-logo-v1.webp`](reference/karez-logo-v1.webp) is Gemini's first lockup. The icon
sits on the left, `KAREZ` follows in wide-spaced capitals that shade from amber to cyan, and a
glowing line runs from the end of the stream to the right edge with four small ring nodes on it,
so the whole logo is itself a karez with shafts. Faint circuit traces fill the corners, as in
simoom's logo.

What to fix:

- **The tagline says "Fastest".** It reads "Fastest Database connectivity for Rust". Nothing has
  been measured yet, and the plan's hard rule 6 forbids a speed claim that would not survive the
  maintainers of tokio-postgres or pgJDBC rerunning it. The prompt asked for "Database
  connectivity for Rust", so Gemini added the word. Remove it, and also lower-case "database" in
  the middle of the sentence. If the M0 measurement later supports a claim, change the tagline
  then.
- **The mark and the wordmark both start with K.** It reads as "K KAREZ". Either accept it as a
  deliberate lockup, as many brands do, or use the stacked version below for places where this
  looks odd.
- **Crop the Gemini sparkle** in the bottom-right corner.

To fix the tagline, attach the logo to Gemini and use this:

> Revise the attached karez logo. Keep everything: the K mark, the amber-to-cyan KAREZ wordmark,
> the glowing line with four ring nodes, the circuit texture and the dark navy background. Change
> only the tagline under the wordmark to exactly: **Database connectivity for Rust**. Remove the
> word "Fastest", and use a capital D only at the start. Leave the bottom-right corner empty and
> add no watermark.

For a stacked version, for the GitHub organisation page, slides and narrow headers:

> Using the attached karez logo, make a **stacked** version, 4:3: the K mark centred at the top,
> the word KAREZ centred below it in the same amber-to-cyan gradient, the tagline "Database
> connectivity for Rust" in small italic pale cyan under that, and the glowing line with four
> ring nodes running across the bottom. Same background and colours. No watermark.

## The social preview (already generated)

[`reference/karez-social-v1.webp`](reference/karez-social-v1.webp) is the draft for GitHub's
repository social image. It is about 2:1, so it resizes cleanly to 1280 x 640. The layout copies
simoom's: the `KAREZ` wordmark with a warm glow at the top left, the tagline under it with
"database connectivity" and "Rust" in cyan, three pill tags, and the Cameleer camel badge at the
bottom left. On the right is the key visual as an isometric slab: five dunes labelled JDBC, ODBC,
ADBC, Python and Arrow, a glowing database cylinder at the source, the stream running down the
tunnel, small cubes of data, and the fan of circuit lines at the low end. It already carries the
label and database fixes from prompt B5.

What to fix:

- **The badge caption has two lines that say the same thing.** It reads "The database engine of
  Cameleer" and then "The connectivity engine of Cameleer". Keep one. Simoom's caption is "The
  HTTP Engine of Cameleer", so **"The database engine of Cameleer"** matches the family.
- **"Fast" in the tagline is a goal, not a measurement.** It is milder than "Fastest" on the logo
  and it also appears in the README, so it is acceptable as design intent. If you want to hold to
  plan.md's hard rule 6 strictly, use "Small, standards-shaped database connectivity for Rust" until
  the first measurement milestone supports it.
- **The dune labels are small.** At 1280 x 640 and in link-preview thumbnails, "ODBC", "ADBC" and
  "Python" shrink to a few pixels. Make a second version without the labels for link previews.
- **Crop the Gemini sparkle** in the bottom-right corner, and keep content at least 40 px from
  every edge at the final size. The text already clears that.
- **Check the file size.** Export as PNG at 1280 x 640 and keep it under 1 MB. GitHub may
  re-encode large files.

To fix the caption and make the unlabelled version, attach the image to Gemini and use:

> Revise the attached karez social image. Keep everything. Make these changes only: (1) change
> the two-line caption under "Cameleer" to one line reading exactly **The database engine of
> Cameleer**; (2) create a second version of the image with the five dune labels removed, keeping
> the dunes, shafts, stream and everything else the same. Leave the bottom-right corner empty and
> add no watermark.

## Update: the small marks are built in-house, not with Gemini

Gemini would not apply multi-step edits to the K (a second round changed only the stream
thickness and ignored the fan, glow, border and background changes), so the favicon family is
built from vector geometry instead. The K was measured from `reference/karez-k-solid-v1.webp` and
rebuilt in [`scripts/brand/build_icons.py`](../../scripts/brand/build_icons.py). It produces the
colour, white, dark and amber marks, the tile, both avatars, and the favicon and PNG sizes, all with
true transparent backgrounds. See [`assets/README.md`](../../assets/README.md).

So **prompts for the favicon, the mono marks and the avatar are no longer needed**, and the
"what to do, in order" list in the next section is superseded. What still needs Gemini:

- the logo tagline fix, in the logo section below;
- the social preview caption fix and the unlabelled version, in the social preview section;
- the optional extras in B6.

## The icon set (already generated)

[`reference/karez-icon-set-v1.webp`](reference/karez-icon-set-v1.webp) is Gemini's answer to
prompt B4. It is **one contact sheet, not separate files**, with six tiles: a 64 px style
favicon, a white mark, a navy mark on light grey, a single-colour amber K, and two circular
avatars. Each tile has a caption in the image and a navy or grey background instead of a
transparent one, so none of it can be used as it is. The tiles are also not what B4 asked for in
two places: the favicon kept the shaft cap and three small rings, and the avatars dropped the
cyan.

**Size test (2026-11-09).** I cropped each tile and shrank it to 64, 32 and 16 px:

| tile | 64 px | 32 px | 16 px |
|---|---|---|---|
| favicon, amber and cyan | clear | K and cyan stream both read | K reads, stream is a few cyan pixels |
| white mark | clear | double strokes start to merge | a grey K with fuzzy edges |
| navy mark on light grey | clear, but a grey smear on the stem | fuzzy | fuzzy, the smear makes the stem look dirty |
| single-colour amber K | clear | thin double lines | a K made of hairlines, readable but weak |
| circular avatar | clear | ring and K both thin | the K becomes a blob inside the ring |

What the test says:

- **The colour favicon is the best small mark.** The cyan stream is what keeps it recognisable.
  Use it as the starting point.
- **The root problem is the double outline.** Every K stroke is drawn as two parallel lines, to
  suggest a hollow channel. That is the right look at 128 px and above, but below 32 px the two
  lines merge into one fuzzy line. The small marks need **solid** strokes.
- **The mono light mark is not truly monochrome.** It has a faded grey streak on the stem.
- **The avatars are the weakest.** The K is small inside the ring and its strokes are thin, and
  GitHub shows an organisation avatar at about 40 px. They also lost the cyan. They need a bolder,
  larger K.

What to do, in order:

1. Get a solid-fill master of the K, then derive everything from it:

> Using the attached karez icon, draw a **solid-fill** version of the K mark for small sizes.
> Each stroke is a single solid bar, with no double outlines and no inner lines. The stem is a solid
> amber bar with a small round cap at the top. The upper and lower arms are solid amber. A single
> thick cyan stream runs down the stem, turns at the join and exits along the lower arm, ending in
> a short fan of three thick lines with round nodes. No small rings and no glow. Flat colours on
> a transparent background, centred, with equal margin on all sides. No text, no caption, no
> watermark.

2. Ask for each variant as its **own image on a transparent background**, one prompt per image,
   not a sheet: the colour favicon, the white mark, the near-black mark (fully flat, no grey
   gradient), the amber mark, and the avatar. For the avatar, say "the K fills about 65 percent of
   the circle, with thick strokes, and the stream is cyan".
3. If Gemini cannot produce a transparent background, remove it afterwards with an editor.
4. **Convert the solid master to SVG** with a tracer such as Inkscape's "Trace Bitmap" or
   Illustrator's Image Trace. A K with flat colours traces cleanly. Export the favicon sizes
   (16, 32, 48, 180 and 512 px) from the SVG, not from the raster, so they stay sharp.

## Part A: the brief (paste this first)

> I need a brand identity for an open-source software project called **karez**. Please read this
> brief fully, then wait for my next message. Do not generate anything yet.
>
> **The name.** A *karez* (also written *kariz*, *kahriz*; in Arabic and Persian contexts *qanat*;
> in Oman, *falaj*) is an ancient underground water channel. A tunnel is dug with a very gentle
> slope from a deep aquifer, usually at the foot of hills, out to a village and its fields.
> Water flows the whole way by gravity alone, with no pumps and no moving parts, and it does not
> evaporate in the desert sun because it runs underground. A line of vertical shafts, spaced at
> regular intervals along the tunnel and visible from above as a row of small dark circles in the
> sand, lets people dig it, inspect it and keep it clean. The system is about 3,000 years old, was
> built across Persia, Central Asia, the Middle East and North Africa, and the Persian qanats are
> a UNESCO World Heritage Site. It is a symbol of patient, efficient, dependable engineering that
> moves something precious a long way using almost no energy.
>
> **The project.** karez is a software library, written in Rust, that connects programs to
> databases. It plays the role that JDBC and ODBC play, and it follows those standards plus ADBC.
> One library speaks to PostgreSQL, MySQL and MariaDB, SQL Server and SQLite, and it can be used
> from Python, Java, Go, .NET, R and business-intelligence tools. Its goal is to be fast, small
> and safe: data is decoded in large batches straight from the network into columns, instead of
> one row at a time, and connections send many requests down the line without waiting. It has
> almost no dependencies and a tiny size.
>
> **Why this name.** The database is the aquifer: a deep, rich source. The applications, the
> dashboards and the data tools are the village and the fields. karez is the channel between
> them. Like the ancient one, it moves the precious thing (data) a long way, steadily and in
> bulk, with no pumping (no wasted work, no copying, no heavy runtime), and it has regular access
> shafts (metadata, observability, safety checks) along the way. Today's common alternative is
> carrying water one bucket at a time, which is how row-by-row database drivers work. The channel
> replaces the buckets.
>
> **The family.** karez belongs to **Cameleer**, an open-source family of Rust software with a
> desert and camel naming theme. Its sibling **simoom** (a hot desert wind) is an HTTP server
> engine. The attached images are simoom's logo, social image and icon. Karez must look like a
> sibling: the same dark, technical, premium look, but its own subject. Where simoom is wind and
> fire, karez is **water, stillness and flow**.
>
> **Visual language.**
> - Background: deep navy to near-black, `#0A1020` to `#0F1A2E`, with a very faint engineering
>   grid or circuit-trace texture, as in the attached images.
> - Two accent colours. **Amber and gold** (`#F59E0B` to `#FFD08A`) stand for sand, the desert
>   and the Cameleer family. **Cyan and electric blue** (`#22D3EE` to `#38BDF8`) stand for water
>   and data. Water is the hero colour here; amber supports it. Use soft glows, not flat fills.
> - Fine circuit lines, small nodes, subtle gears or dots may appear as texture, as in simoom, but
>   sparingly. The main shape must stay simple and read at 32 px.
> - Mood: calm, precise, ancient made modern. No cartoon camels, no clip-art palm trees, no
>   pipes with taps, no drops that look like a plumbing or water-company logo, no stock-photo
>   look, no human figures, no flags or national symbols.
> - Typography: a clean geometric sans-serif in all caps for `KAREZ`, slightly wide letter
>   spacing, like the simoom wordmark but calmer and without the flame.
> - Text rule: the only words allowed in any image are `KAREZ` and the exact tagline I give you.
>   Spell it exactly K-A-R-E-Z.
>
> **Key visual idea.** A side cross-section of a karez: a dark rounded hillside or desert dune in
> amber; inside it a long, gently descending tunnel carrying a glowing cyan stream; vertical
> shafts rising from the tunnel to the surface at regular intervals, each topped with a small
> ring of sand; at the low end the water emerges into a small bright oasis or a field of fine
> lines. Seen from above, the shafts are a row of dots in a line. The stream can break into
> small, neat, luminous blocks or ribbons (columns of data) to hint at batches. Keep it abstract
> and graphic.
>
> Confirm that you understand, and summarise the idea in two sentences. Then wait.

---

## Part B: the prompts (paste one at a time)

### B1. App icon (square, the primary mark)

Aspect ratio 1:1. Target 2048 x 2048, same format as `simoom-icon.png`.

> Create the **karez app icon**, a square 1:1 image in the same format as the attached simoom
> icon: a rounded-square tile with a thin amber border on a dark navy background, one bold central
> symbol, and a small detail in a corner.
>
> The central symbol is a **bold, simple mark that combines the letter K with a karez**: the
> vertical stem of the K is a vertical access shaft, and the two arms of the K are the tunnel,
> descending in one gentle, smooth diagonal with a luminous cyan stream running through the lower
> arm and spilling out to a small bright point at the end. Two or three tiny circles on the
> upper arm are the shafts seen from above. The K is amber-gold where it is sand and rock, and
> cyan where it carries water. It must be recognisable as a K, and it must remain readable if the
> image is shrunk to 32 px, so use thick strokes, few details and strong contrast. Very faint
> circuit traces in the background tile only.
>
> Corner detail: leave the bottom-right corner empty. No text at all in this image. No watermark.
> Give me **three different variations** of the symbol, with the same tile and colours.

### B2. Logo (wide, with wordmark)

Aspect ratio about 16:9. Target 2752 x 1536, same format as `simoom-logo.png`.

> Using the icon symbol I selected, create the **karez logo lockup**, wide, on the same dark
> navy engineering-grid background as the attached simoom logo.
>
> The symbol sits on the left at about one third of the width. To its right, the word **KAREZ**
> in large, clean, wide-spaced geometric sans-serif capitals, with a gradient from amber-gold on
> the left to cyan on the right, as if sand turned into water along the word. Under the word, in
> small italic text in pale cyan, the tagline: **Database connectivity for Rust**.
>
> Behind and around the wordmark, add a thin, glowing cyan line that runs left to right along the
> baseline with a few small ring-shaped nodes on it at regular intervals, so it reads as a
> channel with shafts. A few fine circuit traces in amber and cyan fade in towards the lower left
> and upper right, as in the simoom logo, but keep the centre clear. No other text. No watermark.

### B3. GitHub social preview

Aspect ratio 2:1. Target 2912 x 1440, then resize to 1280 x 640 for GitHub.

> Create the **karez GitHub social preview image**, 2:1. Use the attached karez key visual (the
> glowing dunes and the cyan stream) as the illustration on the right. Use the same layout as the attached
> simoom social image: text on the left over a dark navy field, a large atmospheric illustration
> on the right.
>
> Left side, top to bottom:
> - The word **KAREZ** in large, bold, slightly italic capitals with a warm amber glow.
> - Under it, in light grey: "Fast, small, standards-shaped" on the first line, and in bold
>   "database connectivity for Rust" on the second.
> - Three rounded pill tags with thin amber outlines on a dark brown fill, reading exactly:
>   "PostgreSQL · MySQL · SQL Server · SQLite", then "JDBC · ODBC · ADBC", then "Arrow-native".
> - Bottom left: a small round badge with a camel silhouette on a dune, exactly like the one in
>   the simoom image, with the text "Cameleer" in white and below it "The database engine of
>   Cameleer" in grey.
>
> Right side: a large, dramatic illustration of a **karez cross-section**. A dune and hillside
> of glowing amber sand, with a long gently sloping tunnel carrying a bright cyan stream of
> light. The stream breaks into neat, glowing rectangular blocks (columns of data). Vertical
> shafts rise from the tunnel to the surface at regular intervals, and a dotted line of shaft
> rings is visible on the sand surface. At the low end, the water fans out into a small bright
> oasis of fine cyan lines and circuit traces. Floating sand grains and soft glows, as in the
> simoom image. Keep the important content at least 5% away from every edge. No watermark.

### B4. Icon set (small sizes, simplified)

> Using the selected karez symbol, give me a set of **simplified icons**, each on its own, on a
> transparent or flat navy background, centred, with generous margin:
> 1. **Favicon, 64 px style:** the K-and-channel symbol reduced to a flat, two-colour version (amber
>    and cyan), with no glow and no fine lines. It must stay readable at 16 px.
> 2. **Monochrome mark, white** on a transparent background, for dark README banners.
> 3. **Monochrome mark, near-black** on a transparent background, for light pages and print.
> 4. **Flat single-colour mark in amber** for the docs site header.
> 5. **Circular avatar version** of the symbol, for the GitHub organisation or a package registry:
>    the symbol centred in a navy circle with a thin amber ring.
>
> No text and no watermark in any of them.

### B5. Revise the key visual (run this on the attached `karez-key-visual.webp`)

Aspect ratio 16:9, as the original.

> Revise the attached karez image. Keep everything that works: the dark navy background, the five
> amber dunes, the vertical shafts, the sloping tunnel, the glowing cyan stream of small data
> blocks and the fan of fine circuit lines at the low end. Make these changes only:
>
> 1. Change the five small dune labels, left to right, to exactly: **JDBC**, **ODBC**, **ADBC**,
>    **Python**, **Arrow**. Make them about 1.5 times larger, in the same pale amber.
> 2. Change the tagline under KAREZ to exactly: **Database connectivity for Rust**.
> 3. At the high, left end of the tunnel, add a small glowing cyan database cylinder icon, where
>    the stream begins, so the water visibly comes from a database.
> 4. In the lower left corner, add a small round badge with a camel silhouette on a dune, with
>    the word "Cameleer" next to it in light grey, as in the attached simoom social image.
> 5. Leave the bottom-right corner empty and add no watermark.
>
> Do not change the colours, the style or the composition otherwise. Spell KAREZ exactly.

### B6. Optional extras

> Create three small supporting graphics in the same style, each as a separate image without text:
> 1. A **horizontal divider banner**, 4:1, of a long cyan channel running through a field of amber
>    sand with small shaft rings along it, for the top of the docs site.
> 2. A **tile pattern**, seamless and square, of faint cyan channel lines and shaft dots on navy,
>    for page backgrounds.
> 3. A **file-type or driver icon set**, six square icons in one style, each with a simple glyph
>    on a navy rounded tile: a database cylinder, a lightning bolt for speed, a shield for
>    security, a feather for small size, a column grid for Arrow, and a plug for drivers. Cyan
>    glyphs with amber accents. No text.

---

## What each asset is for

| asset | file name | where it is used | size |
|---|---|---|---|
| App icon | `assets/karez-icon.png` | package registries, docs site, avatar source | 2048 x 2048 |
| Logo | `assets/karez-logo.png` | README header, docs site, slides | 2752 x 1536 |
| Social preview | `assets/karez-social.png` | GitHub repository settings, link previews | 1280 x 640, under 1 MB |
| Favicon and mono marks | `assets/favicon-*.png`, `assets/mark-*.svg` | website, crates.io and docs pages, print | 16 to 512 px |

## Consistency notes for the family

| | simoom | **karez** |
|---|---|---|
| Meaning | a hot, fast desert wind | an underground, gravity-fed water channel |
| Element | wind and fire | water and flow |
| Hero colour | amber and orange | cyan, with amber as sand |
| Symbol | an S made of a flame | a K made of a shaft and a channel |
| Role in the family | the HTTP engine | the database engine |
| Shared | navy ground, circuit texture, the Cameleer camel badge | pill tags, a one-line tagline |
