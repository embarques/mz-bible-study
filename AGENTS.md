# AGENTS.md — Mount Zion Church Bible Study PowerPoint

Agent playbook for building Spanish Bible study decks from scans. Match [`template/youth/master-template.pptx`](template/youth/master-template.pptx) (default `--audience youth`) and recent decks in [`bible-studies/`](bible-studies/). Adult uses the Rust builder (`--audience adult`) and [`lamad-cli/src/build/adult/LAYOUT_GUIDE.md`](lamad-cli/src/build/adult/LAYOUT_GUIDE.md) — follow that guide’s **HARD rules** (no citation parentheses on Texto/image chrome, no title highlight, shrink oversized text, scenic image visible, Texto Áureo citation on its own line).

User-facing summary: [`README.md`](README.md). Template index: [`template/README.md`](template/README.md).

**Deliverables every study:** after the `.pptx` is complete and validates, also export a **PDF** beside it (same name, `.pdf`). Do this automatically — do not ask.

### Git / PR (HARD — protect `main`)

Do **not** commit or land day-to-day work on `main`. Before editing: create `feat/<topic>` (or `fix/…` / `chore/…`), commit there, push, open a **PR into `main`**. Merge only when the user asks. Rule file: [`.cursor/rules/git-branch-pr.mdc`](.cursor/rules/git-branch-pr.mdc).

---

## Required inputs (every study)

| Mode | What you provide |
|------|------------------|
| **Single study** | **1.** Three content scans of the current study (any order — sort by printed page #). **2.** Title page of the **next** study (for Próximo), unless this is the last study |
| **Batch PDF (new)** | **One PDF** with **all studies in order**. Each study = **3 consecutive content pages**. Next study starts immediately after the previous ends. Build **every** study into `bible-studies/` |

### Single-study details

| Input | Purpose |
|-------|---------|
| **1. Three scan images** *or* pages from a small PDF | Full pages of the **current** study. Sort by printed **page number** before extracting |
| **2. One title-page scan** (optional on the **last** study) | First page of the **next** study — number, title, base bíblica for Próximo |

### Batch PDF → all studies

**Checkpoint (mandatory):** On receiving the PDF, **first** extract/inspect pages and tell the user what you see — total pages, study boundaries (3-page groups), each estudio number + title, and that the last study has no Próximo. **Do not generate decks until the user confirms.**

After OK:

1. Rasterize/extract **every** page from the PDF (in order).
2. Segment into studies: **3 pages per study**, sequential (page groups 1–3, 4–6, …). Confirm boundaries with printed `ESTUDIO N` / titles when available.
3. For each study **except the last:** Próximo = metadata from the **next** study’s title page (first page of the next 3-page block — the title layout with number/title/base bíblica).
4. For the **last** study: only 3 pages, no following study → **omit the Próximo estudio slide** (do not ask, do not invent).
5. Produce one `bible-studies/{N} - {TITLE}.pptx` (+ `.pdf`) **per** study in the document.

**If the user does not provide image 2** on a *single* non-final study: **stop and ask** for próximo number / title / base bíblica before the closing slide. Do not invent próximo metadata.

### Minimal user prompt

```
1. [attach 3 current-study scans]
2. [attach next-study title-page scan]
```

From **2**, read number, title, and base bíblica. If base has multiple citations separated by `;`, put **each on its own line** under `Base Bíblica:` on Próximo estudio (keep `;` except on the last) so nothing overflows.

If only **1** is provided, ask:

```
Need próximo estudio:
1. number
2. title
3. base bíblica
```

Optional one-liner if needed: `2A spans pages 96–98`.

Output:
- `bible-studies/{N} - {TITLE}.pptx` (example: `17 - UN ABRAZO QUE DA VIDA.pptx`)
- `bible-studies/{N} - {TITLE}.pdf` — **always** export after the pptx validates (same basename). Do not ask.

---

## Template assets

| File | Role |
|------|------|
| [`template/youth/master-template.pptx`](template/youth/master-template.pptx) | **Youth gold standard** — clone slide XML from here (`--audience youth`, default) |
| [`template/adult/master-template.pptx`](template/adult/master-template.pptx) / `lamad-cli/template/adult/` | Adult gold — build with `lamad build … --audience adult`. Layout HARD rules: [`lamad-cli/src/build/adult/LAYOUT_GUIDE.md`](lamad-cli/src/build/adult/LAYOUT_GUIDE.md) |

**Never overwrite** `template/`. Scratch: `_agent-reference.pptx` or `/tmp` / `generated/`. Generated decks go in **`bible-studies/`**.

---

## How to edit PowerPoint

Edit by **OOXML only**: unzip → edit XML/media → rezip `ZIP_DEFLATED`.

Prefer **copying working slide XML** from `template/youth/master-template.pptx` (or a recent good deck), then swap text/images — do not invent new layouts.

After adding/removing/reordering slides, update:

- `[Content_Types].xml`
- `ppt/_rels/presentation.xml.rels`
- `ppt/presentation.xml` → `sldIdLst` order

---

## End-to-end workflow (scan → deck)

### 1. Sort scans by printed page number, then read

Upload order may be random. Find the **page number** (usually bottom-left) and process in **ascending** order. Reconstruct clean Spanish text (image Read + OCR assist if needed).

Typical Senda de Vida page map (3 consecutive study pages):

| Page role (after sorting) | Usually contains |
|------|------------------|
| **First page** | Estudio number + title, base bíblica, idea principal, propósitos, **Lectura Bíblica**, **Para Memorizar**, **Comentarios acerca del tema** |
| **Middle page** | Ideas para el maestro (skip), **Introducción**, **Desarrollo** point **1** (A/B), start of point **2** (often **2.A** cuts off mid-sentence) |
| **Last page** | End of **2.A**, **2.B**, point **3** (A/B), **Conclusión**, Preguntas de reflexión (skip) |

**Include:** everything that maps to the slide flow below.  
**Exclude unless asked:** “Ideas para el maestro”, “Preguntas de reflexión”, footers/copyright.

### 2. Extract a structured content outline

1. Estudio number + title + base bíblica (current)
2. Lectura verses (exact ranges printed)
3. Propósitos (1–3)
4. Idea principal
5. Para memorizar (verse text; citation often in diagram footer only)
6. Comentario (full paragraphs)
7. Introducción (full)
8. For points **1 / 2 / 3**: section title + verse range; Texto Bíblico verses; **A** + **B** bodies
9. Conclusión
10. Próximo estudio ← from title-page scan **2**

**Cross-page merge:** join split paragraphs at cutoffs before packing.  
**Transcription:** prefer readable scan text over noisy OCR; fix obvious OCR errors; keep wording faithful — don’t paraphrase theology.

### 3. Start the deck file

- Clone [`template/youth/master-template.pptx`](template/youth/master-template.pptx) (default `--audience youth`).
- Write `bible-studies/{N} - {TITLE}.pptx`.
- Replace all study-specific text; generate and swap section images.

### 4. Fill slides → pack → QA

Follow slide order and recipes below, then the checklist.

### 5. Generate section images (1 / 2 / 3)

**Just generate them — do not ask.** You already have permission to create section art. Do **not** ask the user to confirm prompts, styles, or whether to generate. When using **`mzbs prepare`** (single or `--from`/`--to` batch), follow the **CLI-assigned** `section_style` for that estudio (rotating catalog — different id every study). For chat-only builds, pick a design family different from prior studies yourself. Generate all three images, insert them into the deck, and move on. If a generation fails or is blocked, regenerate with a safer framing and continue — still without asking.

**One design family per study; different family from other studies.**

- Within a study, all three section images share the **same visual design language** (lighting, finish, edge treatment, optional accents).
- Across studies, **change the design** — `mzbs` assigns a rotating family; chat builds must not reuse the previous study’s look (e.g. if study 16 used yellow dashed accent arcs / a particular white-curve panel, study 17 and 18 must not copy that).
- Catalog: `python/mz_bible_study/section_styles.py` (parchment, lavender mist, sage paper, ink wash, dawn gold, …).
- When cloning a prior deck, **always strip** PowerPoint shapes named `Arc …` (yellow dashed/dotted arcs). That motif is retired — **do not** keep or redraw dotted/dashed arc chrome on section slides for any new study.
- Generate three **new** illustrations every study (don’t reuse another study’s media files).

Requirements:

- Aspect **16:9**; final media ≈ **1408×768** PNG.
- Scene fits each section theme; be inventive.
- **Finished slide backgrounds (HARD):** each PNG must include a deliberate **left text-safe treatment** (~35–45% width — soft mist, parchment fade, watercolor bloom, dusk glow, cool paper wash, etc.) that **matches this study’s design family**. Put the scene subject right/center-right. Do **not** deliver a busy undressed full-bleed under the title band. The treatment must **change with each study** — do not reuse the previous study’s fade motif.
- Full-bleed canvas is fine **only when** that left treatment is baked into the art.
- **HARD RULE — title + verse must be readable:** calm/light area under the text (via the left treatment above), or switch title/verse colors if needed. Never leave dark navy text on a busy dark photo, or white text on a bright wash.
- No text, letters, logos, watermarks, or decorative caption marks in the art. **Never** use yellow dotted/dashed arcs (study-16 motif is retired).
- **No** `alphaModFix` / opacity dimming on the photo itself.
- Slide chrome (XML): orange bar (`E85D04`), gray line, title (`1- TITLE`), bold verse under gray line — colors chosen so they stay readable on the image.

**Title vs verse (HARD RULE):** the section title and the verse citation must **never overlap**. Leave a clear gap above the gray line; the title box must end above the gray line; the verse sits **under** the gray line only. For long titles: shrink title font and/or deepen the title box and **push the gray line + verse down** so everything fits without collision.

If safety filters block a pose, regenerate with safer framing (same theological beat).

---

## Typical study flow (slide order)

1. **Title** — Estudio Bíblico {N}, title, Base Bíblica  
2. **Lectura Bíblica** — multi-slide  
3. **Propósitos**  
4. **Idea Principal / Para Memorizar** (SmartArt / diagrams)  
5. **Comentario acerca del pasaje bíblico** — multi-slide  
6. **Introducción** — header + body slides  
7. For each point **1 / 2 / 3**: section image → **Texto Bíblico** → **N.A** → **N.B**  
8. **Conclusión** — multi-slide  
9. **Próximo estudio** — title-slide layout; metadata from scan **2**

---

## Global layout rules

### Logo = bottom limit (HARD RULE)

On **every slide that shows the Mount Zion Church logo** (comentario, intro, A/B, conclusión, Lectura, Texto Bíblico, etc.), the **top edge of that logo** is the invisible text boundary:

- **Do not draw** any guide/red line on the slide — the logo itself is the guide.
- **No text may sit on or below the top of the logo** (not overlapping it, not in the footer band).
- Practical content-box bottom ≈ **6.45"** on a 7.5" slide so the last line stays **above** the logo top.
- Use `noAutofit`; turn off `spAutoFit` / `normAutofit`.

**How to cut when text would cross the logo top:**

1. Fill the slide with as much text as fits **above** the logo top.
2. If the **next sentence** (or next whole verse on Lectura/Texto) would go below the logo top, **do not start it** on this slide.
3. Move that **entire sentence / whole verse** to the next slide.
4. Always break at a **sentence end** (`.?!`) for body text, or a **whole verse** for Lectura/Texto. Never end a slide mid-sentence. Never start the next slide with the leftover half of a sentence.

Wrong: last line through/under the logo, or a sentence split across slides.  
Right: current slide ends at a period (or whole verse) above the logo; next slide starts with the next full unit.

### Packing text (HARD RULE — this was already fixed in early decks; do not regress)

**Goal:** fill each slide up to the logo floor. **Wrong** = huge empty lower half while the next slide has only 1–2 lines. **Wrong** = text through the logo.

- Pack **dense**. Before creating/keeping a continuation slide, ask: *“Does the next sentence or whole verse still fit above the logo on this slide?”* If yes → **keep it on this slide**.
- Split **only** when the next unit will not fit above the logo:
  - Body / comentario / intro / A-B / conclusión → split at **sentence end** (`.?!`)
  - Lectura / Texto Bíblico → split at **whole verses only** (never mid-verse / mid-sentence)
- Character budgets that usually stay **above the logo top** (Verlag Light ~42pt body; ~44pt Lectura/Texto) — treat as soft caps; if QA still crosses the logo top, drop the last full sentence to the next slide:
  - Body / comentario / intro / A-B / conclusión: aim **~360–400** chars of body text per slide (safer than 440). **Over ~420 often crosses the logo top.**
  - Lectura / Texto with citation on page: ~**250–300** chars of verse text; without citation, a bit more. Short verses **must** share a slide with neighbors when they fit above the logo top.
- **Anti-patterns (forbidden):**
  - One short verse alone on a Texto/Lectura slide while the previous slide has large empty black/white space
  - Splitting one verse across two slides
  - Body slide with only ~½ page of text while the next slide continues the same section
  - Body slide packed past the logo (~500+ chars at 42pt usually fails)
- After packing a series, **re-check**: if slide N is sparse and slide N+1’s first sentence/verse fits on N, merge backward.
- Duplicate slide XML when you need another page; **remove** unused continuation slides from `sldIdLst` (string-safe CT/rels edits); rebuild order.

### Scripture references

- Inline citations in body → **bold** (e.g. `(Efesios 2:1)`, `(Hechos 9:40)`, `2 Reyes 4:21`).
- Lectura/Texto verse numbers + citation titles stay bold and colored per layout.

### Font & bold rules (HARD — every study, never regress)

These are the **correct** weights. Match a known-good deck (e.g. study 16/18) and the template. When editing OOXML, **preserve** template `b="1"` on runs that should stay bold. Only force `bold=False` where this table says “regular”.

| Location | Bold? | Notes |
|----------|-------|--------|
| Title / Próximo: label `Base Bíblica:` | **Bold** | Only this label |
| Title / Próximo: citation line(s) under it | Regular | Never bold the verses here |
| Lectura: citation title (first slide of range) | **Bold** | Red; keep template bold |
| Lectura: verse **numbers** | **Bold** | Red; keep template bold |
| Lectura: verse body text | Regular | Black |
| Texto Bíblico: citation title (first of range) | **Bold** | Yellow |
| Texto Bíblico: verse **numbers** | **Bold** | Yellow |
| Texto Bíblico: verse body text | Regular | White |
| Section image: title `1- …` / verse under gray line | **Bold** | Keep template bold (Verlag Black) |
| A/B point titles `1.A- …` | **Bold** | Times New Roman ~40pt (`sz="4000"`) — keep `b="1"` |
| Comentario / Intro / A-B **body** | Regular | Verlag Light ~42pt; **except** inline scripture refs → **bold** |
| Conclusión header `CONCLUSIÓN` | **Bold** | Verlag Black |
| Conclusión **body** | Regular weight, **Times New Roman 42pt** | Force `typeface="Times New Roman"` + `sz="4200"`; inline refs still **bold** |

**OOXML / build-script pitfall (study 19 regression):**  
Do **not** call a helper that strips `b="1"` from every run with `bold=False` as the default.  
- Default = **preserve** the cloned run’s bold flag.  
- Force unbold **only** for: title/próximo citation lines under `Base Bíblica:`, and non-ref spans when applying inline-ref bolding.  
- Lectura/Texto `set_verses` must pass “preserve” for citation + verse numbers + body (template already has the right weights).

### Don’t fight the template

- Keep template headers (**Introducción** = white on blue bar).
- Section chrome (orange bar, gray line, title/verse placement) still follows the template overlays — the **illustration style** may change (see section images).

---

## Slide-type recipes

### 1. Title + próximo estudio

- Layout: title layout (`slideLayout12`).
- **Estudio Bíblico** + number — Gill Sans MT ~50pt bold.
- Title — **Verlag Black** ~80pt, centered.
- Base Bíblica — Gill Sans MT ~24pt, centered. **Only** the label `Base Bíblica:` is bold; citation line(s) are regular weight (not bold).
- **Multiple citations:** one per line under `Base Bíblica:` (`;` at end of each line except last). No overflow.
- **Próximo:** same title-slide rules; only change number/title/base from scan **2**.

### 2. Lectura Bíblica

- Header `LECTURA BÍBLICA`.
- First slide of a range: red citation title + verses — citation title **bold**.
- Continuations: **no** citation title — verse number + text only.
- Verse **numbers** bold red; verse **body** regular black; ~44pt; noAutofit; stop above logo.
- Keep study wording (including `[...]` gaps if printed).
- **Never strip bold** from citation titles or verse numbers when rewriting verse XML.

### 3. Propósitos / Idea / Memorizar

- SmartArt/diagrams (`ppt/diagrams/…`).
- If citation is in diagram **footer**, do **not** also append `(Libro …)` to memorizar body.
- **Propósitos SmartArt (HARD RULE):** after writing the three purpose texts into `ppt/diagrams/data2.xml` **and** `ppt/diagrams/drawing2.xml`, set every body run size in **`drawing2.xml`** from `sz="5600"` (56pt) → **`sz="3600"` (36pt)**. Leave `data2.xml` text content as-is; do **not** shrink other slides. Template 56pt overflows long Spanish purposes and PDF-exports as garbled / double-layered text (ghost text outside the boxes). **36pt is required every study** so Propósitos comes out clean. Verify in PDF that each purpose sits inside its blue box with no overflow or interleaved ghost text.

### 4. Comentario / Introducción / A-B / Conclusión

- Blue header + white body + logo.
- Comentario / Introducción / A-B body: **Verlag Light** ~42pt (`sz="4200"`); no bullets on comentario/intro.
- Comentario header: `COMENTARIO ACERCA DEL PASAJE BÍBLICO`.
- Introducción: header slide then body slides.
- A/B titles like `1.A- …` matching example.
- **Conclusión body: Times New Roman 42pt** — **HARD RULE.** Every conclusión body run must use `a:latin typeface="Times New Roman"` (and matching `ea`/`cs` if present) with `sz="4200"`. **Do not inherit** the template/example body font (master-template often has Verlag Light on conclusión — that is wrong; override it). Header `CONCLUSIÓN` stays Verlag Black. May use taller `CuadroTexto 5`; still respect logo top.
- Pack ~360–400 chars; bold inline refs.

### 5. Section image slides (1 / 2 / 3)

- **Per study:** one shared design language for all three images. **Next study:** a different design — never recycle the previous study’s motif.
- **Always strip** cloned `Arc …` yellow dashed/dotted shapes — that motif is retired; do not put it back.
- Whole-image art is fine **only with** a baked-in left text-safe treatment that matches this study’s design family (change the treatment next study).
- **Title + verse must stay readable** — calm area under text, soft scrim, and/or text color that contrasts with the background.
- Orange bar, gray line, title `1- TITLE`; verse **under** the gray line only.
- **No overlap** between title and verse — if the title is long, reduce font size and/or move gray line + verse down until there is a clear gap.
- Final media ≈ **1408×768** PNG; no text/logos/watermarks in the image; **no** `alphaModFix` dimming on the photo.

### 6. Texto Bíblico

- Black background; `Texto Bíblico` white centered.
- Citation + verse numbers **yellow** (`FFFF00`) and **bold**; verse text **white** regular.
- Citation only on first slide of passage; continuations = verses only.
- **Pack whole verses densely** up to the logo — combine short verses on one slide; **never** orphan a short verse on its own slide; **never** split mid-verse.
- Same bold rule as Lectura: do not strip `b="1"` from citation/numbers when rewriting.

### 7. 1.A–3.B bodies

- Title `N.A- …` / `N.B- …`: **bold** Times New Roman ~40pt (`sz="4000"`) — preserve template bold.
- Body: Verlag Light ~42pt regular; pack dense to ~360–400 chars; **bold** inline refs only; **logo floor**.
- If the next sentence fits above the logo, it stays; only then open another slide.

---

## Workflow checklist

1. Confirm **3 current scans**. If next title-page scan is missing, **ask** for próximo number / title / base bíblica before finishing.  
2. Sort current by page #; extract outline; merge cross-page sections; extract próximo from scan 2 **or** from the user’s typed reply.  
3. Create `bible-studies/{N} - {TITLE}.pptx` from the audience template (never overwrite `template/`). Prepare JSON lives under `studies/{audience}/`.  
4. Title = current study from scans.  
5. Lectura — whole verses; citation only on first of each range.  
6. Propósitos + Idea/Memorizar (no duplicate footer citation in body). **Force `drawing2.xml` Propósitos body to `sz="3600"`** — spot-check Propósitos in the PDF.  
7. Comentario → Introducción — pack by sentences.  
8. **Generate** three section images immediately (no permission ask) in **this study’s** design family; full-bleed OK if title/verse stay readable (or use a light panel/scrim); insert; **always remove** leftover `Arc` dashed/dotted chrome from the clone.  
9. Points 1–3: section art → Texto → A/B.  
10. Conclusión → Próximo (from scan 2; multi-line base if `;`). **Force Times New Roman 42pt on conclusión body** (never leave Verlag Light from the clone).  
11. Pass: **no logo overflow**, **no sparse slides**, bold refs, no dimming, Lectura/Texto continuations without titles, whole verses only, **conclusión = Times New Roman 42pt**, **Propósitos boxes @ 36pt with no PDF ghost overflow**, **font/bold table above** (Base Bíblica label only bold; Lectura/Texto citation+numbers bold; A/B titles bold).  
12. **Run `mzbs validate` (or `python3 scripts/validate_pptx.py`) on the output** — must exit OK. Do not deliver if it fails.  
13. **Export PDF** — once the `.pptx` validates, run `mzbs export-pdf "bible-studies/{N} - {TITLE}.pptx"` (or `python3 scripts/export_pdf.py …`). Same folder, same basename, `.pdf` extension.  
14. User QA in PowerPoint (and optional PDF check).

---

## Keep this quality every time (efficiency)

These habits are why batch builds stay fast and clean — follow them; don’t reinvent:

1. **Batch PDF workflow** — preview the whole PDF (counts, titles, last study / no Próximo) → wait for OK → build all decks with one reusable builder script (clone known-good → fill text → section images → validate → export PDF). Do not hand-craft each deck from scratch.
2. **Clone, don’t invent layouts** — copy slide XML from `template/youth/master-template.pptx` or a recent good deck; only swap text/media.
3. **Hard gates before “done”** — `mzbs validate` OK + `mzbs export-pdf` + spot-check **Propósitos**, **Conclusión font**, **logo floor**, and **one section title/verse** in the PDF.
4. **Lock lessons into this file + `.cursor/rules/bible-study-pptx.mdc`** when a QA bug is fixed (like Propósitos 36pt). Future sessions read those first.
5. **One design family per study for section art; change it next study** — generate all three images up front; strip retired `Arc` chrome.
6. **Surgical fixes only** — when the user says “everything else is good,” patch only the broken slide/part (don’t regenerate the whole deck unless asked).
## Implementation notes

- Prefer Python + `xml.etree` for batch OOXML edits.  
- Gold path: duplicate slides from master-template → replace text → swap `image4/5/6` → rebuild rels/order.  
- Overwrite existing `bible-studies/{N} - {TITLE}.pptx` only when regenerating that study.  
- After a successful validate, export PDF with `mzbs export-pdf "bible-studies/{N} - {TITLE}.pptx"` (macOS + Microsoft PowerPoint; or `python3 scripts/export_pdf.py …`). Regenerate the PDF whenever the pptx is rebuilt. Prefer the Click CLI package (`pip install -e .` → `mzbs build|validate|export-pdf`).  
- Never hardcode one study filename as the permanent deck.

### Package integrity (MANDATORY — prevents “PowerPoint found a problem with content”)

PowerPoint repair dialogs mean the **ZIP package is inconsistent**. **Never deliver a deck until validation passes.**

#### Corruption causes we have already hit

| Cause | What happens |
|-------|----------------|
| ElementTree writes `[Content_Types].xml` as `ns0:Types` | Repair dialog |
| Content_Types Override for a slide file that is not in the zip | Repair dialog |
| Slide file in zip with no Content_Types Override | Repair dialog |
| `sldIdLst` count ≠ slide Relationships | Repair / missing slides |
| **Duplicating a slide but leaving `notesSlide{old}` in the new slide’s `.rels`** | Repair dialog (study 17) |
| **Zipping with `fn.startswith(".")` which drops `_rels/.rels`** | Repair dialog (study 18) |
| ElementTree renaming `p14:` → `ns4:` / broken prefixes | Sometimes repair / odd open |

#### Safe edit rules

1. **Clone a known-good deck** (`bible-studies/16 - ….pptx` or `template/youth/master-template.pptx`) — do not invent package structure.
2. **`[Content_Types].xml` and `presentation.xml.rels`**
   - Prefer **string surgery** only.
   - Must keep `<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">` and `<Relationships xmlns="…">`.
   - **Never** `ElementTree.write()` these files (emits `ns0:`).
3. **Unused slides:** prefer keeping unused `slide*.xml` in the zip (with Overrides), only omit from `sldIdLst` + slide rels — matches working decks. If you delete a slide file, also remove its `.rels`, Content_Types Override, and presentation rel **in the same edit**.
4. **Duplicating a slide (CRITICAL):**
   - Copy `slideN.xml` → `slideM.xml` and `slideN.xml.rels` → `slideM.xml.rels`.
   - In the new `.rels`, either **remove** the notes relationship, or retarget it to `notesSlideM.xml` (and copy/create that notes part).
   - **Never** leave `Target="../notesSlides/notesSlideN.xml"` on slide M.
   - Add Content_Types Override + presentation Relationship + `sldIdLst` entry (string edits).
5. **Slide XML:** preserve namespaces (`p14`, etc.) when possible; if ElementTree rewrites prefixes, fix them before zipping.
6. **Zipping (CRITICAL):** only skip `.DS_Store`, `._*`, and `~$*`. **Never** `fn.startswith(".")` — that omits `_rels/.rels` and corrupts the pptx.
7. Prefer editing text inside existing shapes; avoid rewriting whole slide trees unless necessary.

#### Final gate (required)

After every rezip, run:

```bash
mzbs validate "bible-studies/{N} - {TITLE}.pptx"
```

It must print `OK` and exit 0. Checks include: zip CRC, **`_rels/.rels` present**, no `ns0` in Content_Types/rels, Overrides ↔ files, `sldIdLst` ↔ slide rels, **notesSlide number matches slide number**, XML parse.

If it fails: **fix and re-run** — do not tell the user the deck is ready.

---

## Quick “don’ts”

- Don’t strip bold globally when fixing Base Bíblica — only the citation *lines* under `Base Bíblica:` are regular; Lectura/Texto citation+numbers, A/B titles, and section titles stay bold.  
- Don’t leave conclusión body in Verlag Light (or any non–Times New Roman) — always force `Times New Roman` / `sz="4200"`.  
- Don’t put text on or below the top of the Mount Zion Church logo (on any slide that has the logo).  
- Don’t draw guide/boundary lines on slides — the logo is the guide.  
- Don’t end a slide mid-sentence; move the whole overflow sentence to the next slide.  
- Don’t leave sparse slides when the next sentence/verse still fits above the logo.  
- Don’t split a verse across two Lectura/Texto slides.  
- Don’t skip merging mid-sentence sections across pages.  
- Don’t include Ideas para el maestro / Preguntas unless asked.  
- Don’t put Lectura/Texto citation titles on continuation slides.  
- Don’t ask the user before generating section images — just generate and insert them.  
- Don’t skip the PDF export after a validated `.pptx` — always write `bible-studies/{N} - {TITLE}.pdf`.  
- Don’t reuse the same section-image design across studies — one design family per study, change it for the next.  
- Don’t leave cloned yellow `Arc` dashed/dotted shapes on section slides — always strip them; that motif is retired.  
- Don’t let section title text overlap the verse under the gray line.  
- Don’t place unreadable title/verse over busy or low-contrast image areas — fix the art under the text or change the text color.  
- Don’t overwrite `template/`.  
- Don’t zip with `fn.startswith(".")` — that drops `_rels/.rels` and corrupts the pptx. Only skip `.DS_Store` / `._*` / `~$*`.  
- Don’t delete slide XML without also removing its Content_Types Override (and rels) — that corrupts the pptx.  
- Don’t duplicate a slide and leave `notesSlide{old}` in the new `.rels`.  
- Don’t `ElementTree.write()` `[Content_Types].xml` or `presentation.xml.rels`.  
- Don’t deliver a deck that fails `mzbs validate` / `scripts/validate_pptx.py`.  
- Don’t leave `ns0:` prefixes in `[Content_Types].xml` or `presentation.xml.rels`.
