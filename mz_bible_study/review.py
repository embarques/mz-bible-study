"""Post-build Cursor agent QA for generated studies."""
from __future__ import annotations

import os
import re
from pathlib import Path

from mz_bible_study.env import resolve_cursor_api_key
from mz_bible_study.audience import DEFAULT_AUDIENCE, normalize_audience
from mz_bible_study.paths import project_root, studies_dir
from mz_bible_study.prepare import PrepareError, expected_paths


class ReviewError(RuntimeError):
    """Review agent failed or reported FAIL."""


def find_built_deck(study: int) -> tuple[Path | None, Path | None]:
    """Return (pptx, pdf) for study N if present under bible-studies/."""
    root = project_root()
    bs = root / "bible-studies"
    if not bs.is_dir():
        return None, None
    pptx = None
    for p in sorted(bs.glob(f"{study} - *.pptx")):
        if p.name.startswith("~$"):
            continue
        # Prefer the non-_v1 deck; take the last match alphabetically as a stable pick
        if pptx is None or "_v1" not in p.stem:
            pptx = p
    pdf = pptx.with_suffix(".pdf") if pptx and pptx.with_suffix(".pdf").exists() else None
    if pdf is None:
        for p in sorted(bs.glob(f"{study} - *.pdf")):
            pdf = p
            break
    return pptx, pdf


def build_review_prompt(
    *,
    studies: list[int],
    source_pdf: Path | None,
    expect_pptx: bool,
    audience: str = DEFAULT_AUDIENCE,
) -> str:
    root = project_root()
    aud = normalize_audience(audience)
    rows: list[str] = []
    for n in studies:
        paths = expected_paths(n, aud)
        pptx, pdf = find_built_deck(n)
        media_dir = f"studies/{aud}/media"
        rows.append(
            f"### Estudio {n}\n"
            f"- json: `{paths['json'].relative_to(root) if paths['json'].exists() else 'MISSING'}`\n"
            f"- images: "
            f"`{paths['img1'].name if paths['img1'].exists() else 'MISSING'}`, "
            f"`{paths['img2'].name if paths['img2'].exists() else 'MISSING'}`, "
            f"`{paths['img3'].name if paths['img3'].exists() else 'MISSING'}` "
            f"under `{media_dir}/`\n"
            f"- pptx: `{pptx.relative_to(root) if pptx else 'MISSING'}`\n"
            f"- pdf: `{pdf.relative_to(root) if pdf else 'MISSING'}`\n"
        )

    pdf_line = (
        f"- Source PDF for content checks: `{source_pdf}`\n"
        if source_pdf
        else "- No source PDF path provided (still QA JSON + decks).\n"
    )
    expect = (
        "Expect finished `bible-studies/{N} - {TITLE}.pptx` (+ `.pdf` when present) for each study."
        if expect_pptx
        else "Prepare-only run: JSON + section images required; pptx/pdf optional."
    )

    return f"""You are QA for Monte de Sion Bible-study CLI output (audience={aud}).

{expect}
{pdf_line}

## Studies to review
{chr(10).join(rows)}

## Checklist (per study)
1. **Files exist** — JSON under `studies/{aud}/`, 3 section PNGs under `studies/{aud}/media/`; if pptx expected, pptx present (and pdf if built with export).
2. **JSON** — `numero` matches; `audience` is `{aud}`; required fields present (`titulo`, `base_biblica`, `lectura`, `propositos`×3, `idea_principal`, `para_memorizar`, `puntos`×3 with A/B, conclusión; `proximo` only if not last). `section_images` lists the 3 PNGs under `studies/{aud}/media/`. **`section_style.id` must match the CLI-assigned family for that estudio** (see `mz_bible_study/section_styles.py` — rotates; neighboring estudios must not share the same id).
3. **Validate pptx** — run `mzbs validate "<pptx>"` (or `.venv/bin/mzbs validate …`). Must print OK.
4. **OOXML spot-check** (unzip / inspect XML if needed):
   - Propósitos `drawing2.xml` body runs `sz="3600"` (not 5600)
   - Conclusión body = Times New Roman 42pt
   - Title `CuadroTexto 13` = Verlag Black **without** `b="1"`
   - Texto Bíblico: citation + verse numbers yellow (`FFFF00`) bold; body white (`FFFFFF`); `solidFill` **before** `latin` in `rPr`
   - Lectura: red citation/numbers; black body
   - No `ns0:` in slide / presentation rels XML
5. **PDF spot-check** (if pdf exists): Texto Bíblico pages must not be blank black; Propósitos boxes readable (no ghost overflow).
6. **Section image style** — open each `studies/{aud}/media/{{N}}-section*.png` (or the section slides in the PDF). They must look like **finished slide backgrounds**: shared design family for the study, and a clear left text-safe treatment (mist/parchment/wash/etc.) so title+verse read on a calm area — **not** a busy undressed full-bleed under the orange bar. Fail the study if images are bare scenes without that treatment. Different studies should not clone the same fade motif.
7. **Content** — if source PDF given, quick check that title / base bíblica / section titles match the printed study (not a different estudio).

## What to do
- Fix **safe mechanical** issues you find (OOXML color/font/bold, missing validate/export) and re-validate.
- Do **not** invent theology; only fix packaging/render bugs or obvious path mistakes.
- Write a short report to `studies/{aud}/REVIEW.md` with overall **PASS** or **FAIL**, one section per estudio, and any fixes applied.
- End your final message with a single line: `REVIEW_STATUS: PASS` or `REVIEW_STATUS: FAIL`.

Also follow AGENTS.md hard rules (logo floor, packing) at a high level—if slides are clearly sparse/overflow from JSON packing mistakes, note FAIL and describe what to fix in the JSON (you may edit JSON and rebuild with `mzbs build` if the issue is clear).
"""


def run_review_agent(
    *,
    studies: list[int],
    source_pdf: Path | None,
    expect_pptx: bool,
    model: str,
    api_key: str | None,
    stream: bool = True,
    audience: str = DEFAULT_AUDIENCE,
) -> Path:
    """Run QA agent; returns path to studies/{{audience}}/REVIEW.md. Raises ReviewError on FAIL."""
    try:
        from cursor_sdk import Agent, CursorAgentError, LocalAgentOptions
    except ImportError as exc:
        raise ReviewError(
            "cursor-sdk is not installed. Run: pip install -e '.[prepare]'"
        ) from exc

    key = api_key or resolve_cursor_api_key()
    if not key:
        raise ReviewError(
            "CURSOR_API_KEY is not set. Add it to a project `.env` file "
            "(see .env.example) or export it. Create a key at "
            "https://cursor.com/dashboard/api"
        )

    aud = normalize_audience(audience)
    root = project_root()
    studies_dir(aud)
    prompt = build_review_prompt(
        studies=studies,
        source_pdf=source_pdf.resolve() if source_pdf else None,
        expect_pptx=expect_pptx,
        audience=aud,
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
        msg = getattr(err, "message", str(err))
        raise ReviewError(f"review agent failed to start: {msg}") from err

    if getattr(result, "status", None) == "error":
        raise ReviewError(
            f"review agent run failed mid-flight (id={getattr(result, 'id', '?')})"
        )

    report = studies_dir(aud) / "REVIEW.md"
    status = _read_review_status(report, result)
    if status != "PASS":
        raise ReviewError(
            f"review reported {status or 'UNKNOWN'}. See {report.relative_to(root)}"
        )
    return report


def _read_review_status(report: Path, result: object) -> str | None:
    text = ""
    if report.exists():
        text += report.read_text(encoding="utf-8")
    # Also scan final agent text if present
    for attr in ("result", "text"):
        val = getattr(result, attr, None)
        if callable(val):
            try:
                val = val()
            except Exception:
                val = None
        if isinstance(val, str):
            text += "\n" + val
    m = re.search(r"REVIEW_STATUS:\s*(PASS|FAIL)", text, re.I)
    if m:
        return m.group(1).upper()
    if report.exists() and re.search(r"\bPASS\b", report.read_text(encoding="utf-8")):
        # Prefer explicit FAIL if both
        body = report.read_text(encoding="utf-8")
        if re.search(r"\bFAIL\b", body) and not re.search(
            r"overall\s*\*?\*?PASS\*?\*?", body, re.I
        ):
            return "FAIL"
    return None


def _stream_run(run: object) -> None:
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
                    t = getattr(block, "text", "") or ""
                    if t:
                        print(t, end="", flush=True)
        print()
    except Exception:
        pass
