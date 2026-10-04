# Adult template slide map — study 24 (`24 EL VALOR DE LA MODESTIA.pptx`)

Source template: `lamad-cli/template/adult/24 EL VALOR DE LA MODESTIA.pptx`  
Build template: `lamad-cli/template/adult/master-template.pptx` (copy of gold + repaired `notesSlide` rels — run `scripts/prepare_adult_master_template.py`)

Total slides in `sldIdLst`: **62** (`slide1.xml` … `slide62.xml`)

This document maps each slide index to its **slide type**, the **shape names** the Rust adult builder should target for text replacement, and implementation notes (SmartArt, media, layout quirks).

---

## Slide type summary

| Type | Slides | Count |
|------|--------|------:|
| `title` | 1 | 1 |
| `lectura_antifonal` | 2–5 | 4 |
| `objetivos` | 6 | 1 |
| `pensamiento` | 7 | 1 |
| `ensenanza_datos` | 8 | 1 |
| `intro_header` | 9 | 1 |
| `intro_body` | 10–13 | 4 |
| `tema_header` | 14, 29, 44 | 3 |
| `definicion` | 15, 30, 45 | 3 |
| `ab_title` | 16, 22, 31, 37, 46, 53 | 6 |
| `texto_biblico` | 17, 23, 32, 38, 47, 54 | 6 |
| `ab_body` | 18–21, 24–28, 33–36, 39–43, 48–52, 55–61 | 33 |
| `proximo` | 62 | 1 |

---

## Global shape conventions

### Title / próximo (`title`, `proximo`)

Same layout (`slideLayout1.xml`). Mirror of each other except labels and study number.

| Shape | Role | Replace? |
|-------|------|----------|
| `TextBox 3` | Label `Estudio` / `Bíblico` (two lines) | Usually static |
| `TextBox 4` | Study number (e.g. `24`, `25`) | **Yes** |
| `Rounded Rectangle 5` | Decorative badge chrome | No |
| `Shape 80` | `ESCUELA BIBLICA` / `PRÓXIMA ESCUELA BIBLICA` | Static label |
| `CuadroTexto 1` | Study title (Verlag Black, large) | **Yes** |
| `TextBox 7` | `Base Bíblica:` label + citation line(s) | **Yes** (label bold; citations regular) |
| `Imagen 2` | Top-right branding/art | Swap per study if needed |

### Lectura antifonal (`lectura_antifonal`)

Layout: `slideLayout16.xml`

| Shape | Role | Replace? |
|-------|------|----------|
| `Título 1` | Header `LECTURA ANTIFONAL` | Static |
| `CuadroTexto 5` | Citation + verse text (packed, multi-verse) | **Yes** |

**Packing (HARD):** `expand_glued_verses` → `pack_verses(VERSE_BUDGET=280)` → `set_verses(VerseKind::Lectura)`. Same budget as youth — never overflow the panel; split to a new slide instead. Colours match youth: red citation/numbers, black body (forced). Citation only on the first slide of each passage. Extra packs allocate new slides by duplicating slide 2. JSON `lectura_antifonal` is a list of **passages**, not a fixed slide count.

### Texto bíblico (`texto_biblico`)

Layout: `slideLayout6.xml` (distinct from most body slides)

| Shape | Role | Replace? |
|-------|------|----------|
| `CuadroTexto 3` | Header `Texto Bíblico` | Static |
| `TextBox 4` | Citation + verses (yellow/white) | **Yes** |

**Packing (HARD):** same as youth Texto Bíblico — `expand_glued_verses` → `pack_verses(VERSE_BUDGET=280)` → `set_verses(VerseKind::Texto)` with `noAutofit`. Citation only on first slide of the passage. Extra packs duplicate this prototype and insert into the active order before the A/B body slides.
| `TextBox 4` | Citation + verse text | **Yes** |

### Introducción body (`intro_body`)

Layout: `slideLayout2.xml`

| Shape | Role | Replace? |
|-------|------|----------|
| `Título 1` | Subheader `Introducción` | Static |
| `Marcador de contenido 2` | Body paragraphs | **Yes** |

Decorative `Rectangle 14/16/18/20/22` shapes — do not edit.

### Definición (`definicion`)

**Design:** composed full-bleed card art — see `LAYOUT_GUIDE.md` + `reference/definicion-etimologia-design.png`. Not a plain text body dump.

| Shape | Role | Replace? |
|-------|------|----------|
| Main picture / slide image | Full composed `definicion-{n}.png` (title + rows + icons baked in) | **Yes** |
| `Título 1` / `Marcador de contenido 2` | Clear or hide after image swap so text does not double-render | Clear |

Prototype slides today (15 / 30 / 45) still use the old blue-header layout until media swap is wired; treat the PNG design as the target.

### A/B body (`ab_body`)

| Shape | Role | Replace? |
|-------|------|----------|
| `Título 1` | Point title `N.A - …` / `N.B - …` | **Yes** (title on every continuation slide) |
| `Marcador de contenido 2` | Commentary body | **Yes** |

Decorative `Rectangle 23/25/27` — do not edit.

### A/B title (`ab_title`) — three variants

All use `slideLayout2.xml` unless noted. Bottom band title + reference + hero image.

| Variant | Slides | Title shape | Reference shape | Image shape |
|---------|--------|-------------|-----------------|-------------|
| A | 16 | `Título 1` | `CuadroTexto 6` | `Imagen 4` |
| B | 22 | `Título 1` | `CuadroTexto 4` | `Imagen 2` |
| C | 31, 46, 53 | `Título 1` | `Título 6` | `Imagen 2` or `Imagen 3` |
| D | 37 | `Título 1` | `Título 7` | `Imagen 2` |

`Título 1` on ab_title slides uses compact form (`1.A …` without dash suffix); `ab_body` continuations use `1.A - …`.

### Tema header (`tema_header`) — two variants

| Variant | Slides | Title shape | Reference shape | Media |
|---------|--------|-------------|-----------------|-------|
| A (image) | 14 | `CuadroTexto 3` | `CuadroTexto 5` | `Imagen 2` → `image8.png` |
| B (video) | 29, 44 | `Título 1` | `CuadroTexto 4` | Poster `image11.png` / `image14.png` + `media1.mp4` / `media2.mp4` |

Video slides also contain a picture shape named `Genera_una_video_de_segundo (12)` (slide 29) or `Genera_una_video_de_segundo (14)` (slide 44) — AI-generated video placeholder; replace media rels if regenerating.

---

## SmartArt / diagram slides (6–7)

### Slide 6 — `objetivos`

| Shape / part | Role |
|--------------|------|
| `Título 1` | Label `Objetivos:` (static) |
| `CuadroTexto 2` | **SmartArt graphicFrame** — hosts diagram |
| `Picture 3092` | Earth background (`image2.jpeg`) |
| `Rectangle 3102` | Decorative overlay |

**Diagram package (slide rels → `ppt/diagrams/`):**

| File | Purpose |
|------|---------|
| `data1.xml` | Data model — **3 objective text nodes** (`custT="1"` points) |
| `drawing1.xml` | Rendered SmartArt shapes (vProcess5 vertical process, 3 boxes) |
| `layout1.xml` | Layout definition |
| `colors1.xml` | Color scheme (`accent5_2`) |
| `quickStyle1.xml` | Quick style (`simple2`) |

**Text nodes to replace in `data1.xml` AND `drawing1.xml`:**

1. `Integrar la disposición de ser modestos en toda nuestra manera de vivir.`
2. `Formular la conducta que es propia de los que profesan modestia.`
3. `Evaluar el valor de la modestia en la vida de los hijos de Dios.`

Font in drawing: Verlag Light ~36–40pt (`sz="3600"` / `sz="4000"`). Update both `data1.xml` and `drawing1.xml` to avoid ghost text on PDF export.

### Slide 7 — `pensamiento`

| Shape / part | Role |
|--------------|------|
| `Título 1` (id=5, top) | Label `Pensamiento Central:` (static) |
| `CuadroTexto 2` | **SmartArt graphicFrame** — pensamiento + texto áureo |
| `Título 1` (id=2, lower) | Label `Texto Áureo:` (static) — **duplicate shape name** |
| `Picture 3092` | Same earth background (`image2.jpeg`) |
| `Rectangle 3096` | Decorative gradient overlay |

**Diagram package:**

| File | Purpose |
|------|---------|
| `data2.xml` | Data model — 2 content nodes |
| `drawing2.xml` | LinedList layout render |
| `layout2.xml` | Layout (`LinedList`) |
| `colors2.xml` | Color scheme (`accent1_2`) |
| `quickStyle2.xml` | Quick style (`simple1`) |

**Text nodes to replace in `data2.xml` AND `drawing2.xml`:**

1. **Pensamiento central** (regular Times New Roman 36pt):  
   `El enfoque de la vida cristiana no debe estar vanidosamente en nosotros mismos…`
2. **Texto áureo** (quote + bold citation, 40pt):  
   `"…como también yo en todas las cosas agrado a todos…" (1 Corintios 10:33).`

**Builder note:** Two shapes share the name `Título 1` on slide 7. Target by XML element order or `cNvPr/@id` (`id="5"` = Pensamiento Central, `id="2"` = Texto Áureo), not by name alone.

---

## Slide 8 — `ensenanza_datos`

Single slide with five data panels + one teaching panel. Section numbers/labels live in grouped `Google Shape;…;p28` shapes (not stable for replacement — treat as chrome). **Replace only the `CuadroTexto` bodies:**

| Section label (chrome) | Body shape | Field |
|------------------------|------------|-------|
| 1 ENSEÑANZA | `CuadroTexto 13` | Teaching summary |
| 2 DATOS GENERALES ACERCA DEL TEMA | `CuadroTexto 22` | Section header text only (no separate body box in this study) |
| 3 PERSONAJES | `CuadroTexto 9` | Characters list |
| 4 FECHA | `CuadroTexto 21` | Date / authorship dates |
| 5 LUGAR | `CuadroTexto 32` | Location |
| AUTOR (chrome shows `1`) | `CuadroTexto 38` | Author attribution |

Icon SVGs via rels: `image3.svg`, `image4.svg`, `image5.svg`, `image6.svg` — section header artwork; swap if needed.

---

## Per-slide index

### Front matter (1–8)

| # | Type | Shapes to replace | Notes |
|---|------|-------------------|-------|
| 1 | `title` | `TextBox 4`, `CuadroTexto 1`, `TextBox 7` | `Imagen 2` = `image1.png` |
| 2 | `lectura_antifonal` | `CuadroTexto 5` | Mateo 6:1-4 passage |
| 3 | `lectura_antifonal` | `CuadroTexto 5` | 1 Corintios 10:31-33 |
| 4 | `lectura_antifonal` | `CuadroTexto 5` | 1 Pedro 5:2-4 |
| 5 | `lectura_antifonal` | `CuadroTexto 5` | 1 Pedro 5:5-7 (continuation) |
| 6 | `objetivos` | `data1.xml` + `drawing1.xml` via `CuadroTexto 2` | SmartArt — see above |
| 7 | `pensamiento` | `data2.xml` + `drawing2.xml` via `CuadroTexto 2` | SmartArt; duplicate `Título 1` names |
| 8 | `ensenanza_datos` | `CuadroTexto 13`, `22`, `9`, `21`, `32`, `38` | Single slide, 6 text fields |

### Introducción (9–13)

| # | Type | Shapes to replace | Notes |
|---|------|-------------------|-------|
| 9 | `intro_header` | *(none — static)* | `Título 1` = `INTRODUCCIÓN`; `Imagen 2` = `image7.png` |
| 10 | `intro_body` | `Marcador de contenido 2` | Continuation 1 |
| 11 | `intro_body` | `Marcador de contenido 2` | Continuation 2 |
| 12 | `intro_body` | `Marcador de contenido 2` | Continuation 3 |
| 13 | `intro_body` | `Marcador de contenido 2` | Continuation 4 |

### Tema I (14–28)

| # | Type | Shapes to replace | Notes |
|---|------|-------------------|-------|
| 14 | `tema_header` | `CuadroTexto 3`, `CuadroTexto 5` | TEMA I; `Imagen 2` = `image8.png` |
| 15 | `definicion` | `Marcador de contenido 2` | Tema I vocabulary |
| 16 | `ab_title` | `Título 1`, `CuadroTexto 6` | 1.A title; `Imagen 4` = `image9.png` |
| 17 | `texto_biblico` | `TextBox 4` | Mateo 6:1-2 |
| 18 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 1.A body 1 |
| 19 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 1.A body 2 |
| 20 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 1.A body 3 |
| 21 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 1.A body 4 |
| 22 | `ab_title` | `Título 1`, `CuadroTexto 4` | 1.B title; `Imagen 2` = `image10.png` |
| 23 | `texto_biblico` | `TextBox 4` | Mateo 6:3-4 |
| 24 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 1.B body 1 |
| 25 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 1.B body 2 |
| 26 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 1.B body 3 |
| 27 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 1.B body 4 |
| 28 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 1.B body 5 |

### Tema II (29–43)

| # | Type | Shapes to replace | Notes |
|---|------|-------------------|-------|
| 29 | `tema_header` | `Título 1`, `CuadroTexto 4` | TEMA II; video `media1.mp4` + poster `image11.png` |
| 30 | `definicion` | `Marcador de contenido 2` | Tema II vocabulary |
| 31 | `ab_title` | `Título 1`, `Título 6` | 2.A title; ref in `Título 6`; `Imagen 2` = `image12.png` |
| 32 | `texto_biblico` | `TextBox 4` | 1 Corintios 10:31 |
| 33 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 2.A body 1 |
| 34 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 2.A body 2 |
| 35 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 2.A body 3 |
| 36 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 2.A body 4 |
| 37 | `ab_title` | `Título 1`, `Título 7` | 2.B title; ref in `Título 7`; `Imagen 2` = `image13.png` |
| 38 | `texto_biblico` | `TextBox 4` | 1 Corintios 10:32-33 |
| 39 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 2.B body 1 |
| 40 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 2.B body 2 |
| 41 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 2.B body 3 |
| 42 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 2.B body 4 |
| 43 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 2.B body 5 |

### Tema III (44–61)

| # | Type | Shapes to replace | Notes |
|---|------|-------------------|-------|
| 44 | `tema_header` | `Título 1`, `CuadroTexto 4` | TEMA III; video `media2.mp4` + poster `image14.png` |
| 45 | `definicion` | `Marcador de contenido 2` | Tema III vocabulary |
| 46 | `ab_title` | `Título 1`, `Título 6` | 3.A title; `Imagen 2` = `image15.png` |
| 47 | `texto_biblico` | `TextBox 4` | 1 Pedro 5:2-4 |
| 48 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.A body 1 |
| 49 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.A body 2 |
| 50 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.A body 3 |
| 51 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.A body 4 |
| 52 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.A body 5 |
| 53 | `ab_title` | `Título 1`, `Título 6` | 3.B title; `Imagen 3` = `image16.png` |
| 54 | `texto_biblico` | `TextBox 4` | 1 Pedro 5:5-7 |
| 55 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.B body 1 |
| 56 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.B body 2 |
| 57 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.B body 3 |
| 58 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.B body 4 |
| 59 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.B body 5 |
| 60 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.B body 6 |
| 61 | `ab_body` | `Título 1`, `Marcador de contenido 2` | 3.B body 7 |

### Cierre (62)

| # | Type | Shapes to replace | Notes |
|---|------|-------------------|-------|
| 62 | `proximo` | `TextBox 4`, `CuadroTexto 1`, `TextBox 7` | Next study #25; `Imagen 2` = `image17.png` |

---

## Slide count template for dynamic packing

Fixed front (slides 1–9) + variable sections. Per tema block pattern:

```
tema_header (1)
definicion (1)
ab_title (1) → texto_biblico (1) → ab_body (N)
ab_title (1) → texto_biblico (1) → ab_body (M)
```

Study 24 actual body counts:

| Tema | 1.A bodies | 1.B bodies |
|------|-----------|-----------|
| I (slides 18–28) | 4 (18–21) | 5 (24–28) |
| II (slides 33–43) | 4 (33–36) | 5 (39–43) |
| III (slides 48–61) | 5 (48–52) | 7 (55–61) |

Intro: 4 body slides (10–13). Lectura antifonal: 4 slides (2–5).

When duplicating `ab_body` or `texto_biblico` slides, clone slide XML + rels, fix `notesSlide{N}` targeting, update `[Content_Types].xml`, `presentation.xml.rels`, and `sldIdLst`.

---

## Layout index (from slide rels)

| Layout file | Used by |
|-------------|---------|
| `slideLayout1.xml` | 1, 62 (title / próximo) |
| `slideLayout2.xml` | Most content slides (intro, tema, definicion, ab_title, ab_body, objetivos, pensamiento, tema_header video variant) |
| `slideLayout6.xml` | 17, 23, 32, 38, 47, 54 (`texto_biblico`) |
| `slideLayout16.xml` | 2–5 (`lectura_antifonal`) |

---

## Media inventory (`ppt/media/`)

| File | Used on slide(s) |
|------|------------------|
| `image1.png` | 1 (title) |
| `image2.jpeg` | 6, 7 (objetivos / pensamiento background) |
| `image3.svg` – `image6.svg` | 8 (ensenanza_datos section icons) |
| `image7.png` | 9 (intro header) |
| `image8.png` | 14 (tema I header) |
| `image9.png` | 16 (1.A title) |
| `image10.png` | 22 (1.B title) |
| `image11.png` | 29 (tema II poster) |
| `image12.png` | 31 (2.A title) |
| `image13.png` | 37 (2.B title) |
| `image14.png` | 44 (tema III poster) |
| `image15.png` | 46 (3.A title) |
| `image16.png` | 53 (3.B title) |
| `image17.png` | 62 (próximo) |
| `media1.mp4` | 29 (tema II video) |
| `media2.mp4` | 44 (tema III video) |

---

## Rust builder implementation notes

1. **Shape lookup:** Prefer matching `p:cNvPr/@name` exactly. Exception: slide 7 has two `Título 1` — disambiguate by `cNvPr/@id` or document order.
2. **SmartArt:** Reuse youth-builder pattern (`diagrams.rs`): patch `dataN.xml` + `drawingN.xml` together. Slide 6 uses diagram set **1**; slide 7 uses set **2**.
3. **ab_title reference shapes:** Not interchangeable across variants — `CuadroTexto 4`, `CuadroTexto 6`, `Título 6`, and `Título 7` appear on different slides only.
4. **tema_header reference shapes:** Slide 14 uses `CuadroTexto 5`; slides 29 and 44 use `CuadroTexto 4`.
5. **texto_biblico** uses `TextBox 4` (not `CuadroTexto 5` as in youth lectura).
6. **intro / ab body** uses `Marcador de contenido 2` (not `CuadroTexto 5`).
7. **notesSlide rels:** Each slide has a `notesSlide{N}.xml` rel; when cloning slides, retarget or remove stale notes rels (same rule as youth builder).
8. **No conclusión / propósitos / section-image slides** in adult template — structure differs entirely from youth `master-template.pptx`.
