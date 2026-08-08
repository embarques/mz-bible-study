from __future__ import annotations

import json
from pathlib import Path

import click

from mz_bible_study.build.study import build_study
from mz_bible_study.paths import master_template, project_root
from mz_bible_study.prepare import (
    PrepareError,
    default_pages,
    run_prepare_agent,
    suggested_build_command,
)
from mz_bible_study.section_styles import section_style_for_study


@click.command("from-pdf")
@click.option(
    "--pdf",
    required=True,
    type=click.Path(exists=True, dir_okay=False, path_type=Path),
    help="Batch PDF with all studies in order (3 content pages each).",
)
@click.option(
    "--from",
    "from_study",
    required=True,
    type=int,
    help="First estudio number in the PDF (e.g. 20).",
)
@click.option(
    "--to",
    "to_study",
    required=True,
    type=int,
    help="Last estudio number in the PDF (e.g. 22).",
)
@click.option(
    "--build/--prepare-only",
    "do_build",
    default=True,
    help="Prepare + build + PDF for every study (default). Use --prepare-only for JSON/images only.",
)
@click.option(
    "--export-pdf/--no-export-pdf",
    default=True,
    help="When building, also export PDF (default: on).",
)
@click.option(
    "--model",
    default="composer-2.5",
    show_default=True,
    help="Cursor agent model id.",
)
@click.option(
    "--api-key",
    default=None,
    envvar="CURSOR_API_KEY",
    help="Cursor API key (or set CURSOR_API_KEY).",
)
@click.option(
    "--stream/--no-stream",
    default=True,
    help="Stream agent assistant text while each study is prepared.",
)
@click.option(
    "--base",
    type=click.Path(exists=True, dir_okay=False, path_type=Path),
    default=None,
    help="Prototype pptx for builds (default: master-template.pptx).",
)
@click.option(
    "--stop-on-error/--continue-on-error",
    default=True,
    help="Stop the batch if one study fails (default).",
)
@click.option(
    "--review/--no-review",
    default=True,
    help="After the batch, run a Cursor agent QA pass (default: on).",
)
def from_pdf_cmd(
    pdf: Path,
    from_study: int,
    to_study: int,
    do_build: bool,
    export_pdf: bool,
    model: str,
    api_key: str | None,
    stream: bool,
    base: Path | None,
    stop_on_error: bool,
    review: bool,
) -> None:
    """Prepare (and by default build) every study in a batch PDF, then agent-review."""
    if to_study < from_study:
        raise click.UsageError("--to must be >= --from")

    studies = list(range(from_study, to_study + 1))
    click.echo(
        f"Batch PDF {pdf.name}: estudios {from_study}–{to_study} "
        f"({len(studies)} studies); "
        f"{'prepare+build' if do_build else 'prepare only'}"
        f"{'; +review' if review else ''}"
    )

    built: list[Path] = []
    errors: list[str] = []

    for study in studies:
        is_last = study == to_study
        pages = default_pages(study, from_study)
        next_pages = None if is_last else default_pages(study + 1, from_study)

        click.secho(
            f"\n=== Estudio {study} pages {pages[0]}–{pages[1]}"
            + (" (last, no Próximo)" if is_last else f"; próximo from pages {next_pages[0]}–{next_pages[1]}")
            + " ===",
            fg="cyan",
        )
        style = section_style_for_study(study)
        click.echo(f"  Section style: {style.id} — {style.name}")

        try:
            paths = run_prepare_agent(
                study=study,
                pdf=pdf,
                pages=pages,
                omit_proximo=is_last,
                proximo=None,
                next_pages=next_pages,
                model=model,
                api_key=api_key,
                stream=stream,
            )
            for label, path in paths.items():
                click.echo(f"  {label}: {path.relative_to(project_root())}")

            if do_build:
                data = json.loads(paths["json"].read_text(encoding="utf-8"))
                title = data.get("titulo") or "TITLE"
                out = project_root() / "bible-studies" / f"{study} - {title}.pptx"
                click.echo(f"  Building {out.relative_to(project_root())}…")
                build_study(
                    paths["json"],
                    out,
                    base=base or master_template(),
                    export_pdf=export_pdf,
                )
                built.append(out)
            else:
                click.echo("  Next:\n" + suggested_build_command(study))
        except (PrepareError, Exception) as exc:
            msg = f"estudio {study}: {exc}"
            errors.append(msg)
            click.secho(f"FAIL: {msg}", fg="red", err=True)
            if stop_on_error:
                raise click.ClickException(msg) from exc

    click.echo("")
    if errors:
        click.secho(f"Finished with {len(errors)} error(s).", fg="red")
        for e in errors:
            click.echo(f"  - {e}")
        raise SystemExit(1)

    click.secho(
        f"Done: {len(studies)} prepared"
        + (f", {len(built)} built" if do_build else "")
        + ".",
        fg="green",
    )

    if review:
        from mz_bible_study.review import ReviewError, run_review_agent

        click.secho("\n=== Agent review ===", fg="cyan")
        try:
            report = run_review_agent(
                studies=studies,
                source_pdf=pdf,
                expect_pptx=do_build,
                model=model,
                api_key=api_key,
                stream=stream,
            )
            click.secho(
                f"Review PASS — {report.relative_to(project_root())}",
                fg="green",
            )
        except ReviewError as exc:
            raise click.ClickException(str(exc)) from exc
