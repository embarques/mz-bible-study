# lamad — Rust CLI (Mount Zion Church Bible Study)

**lamad** (Hebrew **לָמַד**) means *to learn by instruction, practice, or experience* — to become trained or accustomed; not mere information, but formation. (Related Piel form **לִמֵּד** *limmed*: *to teach*.)

Single binary for volunteers. **No Python at runtime.** The legacy Python package remains at [`python/mz_bible_study/`](../python/mz_bible_study/); this crate is the future volunteer tool.

Binary name: **`lamad`**. `lamad --help` prints the name’s meaning.

## Running the CLI (`cargo run` vs binary)

Two equivalent ways to invoke the same commands. Everything after `--` with `cargo run` is passed to `lamad` (like `go run . -- …`).

### A. During development — `cargo run`

Needs Rust + Cargo. Builds when sources change, then runs:

```bash
cd lamad-cli
cp config.example.toml config.toml   # paste API key(s); set agent_provider
# Optional: put the scan PDF in scans/pending/

cargo run -- doctor
cargo run -- prepare --from 23 --to 26    # full run → PPTX + PDF
cargo run -- prepare   # interactive form (TTY)
```

Faster binary (slower compile):

```bash
cargo run --release -- prepare -n 23
```

### B. Built binary — `./target/…/lamad` or volunteer zip

Build once, then call the executable directly (no `cargo` on each run):

```bash
cd lamad-cli
cargo build                  # → target/debug/lamad
# or: cargo build --release  # → target/release/lamad

./target/debug/lamad doctor
./target/debug/lamad prepare --from 23 --to 26
```

Volunteer zip packages already include a prebuilt `lamad` (or `lamad.exe`) next to `config.toml` — same flags, no Cargo required:

```bash
./lamad doctor
./lamad prepare --from 23 --to 26
```

## Commands

| Command | Purpose |
|---------|---------|
| `prepare` | **Default full run:** scan PDF → JSON + images → **PowerPoint + PDF** |
| `build` | Rebuild PPTX from existing JSON (+ optional PDF) |
| `validate` | Package integrity (must print `OK`) |
| `export-pdf` | Sibling PDF via PowerPoint AppleScript (macOS) |
| `review` | Optional cloud QA → `studies/{audience}/REVIEW.md` or `REVIEW-{N}.md` |
| `doctor` | Template, provider + API keys, scans/, PowerPoint, pdftoppm |

### Prepare

**One command gives you the deck.** Put a PDF in `scans/pending/`, then:

```bash
# Batch (3 pages per estudio; last study has no Próximo)
lamad prepare --from 20 --to 22

# Single study (PDF page 1 = estudio 23)
lamad prepare --from 23 --to 23
```

**You get:** `bible-studies/{N} - {TITLE}.pptx` and `.pdf` (PDF needs macOS + PowerPoint).

**Optional flags:**

| Flag | When to use |
|------|-------------|
| `--prepare-only` | JSON + images only — **no** PowerPoint (debug / re-run agent) |
| `--no-export-pdf` | PPTX only, skip PDF |
| `--review` | Also run cloud QA checklist after build |
| `-n N` | Find estudio N inside a multi-study PDF (OCR) |

```bash
# Power-user: agent only, no deck
lamad prepare --prepare-only --from 20 --to 22

# QA checklist later (deck must exist)
lamad review --from 20 --to 22

# ChatGPT / OpenAI backend instead of Cursor
lamad prepare --provider chatgpt --from 20 --to 22
```

More examples:

```bash
# Single via OCR discover (finds ESTUDIO N inside a multi-study PDF)
lamad prepare -n 25

# Single via page math when you know PDF page 1 = estudio 23
lamad prepare --from 23 -n 25
```

Study numbers come **only** from `--from`/`--to`/`-n` or the TUI — never from filenames.

While prepare runs you’ll see rustup-style progress on stderr: `info:` / `[1/4]` steps, spinning waits for the cloud agent (with elapsed time), and bars for rasterize / OCR / section images / artifact downloads. Agent narration is **off by default** (it was noisy); pass `--stream` only if you want the raw log.

Page math (`--from` / `--to`): study `n` with PDF start `F` uses pages `((n-F)*3+1)` … `+2`. The PDF must be long enough for the last prepared study; longer PDFs are fine.

OCR discover (`-n N` alone): rasterizes pages, finds title pages (`Base bíblica` + `Idea principal`), reads estudio numbers (with sequence fill from any clear anchor), then prepares that 3-page slice. Needs `tesseract` on PATH for image-only scans. Fallback if OCR fails: `--from F -n N`.

### Scans tray

```
scans/pending/   ← inbox: drop new PDFs here (unprocessed / in progress)
scans/complete/  ← moved here when the entire PDF was prepared successfully
scans/error/     ← `.log` on failure only; PDF stays in pending/ for retry
```

## Configuration

Load order: `--config PATH` → `./config.toml` → `config.toml` beside the binary → `~/.config/lamad/config.toml` (legacy: `~/.config/mzbs/config.toml`).

Env overrides: `CURSOR_API_KEY`, `OPENAI_API_KEY`, `LAMAD_AGENT_PROVIDER`. Never commit `config.toml` with secrets (see `.gitignore`).

```toml
# Backend: "cursor" (default) or "chatgpt" (alias: "openai")
agent_provider = "cursor"

cursor_api_key = "crsr_…"
cursor_model = "composer-2.5"

openai_api_key = "sk-…"
openai_model = "gpt-4o"
openai_image_model = "gpt-image-1"

audience = "youth"
export_pdf = true
# Bundled poppler — change if you move tools/
pdftoppm_path = "tools/pdftoppm"
```

`pdftoppm_path` may be absolute or relative to the app folder / `MZBS_ROOT`. On Windows use `tools/pdftoppm.exe`. If unset, the CLI still tries `tools/pdftoppm` then `PATH`.

| Provider | Prepare | Review | Keys |
|----------|---------|--------|------|
| **cursor** (default) | Cursor Cloud Agents → artifacts | Cursor agent → REVIEW.md | `cursor_api_key` / `CURSOR_API_KEY` |
| **chatgpt** | Vision JSON + Images API (3 PNGs) | Local checks + chat QA → REVIEW.md | `openai_api_key` / `OPENAI_API_KEY` |

## Architecture

- **CLI:** clap (derive)
- **TUI:** ratatui + crossterm, MVU (`tui/app.rs` update, `tui/ui.rs` view only)
- **Async:** tokio; OOXML/zip in `spawn_blocking`
- **HTTP:** reqwest (rustls) → **Cursor** Cloud Agents (`api.cursor.com`) *or* **OpenAI** Chat Completions + Images (`api.openai.com`), selected by `agent_provider`
- **Config:** TOML
- **Build:** port of Python youth builder (PROTO slides, pack, validate, Propósitos 36pt, Conclusión TNR 42pt)

Prompts embedded from `prompts/*.md`. Youth master PPTX loaded from disk (`template/youth/master-template.pptx`).

## Installation & deployment (start to finish)

### A. Build from source (maintainers)

1. Install Rust stable (`rustup`).
2. Clone the repo; checkout the branch that contains `lamad-cli/`.
3. For local `prepare` without a package zip: install **poppler** (`pdftoppm`) or run the package script so `tools/` is filled. **macOS + Microsoft PowerPoint** for PDF export only.
4. Build release:
   ```bash
   cd lamad-cli
   cargo build --release
   ```
   Binary: `target/release/lamad`
5. Smoke:
   ```bash
   ./target/release/lamad doctor
   ./target/release/lamad validate template/youth/master-template.pptx
   ```

### B. Volunteer zip (preferred — includes pdftoppm)

On **each** target OS (macOS, Linux, Windows), from `lamad-cli/`:

```bash
# Needs: Rust, plus poppler on the host (macOS: brew install poppler;
# Linux: poppler-utils). Windows poppler is downloaded by the script.
./scripts/package-release.sh
```

Creates `dist/lamad-app-<os>-<arch>/` and a `.zip` with:

```
lamad-app-<os>-<arch>/
  lamad(.exe)
  config.toml                 # pdftoppm_path already set; paste API key
  config.example.toml
  LEEME.md
  README.md
  tools/
    pdftoppm(.exe)            # + DLLs / lib/ as needed
    README.md
  template/youth/master-template.pptx
  scans/{pending,complete,error}/
  studies/youth/media/
  bible-studies/
```

Ship one zip per OS/arch. Volunteers:

1. Unzip; edit `config.toml` → set `agent_provider` and paste the matching API key.
2. Put the scan PDF in `scans/pending/`.
3. Run `./lamad prepare --from N --to M` (or `lamad.exe` on Windows).
4. Optional: `export MZBS_ROOT="/path/to/unzipped-folder"` if cwd is elsewhere.

**No separate poppler install** when using this zip. Change `pdftoppm_path` in `config.toml` if you relocate `tools/`.

PDF export: **macOS + PowerPoint only**. Windows/Linux packages set `export_pdf = false`; PPTX is still produced (export later on a Mac if needed).

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
| `OPENAI_API_KEY` | Overrides `openai_api_key` in config |
| `LAMAD_AGENT_PROVIDER` | Overrides `agent_provider` (`cursor` or `chatgpt`) |
| `MZBS_ROOT` | Project/data root (template, studies, bible-studies, scans) |

### E. Cloud agent notes

**Cursor (`agent_provider = "cursor"`):**
- Create agent: `POST https://api.cursor.com/v1/agents` with **no `repos`** (no-repo).
- Auth: Basic (`API_KEY:`) or Bearer.
- Prompt includes page images (≤5, ≤15MB each) + embedded PREPARE/AGENTS rules + assigned `section_style`.
- Agent must write JSON + 3 PNGs under `artifacts/`; CLI downloads them into `studies/{audience}/`.

**ChatGPT (`agent_provider = "chatgpt"`):**
- Vision chat (`openai_model`, default `gpt-4o`) extracts study JSON from scan images.
- Images API (`openai_image_model`, default `gpt-image-1`) generates three section PNGs; CLI resizes to ≈1408×768.
- Review uses local file checks + a chat QA pass (no Cloud Agent workspace).

There is **no** silent fallback to the Python CLI.

### F. Adult audience

`--audience adult` uses `template/adult/master-template.pptx` and the adult JSON schema
(`lectura_antifonal`, `temas` A/B, scenic + definición images under `studies/adult/{N}/`).
Youth remains the default.

## Status of this crate

| Area | Status |
|------|--------|
| clap CLI + doctor | Working |
| config.toml | Working (`pdftoppm_path`, …) |
| bundled pdftoppm (`tools/` + package-release) | Working |
| scans tray move complete/error | Working |
| pack_sentences / pack_verses | Working + tests |
| validate_pptx | Working (validates youth master) |
| section_styles catalog | Working |
| youth OOXML build | Working + **golden** `tests/build_golden.rs` |
| export-pdf (macOS) | Ported |
| Cursor HTTP client | Implemented (needs live API key to exercise) |
| OpenAI / ChatGPT prepare + review | Implemented (needs `OPENAI_API_KEY` to exercise) |
| prepare orchestration + page-count check | Working |
| Typed `Study` as build input | Working (`model/study.rs`) |
| TUI MVU form | Compiles; form → same `run()` |
| Adult builder | Explicit error |
| Python CLI | Untouched |
| Contributor docs | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## Dev layout

```
lamad-cli/            # crate folder (binary is lamad — not lamad-cli)
  Cargo.toml
  CONTRIBUTING.md
  scripts/package-release.sh
  src/…
  prompts/
  template/youth/
  tools/
  scans/
  tests/build_golden.rs
  config.example.toml
  LEEME.md
  README.md
```

**New contributors:** start with [`CONTRIBUTING.md`](CONTRIBUTING.md) (module map + coding norms). Do not dive into `src/build/ooxml/` until you need to.
