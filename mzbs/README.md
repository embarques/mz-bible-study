# mzbs — Rust CLI (Monte de Sion Bible Study)

Single binary for volunteers. **No Python at runtime.** The legacy Python package remains at [`python/mz_bible_study/`](../python/mz_bible_study/); this crate is the future volunteer tool.

## Quick start (developers)

```bash
cd mzbs
cp config.example.toml config.toml   # paste cursor_api_key
cargo run -- doctor
cargo run -- prepare --from 23 --to 26 --pdf /path/to/scan.pdf
# Interactive form (TTY):
cargo run -- prepare
```

Binary name: **`mzbs`**.

## Commands

| Command | Purpose |
|---------|---------|
| `prepare` | Unified batch/single: cloud agent → JSON + 3 section PNGs → build → PDF → review |
| `build` | JSON + images → PPTX (+ optional PDF) |
| `validate` | Package integrity (must print `OK`) |
| `export-pdf` | Sibling PDF via PowerPoint AppleScript (macOS) |
| `review` | Re-run cloud QA → `studies/{audience}/REVIEW.md` or `REVIEW-{N}.md` |
| `doctor` | Template, API key, scans/, PowerPoint, pdftoppm |

### Prepare

```bash
# Batch (3 pages per estudio; last study has no Próximo)
mzbs prepare --from 20 --to 22
# PDF optional if exactly one *.pdf sits in scans/

# Single
mzbs prepare -n 22 --pdf scan.pdf
# same as --from 22 --to 22

mzbs prepare --prepare-only --from 20 --to 22
mzbs prepare --no-review --no-export-pdf --from 20 --to 22
```

Study numbers come **only** from `--from`/`--to`/`-n` or the TUI — never from filenames or OCR.

Page math: study `n` with `--from F` uses pages `((n-F)*3+1)` … `+2`. PDF page count must equal `3 * (to - from + 1)`.

### Scans tray

```
scans/           ← drop PDFs here
scans/complete/  ← moved here on success
scans/error/     ← moved here on failure (+ .log)
```

## Configuration

Load order: `--config PATH` → `./config.toml` → `config.toml` beside the binary → `~/.config/mzbs/config.toml`.

`CURSOR_API_KEY` env overrides the file key. Never commit `config.toml` with secrets (see `.gitignore`).

```toml
cursor_api_key = "crsr_…"
cursor_model = "composer-2.5"
audience = "youth"
export_pdf = true
```

## Architecture

- **CLI:** clap (derive)
- **TUI:** ratatui + crossterm, MVU (`tui/app.rs` update, `tui/ui.rs` view only)
- **Async:** tokio; OOXML/zip in `spawn_blocking`
- **HTTP:** reqwest (rustls) → Cursor Cloud Agents API (`api.cursor.com`), **no-repo** agents (omit `repos`)
- **Config:** TOML
- **Build:** port of Python youth builder (PROTO slides, pack, validate, Propósitos 36pt, Conclusión TNR 42pt)

Prompts embedded from `prompts/*.md`. Youth master PPTX loaded from disk (`template/youth/master-template.pptx`).

## Installation & deployment (start to finish)

### A. Build from source (maintainers)

1. Install Rust stable (`rustup`).
2. Clone the repo; checkout the branch that contains `mzbs/`.
3. Optional system deps for prepare:
   - **poppler** (`pdftoppm`) — rasterize PDF pages for the cloud agent
   - **macOS + Microsoft PowerPoint** — PDF export only
4. Build release:
   ```bash
   cd mzbs
   cargo build --release
   ```
   Binary: `target/release/mzbs`
5. Smoke:
   ```bash
   ./target/release/mzbs doctor
   ./target/release/mzbs validate ../template/youth/master-template.pptx
   ```

### B. Volunteer zip layout (distribute)

Ship a folder (zip) volunteers can unzip anywhere:

```
mzbs-app/
  mzbs                          # release binary
  config.example.toml
  LEEME.md
  README.md                     # this file (or a short pointer)
  prompts/                      # optional; prompts are embedded, but handy for humans
  template/youth/master-template.pptx
  scans/
    complete/.gitkeep
    error/.gitkeep
```

1. Copy `config.example.toml` → `config.toml`, paste Cursor API key.
2. Install **poppler** if prepare will run (`brew install poppler` on macOS).
3. Put the unit scan PDF in `scans/`.
4. Run `./mzbs prepare --from N --to M` (or `./mzbs prepare` for the form).
5. Outputs land next to the app when `MZBS_ROOT` is unset and the folder contains `template/` — or set `MZBS_ROOT` to the unzipped folder.

Suggested:

```bash
export MZBS_ROOT="/path/to/mzbs-app"
./mzbs doctor
./mzbs prepare --from 23 --to 26
```

Deliverables:

- `studies/youth/{N}.json` + `studies/youth/media/{N}-section{1,2,3}.png`
- `bible-studies/{N} - {TITLE}.pptx` (+ `.pdf` on macOS with PowerPoint)
- Review: `studies/youth/REVIEW.md` (batch) or `REVIEW-{N}.md` (single)

### C. Code signing / notarization

**Out of scope** for this PR. For macOS Gatekeeper, sign and notarize the binary before wide distribution (Apple Developer ID). Homebrew formula also out of scope.

### D. Environment variables

| Var | Meaning |
|-----|---------|
| `CURSOR_API_KEY` | Overrides `cursor_api_key` in config |
| `MZBS_ROOT` | Project/data root (template, studies, bible-studies, scans) |

### E. Cloud agent notes

- Create agent: `POST https://api.cursor.com/v1/agents` with **no `repos`** (no-repo).
- Auth: Basic (`API_KEY:`) or Bearer.
- Prompt includes page images (≤5, ≤15MB each) + embedded PREPARE/AGENTS rules + assigned `section_style`.
- Agent must write JSON + 3 PNGs under `artifacts/`; CLI downloads them into `studies/{audience}/`.
- There is **no** silent fallback to the Python CLI.

### F. Adult audience

`--audience adult` errors with a clear message. Youth is the default and only implemented builder.

## Status of this crate

| Area | Status |
|------|--------|
| clap CLI + doctor | Working |
| config.toml | Working |
| scans tray move complete/error | Working |
| pack_sentences / pack_verses | Working + tests |
| validate_pptx | Working (validates youth master) |
| section_styles catalog | Working |
| youth OOXML build | Ported (needs live JSON fixtures to golden-test) |
| export-pdf (macOS) | Ported |
| Cursor HTTP client | Implemented (needs live API key to exercise) |
| prepare orchestration + page-count check | Working |
| TUI MVU form | Compiles; form → same `run()` |
| Adult builder | Explicit error |
| Python CLI | Untouched |

## Dev layout

```
mzbs/
  Cargo.toml
  src/…           # see module list in lib.rs
  prompts/        # copies of AGENTS.md, PREPARE_STUDY.md, REVIEW.md
  template/youth/ # copy of master-template.pptx
  scans/
  config.example.toml
  LEEME.md
  README.md
```
