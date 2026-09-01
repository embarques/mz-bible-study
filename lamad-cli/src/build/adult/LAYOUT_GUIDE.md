# Adult study layout guide

**Reference deck:** Estudio 02 — *EL CRUCE MILAGROSO DEL JORDÁN* (58 slides, 16:9).  
Use this PDF as the **visual authority** for adult builds. Study 24 (`master-template.pptx`) is an earlier OOXML prototype; when the two disagree on layout, **follow this guide**.

Images may use a **new house style** (AI-generated, cinematic, full-bleed). Typography, colours, and chrome come from the template; only the photo/illustration art changes per study.

---

## Global rules

### 1. Full-bleed images

These slide types use a **single image covering the entire slide** (edge to edge, 16:9):

| Slide type | Image role | Text placement |
|------------|------------|----------------|
| `intro_header` | Scenic full-bleed (e.g. open Bible, landscape) | Centred semi-transparent white box with section title only |
| `tema_header` | Cinematic scene for the theme | **Bottom bar only** — never title text over the centre of the image |
| `ab_title` | Cinematic scene for the A/B point | **Left panel** (~35–40% width) with torn-paper edge; image fills the rest |

Do **not** place body paragraphs on image slides. Image slides carry titles + references only.

### 2. Text must fit — bottom guard rails

Every text-heavy slide has a **hard floor** where content stops. Nothing may sit on or below that line.

| Slide type | Guard rail | Approx. safe height |
|------------|------------|---------------------|
| `intro_body` / `definicion` / `ab_body` | Navy footer bar at bottom of slide | Content ends **above** the footer (~bottom 8–10% of slide is chrome only) |
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
| Tema bottom bar | Blue-to-peach gradient; white title; reference in **black pill** |
| A/B left panel | Off-white/torn edge; black title; reference in **black pill** |

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

### Lectura antifonal (5 slides in study 02)

- Header `LECTURA ANTIFONAL` + citation with semicolon.
- Red citation + verse numbers; black body; pack whole verses per slide.

### Objetivos / Pensamiento / Enseñanza+Datos

- SmartArt / composite slides — keep template diagram layout; replace text only.
- Objetivos: three cascading blue boxes on dark blue background.

### Introducción

1. **`intro_header`** — full-bleed image + centred translucent box, “INTRODUCCIÓN” only.
2. **`intro_body`** (×N) — blue header bar + white justified body + navy footer. Split before footer.

### Tema block (per theme)

```
tema_header          → full-bleed image + bottom gradient bar (TEMA N, title, ref)
definicion           → blue header + white body (may be 1–2 slides)
ab_title             → full-bleed image + left torn panel (point label, title, ref)
texto_biblico        → black slide, yellow/white verses (1+ slides)
ab_body              → blue header + white body + navy footer (N slides)
(repeat ab_title → texto → ab_body for each A/B/C point in the theme)
```

Study 02 tema II has **three** points (A.2, B.2, C.2); tema I and III have two. Slide count per tema is **variable** — allocate from template or duplicate prototype slides.

### Texto Bíblico

- Black full-slide background.
- Citation in yellow parens on first slide of range: `(JOSUÉ 3:14-16)` when template uses parens.
- Verse numbers yellow bold; body white.
- **Must be visibly readable** — if verses are missing on screen, the slide is broken (check `TextBox 4`, colours, `noAutofit`).

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
| `ab-{tema}{letter}.png` | Each `ab_title` (e.g. `ab-1A.png`, `ab-2B.png`) |

Requirements:

- **Full bleed** — subject/scene fills the frame; no letterboxing.
- **Text-safe zones baked in:** tema = calm/dark lower third for gradient bar; ab_title = calm **left third** for torn panel (match study 02, not youth’s left fade).
- **No text, logos, or watermarks** in the image.
- **New style each study** — do not reuse another study’s art.

Insert via OOXML media replace (`ppt/media/imageN.png`) — same pattern as youth section images but more assets per study.

---

## QA checklist (every adult deck)

1. Open PDF side-by-side with reference study PDF.
2. **Texto Bíblico** — every slide shows yellow/white verses on black (no “empty” slides).
3. **Body slides** — no text touching or crossing the navy footer bar.
4. **Tema / A-B image slides** — image is full-bleed; text only in bar/panel.
5. **Paragraph breaks** — teaching markers `(1)`/`(2)`/`(3)` start new paragraphs.
6. `lamad validate` → OK; export PDF.

---

## Related files

| File | Role |
|------|------|
| `SLIDE_MAP.md` | OOXML shape names for study 24 prototype |
| `scripts/prepare_adult_master_template.py` | Build `master-template.pptx` from gold PPTX |
| `scripts/extract_adult_study_json.py` | Extract JSON from a finished deck |
