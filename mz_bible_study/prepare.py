"""Run a Cursor agent to prepare study JSON + section images (PREPARE_STUDY.md)."""
from __future__ import annotations

import json
import os
import re
from pathlib import Path

from mz_bible_study.env import resolve_cursor_api_key
from mz_bible_study.paths import generated_dir, project_root
from mz_bible_study.section_styles import section_style_for_study, style_prompt_block


class PrepareError(RuntimeError):
    """Prepare failed (agent or missing deliverables)."""


def default_pages(study: int, first_study: int) -> tuple[int, int]:
    """3 content pages per study, sequential from first_study."""
    if study < first_study:
        raise PrepareError(f"study {study} is before first_study {first_study}")
    start = (study - first_study) * 3 + 1
    return start, start + 2


def parse_pages(spec: str) -> tuple[int, int]:
    m = re.fullmatch(r"\s*(\d+)\s*-\s*(\d+)\s*", spec)
    if not m:
        raise PrepareError(f"invalid --pages {spec!r}; use like 7-9")
    a, b = int(m.group(1)), int(m.group(2))
    if b < a:
        raise PrepareError(f"invalid page range {a}-{b}")
    return a, b


def build_prepare_prompt(
    *,
    study: int,
    pdf: Path,
    pages: tuple[int, int],
    omit_proximo: bool,
    proximo: dict | None,
    next_pages: tuple[int, int] | None = None,
) -> str:
    root = project_root()
    guide = (root / "PREPARE_STUDY.md").read_text(encoding="utf-8")
    try:
        pdf_disp: Path | str = pdf.resolve().relative_to(root.resolve())
    except ValueError:
        pdf_disp = pdf.resolve()

    if omit_proximo:
        proximo_block = "Próximo: **omit** (last study — no próximo object, or null)."
    elif proximo is not None:
        proximo_block = (
            "Próximo (required):\n"
            f"  - numero: {proximo['numero']}\n"
            f"  - titulo: {proximo['titulo']}\n"
            f"  - base_biblica: {proximo['base_biblica']}\n"
        )
    elif next_pages is not None:
        proximo_block = (
            "Próximo: read from the **next** study’s title page in the same PDF "
            f"(pages **{next_pages[0]}–{next_pages[1]}** — use the title layout / first page of that block "
            "for número, título, base bíblica). Put them in JSON `proximo`."
        )
    else:
        raise PrepareError("próximo metadata missing and no next_pages to read from")

    style_block = style_prompt_block(study)

    return f"""Follow PREPARE_STUDY.md in this repo (full text also below).

Prepare **Estudio {study}** for the mzbs CLI. Do **NOT** build a .pptx and do **NOT** run `mzbs build` unless the user already asked in this run — only write the deliverables.

## Inputs
- PDF: `{pdf_disp}` (absolute: `{pdf.resolve()}`)
- Pages: **{pages[0]}–{pages[1]}** (3 content pages for this study)
- {proximo_block}

## Deliverables (must exist when you finish)
1. `studies/{study}.json` — same schema as `studies/20.json`; faithful Spanish from the scans
2. `studies/media/{study}-section1.png`
3. `studies/media/{study}-section2.png`
4. `studies/media/{study}-section3.png`

Point `section_images` in the JSON at those three PNG paths (repo-relative).

## Scratch (do not leave in repo root)
- Put PDF page rasters, crops, OCR dumps, and other temp files under `generated/` only
  (e.g. `generated/pages/study{study}-page-{{n}}.png`).
- Never write `_page*.png`, `_study*_page*.png`, or preview folders at the project root.

## Section images — HARD (every report must look styled)
{style_block}

Also:
- Each PNG is a **finished slide background** (not a bare full-bleed dump under the title).
- Scene subject on the **right / center-right**; calm readable left for dark navy title + verse.
- 16:9 ≈1408×768.
- **No** text, letters, logos, watermarks, yellow dashed/dotted arcs.

## Rules
- Match AGENTS.md transcription: merge cross-page cuts; exclude Ideas para el maestro / Preguntas de reflexión.
- Pack body slides ~360–400 chars at sentence ends; Lectura/Texto = whole verses only.
- Create `studies/`, `studies/media/`, and `generated/` if missing.

When finished, print a short confirmation listing the four paths, the assigned `section_style.id`, and the exact `mzbs build` command for this study.

---
# PREPARE_STUDY.md

{guide}
"""


def expected_paths(study: int) -> dict[str, Path]:
    root = project_root()
    return {
        "json": root / "studies" / f"{study}.json",
        "img1": root / "studies" / "media" / f"{study}-section1.png",
        "img2": root / "studies" / "media" / f"{study}-section2.png",
        "img3": root / "studies" / "media" / f"{study}-section3.png",
    }


def verify_deliverables(study: int) -> dict[str, Path]:
    paths = expected_paths(study)
    missing = [str(p) for p in paths.values() if not p.exists()]
    if missing:
        raise PrepareError(
            "agent finished but deliverables missing:\n  - " + "\n  - ".join(missing)
        )
    data = json.loads(paths["json"].read_text(encoding="utf-8"))
    if int(data.get("numero", -1)) != study:
        raise PrepareError(
            f"{paths['json']} numero is {data.get('numero')!r}, expected {study}"
        )
    imgs = data.get("section_images") or []
    if len(imgs) != 3:
        raise PrepareError(
            f"{paths['json']} must list 3 section_images, got {len(imgs)}"
        )
    expected_style = section_style_for_study(study)
    style = data.get("section_style") or {}
    if not isinstance(style, dict) or style.get("id") != expected_style.id:
        raise PrepareError(
            f"{paths['json']} must include section_style.id={expected_style.id!r} "
            f"(assigned for estudio {study}); got {style!r}"
        )
    return paths


def run_prepare_agent(
    *,
    study: int,
    pdf: Path,
    pages: tuple[int, int],
    omit_proximo: bool,
    proximo: dict | None,
    model: str,
    api_key: str | None,
    stream: bool = True,
    next_pages: tuple[int, int] | None = None,
) -> dict[str, Path]:
    """Invoke Cursor local agent; return verified deliverable paths."""
    try:
        from cursor_sdk import Agent, CursorAgentError, LocalAgentOptions
    except ImportError as exc:
        raise PrepareError(
            "cursor-sdk is not installed. Run: pip install -e '.[prepare]'"
        ) from exc

    key = api_key or resolve_cursor_api_key()
    if not key:
        raise PrepareError(
            "CURSOR_API_KEY is not set. Add it to a project `.env` file "
            "(see .env.example) or export it. Create a key at "
            "https://cursor.com/dashboard/api"
        )

    pdf = pdf.expanduser().resolve()
    if not pdf.exists():
        raise PrepareError(f"PDF not found: {pdf}")

    root = project_root()
    (root / "studies" / "media").mkdir(parents=True, exist_ok=True)
    (generated_dir() / "pages").mkdir(parents=True, exist_ok=True)

    prompt = build_prepare_prompt(
        study=study,
        pdf=pdf,
        pages=pages,
        omit_proximo=omit_proximo,
        proximo=proximo,
        next_pages=next_pages,
    )

    try:
        with Agent.create(
            model=model,
            api_key=key,
            local=LocalAgentOptions(cwd=str(root)),
        ) as agent:
            run = agent.send(prompt)
            if stream:
                _stream_run(run)
            result = run.wait()
    except CursorAgentError as err:
        retry = getattr(err, "is_retryable", None)
        msg = getattr(err, "message", str(err))
        raise PrepareError(
            f"agent failed to start: {msg}"
            + (f" (retryable={retry})" if retry is not None else "")
        ) from err

    status = getattr(result, "status", None)
    if status == "error":
        rid = getattr(result, "id", "?")
        raise PrepareError(f"agent run failed mid-flight (id={rid})")

    return verify_deliverables(study)


def _stream_run(run: object) -> None:
    """Print assistant text live if the SDK exposes messages()."""
    messages = getattr(run, "messages", None)
    if messages is None:
        return
    try:
        for message in messages():
            if getattr(message, "type", None) != "assistant":
                continue
            payload = getattr(message, "message", None)
            content = getattr(payload, "content", None) if payload is not None else None
            if not content:
                continue
            for block in content:
                if getattr(block, "type", None) == "text":
                    text = getattr(block, "text", "") or ""
                    if text:
                        print(text, end="", flush=True)
        print()
    except Exception:
        pass


def suggested_build_command(study: int, titulo: str | None = None) -> str:
    paths = expected_paths(study)
    title = titulo
    if title is None and paths["json"].exists():
        try:
            title = json.loads(paths["json"].read_text(encoding="utf-8")).get("titulo")
        except Exception:
            title = None
    title = title or "TITLE"
    out = f"bible-studies/{study} - {title}.pptx"
    return (
        f"mzbs build studies/{study}.json \\\n"
        f'  -o "{out}" \\\n'
        f"  --export-pdf"
    )
