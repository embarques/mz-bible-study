# Contributing to `mzbs` (Rust CLI)

Welcome. This guide is for **junior and first-time contributors**. You do **not** need to understand PowerPoint OOXML on day one.

## Quick map — where do I change X?

| I want to change… | Open this file |
|-------------------|----------------|
| CLI flags / subcommands | [`src/cli.rs`](src/cli.rs) |
| Config file fields | [`src/config.rs`](src/config.rs) |
| Study numbers / page math / `PrepareJob` | [`src/job.rs`](src/job.rs) |
| `scans/` tray (list / move complete/error) | [`src/scans.rs`](src/scans.rs) |
| Finding `template/`, `studies/`, `bible-studies/` | [`src/paths.rs`](src/paths.rs) |
| Sentence / verse packing budgets | [`src/pack.rs`](src/pack.rs) + helpers on [`src/model/study.rs`](src/model/study.rs) |
| Study JSON shape (fields) | [`src/model/study.rs`](src/model/study.rs) |
| Slide **order** / which PROTO slide gets content | [`src/build/study.rs`](src/build/study.rs) |
| Youth prototype slide numbers | [`src/build/proto.rs`](src/build/proto.rs) |
| Section-image style catalog | [`src/section_styles.rs`](src/section_styles.rs) |
| PPTX validate rules | [`src/validate.rs`](src/validate.rs) |
| PDF export (macOS AppleScript) | [`src/export.rs`](src/export.rs) |
| Cursor Cloud HTTP client | [`src/agent/client.rs`](src/agent/client.rs) |
| Prepare prompt + artifact download | [`src/agent/prepare.rs`](src/agent/prepare.rs) |
| Review prompt | [`src/agent/review.rs`](src/agent/review.rs) + [`prompts/REVIEW.md`](prompts/REVIEW.md) |
| TUI form (keys / model) | [`src/tui/app.rs`](src/tui/app.rs) |
| TUI drawing only | [`src/tui/ui.rs`](src/tui/ui.rs) |
| Prepare orchestration (batch loop) | [`src/lib.rs`](src/lib.rs) |
| Embedded agent rules | [`prompts/`](prompts/) |

### OOXML (advanced — ask for review)

| Task | Module under [`src/build/ooxml/`](src/build/ooxml/) |
|------|-----------------------------------------------------|
| Unzip / rezip / `ns0:` fix | `zip.rs` |
| Duplicate slides / order | `slides.rs` |
| Title / Base Bíblica | `title.rs` |
| Lectura / Texto verses | `verses.rs` |
| Section chrome, A/B, body | `body.rs` |
| Propósitos / Idea diagrams | `diagrams.rs` |
| Section PNG media | `images.rs` |
| Shape text helpers | `shape.rs` |
| Tiny XML scanner | `xml.rs` |

Read the HARD RULES at the top of [`src/build/ooxml/mod.rs`](src/build/ooxml/mod.rs) before editing. Wrong zip filters or `ns0:` prefixes corrupt decks.

## Architecture (one picture)

```
CLI / TUI ──► PrepareJob ──► lib::run()
                                │
                    ┌───────────┼───────────┐
                    ▼           ▼           ▼
              agent/prepare  build/study  agent/review
                    │           │
                    ▼           ▼
              studies/*.json  bible-studies/*.pptx
              + section PNGs       │
                                   ▼
                              validate + export-pdf
```

- **CLI and TUI both call the same** `mzbs::run(job, cfg)`.
- **TUI is MVU:** only `tui/app.rs` mutates state; `tui/ui.rs` only draws.
- **Build never invents theology** — it only packs and fills from JSON.

## Local setup

```bash
cd mzbs
cp config.example.toml config.toml   # optional for doctor/build tests
cargo test
cargo run -- doctor
```

System extras for full prepare: `pdftoppm` (poppler), Cursor API key, macOS + PowerPoint for PDF.

## How to add a small feature (checklist)

1. Find the row in the map above — edit **one** module when possible.
2. Add or extend a unit test next to the code (`#[cfg(test)]`).
3. Run `cargo test` and `cargo clippy --all-targets`.
4. If you touch OOXML or validate rules, also run:
   ```bash
   cargo test --test build_golden
   cargo run -- validate ../template/youth/master-template.pptx
   ```
5. Do **not** invent próximo metadata, estudio numbers, or adult-builder layouts.
6. Do **not** shell out to Python.

## Coding norms juniors should follow

- Prefer `anyhow::Result` + clear `bail!("…")` messages users can act on.
- Prefer typed `Study` over raw `serde_json::Value` (use `apply_study_value` only as an adapter).
- Keep functions small; put multi-arg bundles in a struct (see `PrepareOne` in `lib.rs`).
- Comments explain **why** (OOXML pitfalls, clap quirks), not what the next line does.
- Never filter zip entries with `startswith(".")`.
- Never invent study content — fail closed.

## Tests that matter

| Test | What it guards |
|------|----------------|
| `pack::*` | Sentence/verse budgets |
| `section_styles::*` | Rotating style ids |
| `agent::prepare::tests::*` | Artifact path mapping |
| `build::ooxml::tests::*` | solidFill order, Arc strip, verse split |
| `tests/build_golden.rs` | Real template → build → validate OK |

## PR tips

- One concern per PR when you can (docs vs OOXML vs CLI).
- Say what you tested (`cargo test`, doctor, golden).
- Link AGENTS.md rule if you fix a packaging bug.
