# Prepare a study for `mzbs`

Two ways:

1. **CLI (recommended):** `mzbs prepare …` runs a Cursor agent that follows this file  
2. **Chat:** paste the prompt below into Cursor

---

## `mzbs from-pdf` (whole PDF)

Default: **prepare every study + build + export PDF + agent review**.

```bash
mzbs from-pdf --pdf "Bible Study 20-22.pdf" --from 20 --to 22
```

- Studies must be **3 pages each**, in order, starting at `--from`
- Middle studies: Próximo is read from the **next** study’s title page in the PDF  
- Last study (`--to`): Próximo omitted  
- `--prepare-only` skips build/PDF  
- `--no-review` skips the final QA agent  
- Review report: `studies/{audience}/REVIEW.md` (`REVIEW_STATUS: PASS|FAIL`)

Re-run QA anytime:

```bash
mzbs review --from 20 --to 22 --pdf "Bible Study 20-22.pdf"
```

## `mzbs prepare` (one study)

```bash
pip install -e ".[prepare]"
export CURSOR_API_KEY=…   # Cursor Dashboard → Integrations

# Estudio 22 (last in Bible Study 20-22.pdf)
mzbs prepare --pdf "Bible Study 20-22.pdf" -n 22 --pages 7-9 --last

# Then build (or add --build to prepare)
mzbs build studies/youth/22.json \
  -o "bible-studies/22 - MEJORA TU AUTOESTIMA.pptx" \
  --audience youth \
  --export-pdf
```

Not last study — pass próximo metadata:

```bash
mzbs prepare --pdf "Bible Study 20-22.pdf" -n 21 --pages 4-6 \
  --proximo-numero 22 \
  --proximo-titulo "MEJORA TU AUTOESTIMA" \
  --proximo-base "Efesios 1:3-14"
```

Omit `--pages` if you pass `--first-study` (page math: 3 pages per study from that number).

---

## What you tell the agent in chat (copy / paste)

```
Follow PREPARE_STUDY.md.

Prepare Estudio 22 for the mzbs CLI (do NOT build the pptx yourself).

Inputs:
- PDF: Bible Study 20-22.pdf
- Study 22 = pages 7–9 (3 content pages). Próximo: omit (last study in this PDF).

Deliver only:
1. studies/youth/22.json  (schema like studies/youth/20.json; faithful Spanish from the scans)
2. studies/youth/media/22-section1.png
3. studies/youth/media/22-section2.png
4. studies/youth/media/22-section3.png

Rules:
- Match AGENTS.md transcription rules (merge cross-page cuts; exclude Ideas para el maestro / Preguntas).
- Pack body slides ~360–400 chars; Lectura/Texto whole verses only.
- One design family for the 3 section images; different from prior studies; 16:9; no text/logos/arcs; title+verse area must stay readable.
- Point section_images in the JSON at those 3 PNGs.
- Put page rasters / OCR / crops under `generated/` only — never `_page*.png` or preview folders at repo root.
- Section PNGs must include a per-study left text-safe fade/treatment (not bare full-bleed under the title).
- When done, report the mzbs build command I should run. Do not run mzbs unless I ask.
```

Adjust the study number, PDF/pages, and Próximo lines for other estudios.

---

## For Estudio 22 specifically

| Item | Value |
|------|--------|
| Source PDF | `Bible Study 20-22.pdf` (repo root; gitignored) |
| Pages | **7–9** (studies are 3 pages each: 20→1–3, 21→4–6, 22→7–9) |
| Próximo | **Omit** (`"proximo": null` or omit the key) — 22 is last in this PDF |
| Output JSON | `studies/youth/22.json` (or `studies/adult/…` for adult) |
| Section art | `studies/youth/media/22-section{1,2,3}.png` |

---

## What must be produced

### 1. `studies/{audience}/{N}.json`

Mirror the shape of `studies/youth/20.json`:

| Field | Notes |
|-------|--------|
| `numero`, `titulo` | From title page |
| `base_biblica` | Array of lines (split on `;` if multiple) |
| `lectura` | `cita` + `versiculos` (`"N text…"`) |
| `lectura_slides` | Optional; else CLI packs verses (~280 chars) |
| `propositos` | Exactly 3 strings |
| `idea_principal` | String |
| `para_memorizar` | `{ "texto", "cita" }` — don’t duplicate cita in texto if it lives in the diagram footer |
| `comentario_slides` / `comentario` | Prefer pre-packed `*_slides` arrays (~360–400 chars, sentence ends) |
| `introduccion_slides` / `introduccion` | Same packing rules |
| `puntos` | Exactly 3: `n`, `titulo`, `rango`, `texto_biblico`, optional `texto_slides`, `A`/`B` with `titulo` + `slides` or `cuerpo` |
| `conclusion_slides` / `conclusion` | Same packing; inline refs stay as printed |
| `proximo` | `{ numero, titulo, base_biblica }` **or omit/null** if last study |
| `section_images` | 3 paths relative to repo root |
| `section_style` | **Required.** `{"id": "…", "name": "…"}` — must match the **assigned** family the CLI puts in the prepare prompt (rotates by estudio number; never invent a different id) |

**Include:** Lectura, propósitos, idea, memorizar, comentario, intro, points 1–3 (Texto + A/B), conclusión, próximo when applicable.  
**Exclude:** Ideas para el maestro, Preguntas de reflexión, footers.

Faithful Spanish — fix OCR noise; don’t paraphrase theology.

### 2. Three section images

- Paths: `studies/{audience}/media/{N}-section1.png` … `section3.png`
- **`mzbs prepare` / `mzbs from-pdf` assigns a style family** for that estudio number — follow it exactly (see prompt block `ASSIGNED section style`). Do not use the same style as the previous estudio; the CLI rotation already picks a different id.
- Each PNG is a **finished slide background** using that family’s left text-safe treatment
- Scene on the right; calm left for dark title + verse
- 16:9 (≈1408×768)
- **No** text, letters, logos, watermarks, yellow dashed/dotted arcs
- Themes match points 1 / 2 / 3 of **this** study
- Record the assigned style in JSON `section_style`

---

## Split of responsibility

| `mzbs prepare` / agent | `mzbs build` |
|--------------------------|--------------|
| Read scans/PDF | Clone master template, fill OOXML |
| Write JSON + pack slides | Validate + export PDF |
| Generate 3 section PNGs | |
| Stop before pptx (unless `--build`) | Open deck and QA |

Full deck craft rules live in `AGENTS.md`.
