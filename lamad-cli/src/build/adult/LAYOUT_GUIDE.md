# Adult study layout guide

**Reference deck:** Estudio 4 — *DIOS BUSCA DISCÍPULOS CONFORME A SU CORAZÓN* (finished `.pptx` in `bible-studies/` after manual QA).  
Use that deck + this guide as the **visual authority** for adult builds. `template/adult/master-template.pptx` is the 62-slide OOXML prototype remapped from Estudio 4 (`scripts/prepare_adult_master_template.py`). When an old prototype disagrees with Estudio 4 or this guide, **follow Estudio 4 + this guide**.

Images may use a **new house style** (AI-generated, cinematic, full-bleed). Typography, colours, and chrome come from the template; only the photo/illustration art changes per study.

---

## HARD rules (every adult deck — never regress)

These are enforced in `lamad-cli` build + QC. Do not “match gold” when gold violates them.

| Rule | Detail |
|------|--------|
| **No `()` on citations** | Image chrome (TEMA / A-B verse under gray line) and **Texto Bíblico** citation titles are plain: `MATEO 6:1-4` — never `(MATEO 6:1-4)`. Strip parens from JSON/`rango`/`cita` on fill. Inline body refs like `(Génesis 16:13)` stay bold with parens (body prose only). |
| **Title = youth style** | No cyan `<a:highlight>` chip behind the study title (or próximo). Black Verlag title on white — strip all `a:highlight` on title/próximo fills. |
| **Text too big → shrink** | If title/verse/diagram text overflows or collides, **decrease font** (TEMA/A-B image titles: 18–32pt; A/B **body** headers `Título 1`: 22–36pt by length, top-anchored, no orphan `1.B -` before a break). Never leave title overlapping the verse under the gray line or climbing off the top of the slide. |
| **Scenic image visible** | After cloning youth section chrome, **remove** the full-bleed black `!!Rectangle`. Normalize pic to one `<a:stretch><a:fillRect/></a:stretch>` full-bleed. QC fails if `!!Rectangle` remains or `<p:pic>` is missing. |
| **Texto Áureo citation line** | Quote on its own paragraph; biblical citation (`1 Corintios 10:33`) on the **next line** (never same line as the end of the quote). No parentheses on that citation. Shrink long Pensamiento/Áureo body text so it fits the SmartArt boxes. |
| **Texto verse box** | Clamp `TextBox 4` so verses sit below the “Texto Bíblico” header (`y ≥ 856357` EMU). Never overlap header. |
| **A/B + intro body count is dynamic** | JSON may have **more or fewer** body paragraphs than the gold template. Builder reuses prototype slides and **duplicates** when needed (same as Lectura/Texto). Never fail with “needs N body slides, got M”. |
| **A/B body panels unified** | Every `1.A`–`3.B` body slide uses the **same** `Título 1` + body box geometry (`x/cx/y/cy` band). Never keep prototype widths — some gold slides have title `cx` wider than the slide (no wrap / text off edge). Two-line header band; tight gap to body. Only the text differs. |
| **Package / Repair** | Empty `<a:stretch />`, wrong notes rels, or stale `app.xml` `<Slides>` after duplication → PowerPoint Repair. Validate + open clean before delivery. |
| **Output path** | Generated decks → repo-root `bible-studies/` only (never `lamad-cli/bible-studies/`). |

---

## Global rules

### 1. Full-bleed images

These slide types use a **single image covering the entire slide** (edge to edge, 16:9):

| Slide type | Image role | Text placement |
|------------|------------|----------------|
| `intro_header` | Scenic full-bleed (e.g. open Bible, landscape) | Centred semi-transparent white box with section title only |
| `tema_header` | Cinematic scene for the theme | **Youth section chrome at bottom-left** — orange bar, title, gray line, verse (same idea as youth; lower because adult titles are longer) |
| `ab_title` | Cinematic scene for the A/B point | **Same youth chrome at bottom-left** (not torn-paper / top bar) |

**Contrast (CLI, automatic):** sample the left ~40% **and** the full frame. Dark band **or** dark cinematic scene with a soft left mist → **white** title + **white** verse. Only true light parchment/wash (bright overall) → navy title/verse. Never leave dark/gray text on a dark mood photo.

Do **not** place body paragraphs on image slides. Image slides carry titles + references only.

### 2. Text must fit — bottom guard rails

Every text-heavy slide has a **hard floor** where content stops. Nothing may sit on or below that line.

| Slide type | Guard rail | Approx. safe height |
|------------|------------|---------------------|
| `intro_body` / `ab_body` | Navy footer bar at bottom of slide | Content ends **above** the footer (~bottom 8–10% of slide is chrome only) |
| `lectura_antifonal` | Bottom of white/blue content panel | Pack whole verses; split slide before next verse crosses the floor |
| `texto_biblico` | Empty black margin at bottom | Leave **~20–25%** of slide height below last verse line |
| `tema_header` | Bottom gradient bar | Title + reference live **inside** the bar (~bottom 20–25% of slide) |

**Packing budget (soft caps — split slide if exceeded):**

| Shape | ~chars per slide / paragraph |
|-------|------------------------------|
| `Marcador de contenido 2` (body) | **360–400** per paragraph; **≤430** hard warn |
| `CuadroTexto 5` (lectura) | **250–300** verse chars |
| `TextBox 4` (texto bíblico) | **250–300** verse chars; split at whole verses |
| Teaching body | Split at `(1)`, `(2)`, `(3)` **and** at sentence ends before the floor |

### 3. Colours & fonts (from reference deck)

| Element | Style |
|---------|--------|
| Title slide | White bg; orange `ESCUELA BIBLICA`; black title; `Base Bíblica:` in black pill |
| Section headers (`Introducción`, body `1.A - …`) | Cyan/blue bar, **white bold** text |
| Body text | Black, justified, serif/sans on **white** panel |
| Footer chrome | **Navy** horizontal bar (body slides) |
| Texto Bíblico header | White centred “Texto Bíblico” on black |
| Texto citation + verse numbers | **Yellow** `#FFFF00`, bold |
| Texto verse body | **White** `#FFFFFF`, regular |
| Tema / A-B image chrome | Youth-style: orange `#E85D04` bar, title + gray line + verse — **bottom-left**, panel **~5.4" wide** (wider than youth ~3.6"), title **22–36pt** by length, verse **22pt** |

Preserve template run colours when filling OOXML — do not flatten to a single style.

### 4. Paragraph structure

Match the reference deck’s paragraph breaks:

- Teaching bodies: new `<a:p>` at each `(1)`, `(2)`, `(3)` marker.
- When the reference uses **two paragraphs** without a numeric marker (e.g. intro slide splitting a long block), follow the **gold/PDF line breaks**, not one dense block.
- Texto Bíblico: one paragraph per citation line, one per verse; split glued `; 3 …` / `, 4 …` mid-string.
- Poetry lines after `porque:` → separate paragraphs (e.g. “Dios resiste…” / “Y da gracia…”).

---

## Slide-type recipes (study 02)

### Title / próximo

- Study number badge, `ESCUELA BIBLICA`, church logo, centred title, `Base Bíblica:` label + citations in black rounded pill.
- `Base Bíblica:` **space after colon**; citations separated with `; ` on one line.
- **No highlight background** on the main title (match youth). Strip `<a:highlight>`.
- **Long titles (HARD):** keep title **grande** when it fits (near gold 115pt). Split long titles into 2–3 explicit lines; then **shrink** (step −4pt, floor 64pt) until the estimated wrapped height stays above Base Bíblica. Expand `CuadroTexto 1` above Base with top anchor. Never cover the citation.

### Enseñanza + Datos generales

- **Datos row text (HARD):** AUTOR / PERSONAJES / FECHA / LUGAR value boxes must sit **vertically centred on the left label banner** for that row (`bodyPr anchor="ctr"` + tighten `y`/`cy` to the banner midline for single-line values). Never leave short text top-aligned in a tall gold placeholder.
- ENSEÑANZA (`CuadroTexto 13`) may stay tall when two lines; still `anchor="ctr"`.

### Lectura antifonal

- Same verse formatting as youth **Lectura Bíblica** (`set_verses` + `VerseKind::Lectura`): red citation/numbers, black body, whole verses only.
- Pack at `VERSE_BUDGET` (**280 chars**, same as youth) after expanding glued verses. If the next whole verse would overflow, **new slide**. Never dump Mateo 6:1–4 onto one slide. Citation only on the first slide of each passage.
- **Colours (match youth Lectura):** citation + verse numbers **red** `#FF0000` bold; verse body **black** `#000000` regular — forced explicitly (never inherit a wrong sample fill). Chapter-boundary markers like `16:1` are verse numbers too (red/bold), not body text.
- Template ships 4 prototype slides (2–5). If more packs are needed, **duplicate** the lectura prototype and insert into the active order (same allocate pattern as youth). Unused prototypes stay on disk but drop out of `sldIdLst`.
- Header `LECTURA ANTIFONAL` + citation with semicolon when the template sample has one.

### Objetivos / Pensamiento / Enseñanza+Datos

- SmartArt / composite slides — keep template diagram layout; replace text only.
- Objetivos: three cascading blue boxes on dark blue background.
- **Pensamiento / Texto Áureo:** fill the three slots correctly (pensamiento, quote, cita) — do **not** use blind “longest N texts” replacement. Citation on its **own line** under the quote; no `()`; shrink body runs if long.

### Introducción

1. **`intro_header`** — full-bleed image + centred translucent box, “INTRODUCCIÓN” only.
2. **`intro_body`** (×N) — blue header bar + white body + logo. Split before the logo floor.
3. **Intro body box (HARD):** `Marcador de contenido 2` fills the **full white band** below the blue header (same width as `Título 1`, top-aligned text). Short paragraphs stay at the **top** of the box — never shrink + centre a tiny box mid-slide.

### Definición y etimología (`definicion`) — **image card design**

**Visual authority:** [`reference/definicion-etimologia-design.png`](reference/definicion-etimologia-design.png).

Do **not** dump definitions as a plain blue-header body slide. Each tema’s definición slide is a **composed 16:9 graphic** (full-bleed) matching this layout:

| Region | Spec |
|--------|------|
| Background | Soft landscape / scenic photo (muted blues–greys), faint white wash under the cards |
| Header | Open-book icon + title `DEFINICIÓN Y ETIMOLOGÍA` (bold navy) + thin navy rule ending in a dot |
| Rows | One horizontal rounded white card **per term** (stack 2–4; if more terms, split across slides) |

**Each row (left → right):**

1. **Left icon** — circular/flat icon that matches the *term* (different every row; invent from the wording)
2. **Chevron / arrow panel** — coloured, pointed right; white bold text = quoted `termino` + optional `referencia`
3. **Vertical grey rule**
4. **Definition body** — black sans text = `texto`
5. **Right icon** — second motif that matches the *explanation* (again, unique per row)

**Row colours** rotate per card (blue → darker blue → teal → purple, etc.) so adjacent rows never share the same chevron colour.

**Icons are content-specific** — do not reuse the sample’s clock/storm/brain icons when the study talks about *justicia*, *modestia*, *humildad*, etc. Generate new icon pairs that fit each termino/texto.

**Build path:** generate `definicion-{tema}.png` (≈1408×768+) with all of the above baked into the image (including title + body text — this is a composed slide, not chrome-over-photo). Swap onto the definición prototype slide’s picture fill / main image shape. Clear leftover text placeholders so they do not double-render.

JSON fields per term (`Definicion`):

| Field | Role |
|-------|------|
| `termino` | Chevron title (quoted on the art) — **verbatim from the printed «Definiciones y etimología» box** |
| `texto` | Definition body — **verbatim from that box** |
| `referencia` | Optional verse/ref **only if printed in that box** |
| `icono_izq` / `icono_der` | Optional motif hints for the image generator |

**HARD — never invent:** If the tema has no «Definiciones y etimología» box on the scan, use `definiciones: []` and **omit** the definición slide. Do not fabricate terms from body prose (Rebelión, Anatema, Unción, Hebrew roots, extra verses, Reina-Valera footers, etc.). Card PNGs must render only the JSON terms.

---

### Tema block (per theme)

```
tema_header          → full-bleed image + youth chrome bottom-left (TEMA N- title, ref)
definicion           → composed card PNG (header + icon/chevron/def/icon rows)
ab_title             → full-bleed image + youth chrome bottom-left (N.A- title, ref)
texto_biblico        → black slide, yellow/white verses (1+ slides)
ab_body              → blue header + white body + navy footer (N slides)
(repeat ab_title → texto → ab_body for each A/B/C point in the theme)
```

Study 02 tema II has **three** points (A.2, B.2, C.2); tema I and III have two. Slide count per tema is **variable** — allocate from template or duplicate prototype slides.

### Texto Bíblico

- Same pipeline as youth **Texto Bíblico**: `expand_glued_verses` → `pack_verses(VERSE_BUDGET=280)` → `set_verses(VerseKind::Texto)`.
- Yellow citation + verse numbers; white body on black; citation only on the first slide of each passage.
- Leave **~20–25%** empty margin at the bottom. If verses exceed the budget, **duplicate** the texto prototype and continue (do not dump overflow onto one slide).
- `set_verses` forces `noAutofit` on the verse box — `spAutoFit` can collapse text until the slide looks blank.
- Black full-slide background.
- **Citation: NO parentheses** — `JOSUÉ 3:14-16` / `1 CORINTIOS 10:32,33` (never `(…)`). Adult gold may ship parens; strip them on fill.
- Verse numbers yellow bold; body white.
- **Must be visibly readable** — if verses are missing on screen, the slide is broken (check `TextBox 4`, colours, `noAutofit`, Y below header).

---

## Study 02 slide map (58 slides)

| Pages | Type | Notes |
|------:|------|-------|
| 1 | `title` | EL CRUCE MILAGROSO DEL JORDÁN |
| 2–6 | `lectura_antifonal` | 5 slides, Josué 3:14–16; 4:5–18 |
| 7 | `objetivos` | 3 objectives |
| 8 | `pensamiento` | Pensamiento central + texto áureo |
| 9 | `ensenanza_datos` | Teaching + datos generales |
| 10 | `intro_header` | Full-bleed + “INTRODUCCIÓN” |
| 11–14 | `intro_body` | 4 slides |
| 15 | `tema_header` | TEMA I |
| 16–17 | `definicion` | 2 slides |
| 18 | `ab_title` | 1.A — full-bleed + left panel |
| 19–20 | `texto_biblico` | Josué 3:14–16 |
| 21–23 | `ab_body` | 1.A — 3 slides |
| 24 | `ab_title` | B.1 |
| 25 | `texto_biblico` | Josué 3:17 |
| 26–28 | `ab_body` | B.1 — 3 slides |
| 29 | `tema_header` | TEMA II |
| 30 | `definicion` | 1 slide |
| 31–46 | A.2 / B.2 / C.2 blocks | 3 sub-points (16 slides) |
| 47 | `tema_header` | TEMA III |
| 48–57 | definicion + A.3 + B.3 | 11 slides |
| 58 | `proximo` | Estudio 03 |

---

## Section images (builder)

For each study, generate **full-bleed 16:9 PNGs** (~1408×768 or larger):

| Image | Used on |
|-------|---------|
| `intro-header.png` | `intro_header` |
| `tema-{1,2,3}.png` | Each `tema_header` |
| `definicion-{1,2,3}.png` | Each `definicion` — **card layout** (see design ref above); icons unique per term |
| `ab-{tema}{letter}.png` | Each `ab_title` (e.g. `ab-1A.png`, `ab-2B.png`) |

Requirements:

- **Full bleed** — subject/scene fills the **entire** slide edge-to-edge (same as adult 2.A reference). Builder forces pic `xfrm` to `0,0` / `12192000×6858000` and strips `srcRect` crops.
- **Multiple designs per study** — intro, each tema, and each A/B title get **different** cinematic looks (palette/lighting/composition). Do not reuse one motif for every slide.
- **Text-safe bottom-left (~35–40% width):** soft mist / parchment wash under the youth-style chrome (navy title + gray verse). Same idea as youth section art — calm area under text, scene on the right.
- **No text, logos, or watermarks** in the image itself (chrome is OOXML).
- Video tema headers (II/III): strip video; use still full-bleed poster art.

Insert via OOXML media replace (`ppt/media/imageN.png`) — same pattern as youth section images but more assets per study.

---

## QA checklist (every adult deck)

1. Open PDF side-by-side with reference study PDF.
2. **Texto Bíblico** — every slide shows yellow/white verses on black (no “empty” slides).
3. **Definición** — matches card design (header + icon/chevron/text/icon rows); icons fit the terms, not the sample stock set.
4. **Body slides** — no text touching or crossing the navy footer bar.
5. **Tema / A-B image slides** — full-bleed art; youth chrome (orange bar / title / gray line / verse) at bottom-left only.
6. **Paragraph breaks** — teaching markers `(1)`/`(2)`/`(3)` start new paragraphs.
7. `lamad validate` → OK; export PDF.

---

## Related files

| File | Role |
|------|------|
| `reference/definicion-etimologia-design.png` | Visual authority for definición cards |
| `SLIDE_MAP.md` | OOXML shape names for study 4 prototype |
| `scripts/prepare_adult_master_template.py` | Build `master-template.pptx` from gold PPTX |
| `scripts/extract_adult_study_json.py` | Extract JSON from a finished deck |
