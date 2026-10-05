# Mount Zion Church Bible Study

Spanish Bible study PowerPoints for Mount Zion Church.

## CLI (`lamad` / `mzbs`)

### Rust — **lamad** (preferred for volunteers — [`lamad-cli/`](lamad-cli/))

**lamad** (Hebrew **לָמַד**) means “to learn” or “to study”.

```bash
cd lamad-cli
cp config.example.toml config.toml   # paste Cursor API key; pdftoppm_path = tools/pdftoppm
cargo run --release -- prepare --from 20 --to 22
# or: cargo build --release && ./target/release/lamad prepare …
```

**Volunteer deploy (mac / Windows / Linux):** run `./scripts/package-release.sh` on each OS → zip under `lamad-cli/dist/` includes the `lamad` binary + bundled `tools/pdftoppm` + template. See [`lamad-cli/README.md`](lamad-cli/README.md) and [`lamad-cli/LEEME.md`](lamad-cli/LEEME.md). No Python at runtime.

### Python (legacy — [`python/mz_bible_study/`](python/mz_bible_study/))

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -e ".[prepare]"   # includes cursor-sdk for `mzbs prepare`
```

Set `CURSOR_API_KEY` in a project `.env` (see [`.env.example`](.env.example)):

1. Open [cursor.com/dashboard/api](https://cursor.com/dashboard/api)
2. Create an **API Key** (format `crsr_…`)
3. Copy `.env.example` → `.env` and paste the key:

```bash
cp .env.example .env
# edit .env → CURSOR_API_KEY=crsr_…
```

`mzbs` (Python) loads `.env` automatically (existing shell exports still win).

```bash
# 1) Agent prepares JSON + section images
mzbs prepare --pdf "Bible Study 20-22.pdf" -n 22 --pages 7-9 --last

# Or prepare + build + PDF in one go:
mzbs prepare --pdf "Bible Study 20-22.pdf" -n 22 --pages 7-9 --last --build

# Or the whole PDF (prepare + build every study + agent review):
mzbs prepare --pdf "Bible Study 20-22.pdf" --from 20 --to 22
# Default audience is youth. Use --audience adult when that builder is ready.
# Each estudio gets a rotating section-image style (different every study).

# Override the master PPTX explicitly:
# mzbs build studies/youth/22.json -o "…" --template /path/to/custom.pptx

# Prepare-only batch (still reviews JSON/images unless --no-review):
mzbs prepare --pdf "Bible Study 20-22.pdf" --from 20 --to 22 --prepare-only

# Re-run QA later:
mzbs review --from 20 --to 22 --pdf "Bible Study 20-22.pdf"

# 2) Or build later yourself
mzbs build studies/youth/22.json -o "bible-studies/22 - TITLE.pptx" --export-pdf

mzbs validate "bible-studies/22 - TITLE.pptx"
mzbs export-pdf "bible-studies/22 - TITLE.pptx"
```

- **`mzbs prepare --from … --to …`:** batch — every study → prepare, build + PDF, then **agent review** (`studies/{audience}/REVIEW.md`)  
- **`mzbs prepare -n …`:** one study → JSON + section images under `studies/{audience}/`  
- **`--audience youth|adult`:** uses `template/{audience}/master-template.pptx` by default (youth if omitted). Pass `--template PATH` to override.  
- **`mzbs build`:** clone audience (or `--template`) master, pack slides, fill OOXML, validate, export PDF  
- **`mzbs review`:** Cursor agent double-checks outputs (files, validate, OOXML/PDF spot-checks)  

## What you provide

### Option A — one study (unchanged)

```
1. [3 scans of the current study]
2. [1 scan of the next study’s title page]
```

Or a small PDF with just those pages.

### Option B — whole unit from one PDF (new)

Send **one PDF** that contains **every study in order**.

**Before generating:** the agent inspects the PDF and reports what it sees (page count, how studies split, estudio numbers/titles, which is last / no Próximo). **Wait for your OK** before building any decks.

Then the agent:

1. Extracts all pages
2. Splits into studies (**3 content pages each**, in sequence)
3. Builds **every** deck into `bible-studies/`

**How Próximo works in a batch PDF:**

| Study in the PDF | Próximo estudio |
|------------------|-----------------|
| Not the last | Taken from the **next** study’s title page (first page of the next 3-page block / title layout) |
| **Last** study | Only 3 pages — **no** next study → **omit** the Próximo slide |

Detail:

- **1** (single) — three content pages (any order; numbered). From a PDF, pages are extracted automatically.
- **2** (single) — title page of the *following* study (number, title, base bíblica)
- **Batch PDF** — studies back-to-back; next study starts when the previous one’s 3 pages end; last study ends the document (3 pages only)

If you skip **2** on a *single* study that still needs Próximo, you’ll be asked for number, title, and bible verses.

Optional: one short note if something spans pages oddly, e.g. `2A spans pages 96–98`.

## What you get

A finished deck in **`bible-studies/`**, named like this:

`bible-studies/17 - UN ABRAZO QUE DA VIDA.pptx`

plus a matching PDF:

`bible-studies/17 - UN ABRAZO QUE DA VIDA.pdf`

Open the PowerPoint and check that text doesn’t run into the logo.

## Folders

| Folder | What’s in it |
|--------|----------------|
| [`bible-studies/`](bible-studies/) | Generated presentations |
| [`studies/`](studies/) | Prepare JSON + section images by audience (`youth/`, `adult/`; gitignored) |
| [`generated/`](generated/) | Scratch: page rasters, PDF previews, build work (gitignored) |
| [`template/`](template/) | Master templates by audience (`youth/`, `adult/`) |
| [`python/mz_bible_study/`](python/mz_bible_study/) | Legacy Python Click CLI |
| [`lamad-cli/`](lamad-cli/) | **Rust CLI** (`lamad` binary; Hebrew לָמַד, “to learn / to study”) |
| [`scripts/`](scripts/) | Thin wrappers (prefer `mzbs`) |
