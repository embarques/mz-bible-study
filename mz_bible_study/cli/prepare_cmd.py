from __future__ import annotations

import json
from pathlib import Path

import click

from mz_bible_study.audience import stamp_study_audience
from mz_bible_study.build.study import build_study
from mz_bible_study.cli.options import (
    audience_option,
    base_alias_option,
    resolve_template_path,
    template_option,
)
from mz_bible_study.paths import project_root
from mz_bible_study.prepare import (
    PrepareError,
    default_pages,
    parse_pages,
    run_prepare_agent,
    suggested_build_command,
)
from mz_bible_study.section_styles import section_style_for_study


@click.command("prepare")
@click.option(
    "--pdf",
    required=True,
    type=click.Path(exists=True, dir_okay=False, path_type=Path),
    help="Source PDF (batch or single-study pages).",
)
@click.option(
    "-n",
    "--study",
    "study_number",
    required=True,
    type=int,
    help="Estudio number to prepare (e.g. 22).",
)
@click.option(
    "--pages",
    default=None,
    help="Page range in the PDF, e.g. 7-9. Default: derived from --first-study.",
)
@click.option(
    "--first-study",
    default=None,
    type=int,
    help="First estudio number in the PDF (for automatic page math). Default: --study when --pages set, else required with --pages omitted.",
)
@click.option(
    "--last/--not-last",
    "is_last",
    default=False,
    help="Last study in the PDF → omit Próximo.",
)
@click.option("--proximo-numero", type=int, default=None, help="Next study number (if not --last).")
@click.option("--proximo-titulo", default=None, help="Next study title (if not --last).")
@click.option(
    "--proximo-base",
    default=None,
    help="Next study base bíblica (if not --last). Use ; between citations.",
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
    help="Stream agent assistant text while it runs.",
)
@click.option(
    "--build/--no-build",
    default=False,
    help="After prepare succeeds, run mzbs build automatically.",
)
@click.option(
    "-o",
    "--output",
    type=click.Path(dir_okay=False, path_type=Path),
    default=None,
    help="Output pptx when using --build (default: bible-studies/{N} - {TITLE}.pptx).",
)
@click.option(
    "--export-pdf/--no-export-pdf",
    default=True,
    help="With --build, also export PDF (default: on).",
)
@audience_option
@template_option
@base_alias_option
@click.option(
    "--review/--no-review",
    default=True,
    help="After --build, run agent QA (default: on when building).",
)
def prepare_cmd(
    pdf: Path,
    study_number: int,
    pages: str | None,
    first_study: int | None,
    is_last: bool,
    proximo_numero: int | None,
    proximo_titulo: str | None,
    proximo_base: str | None,
    model: str,
    api_key: str | None,
    stream: bool,
    build: bool,
    output: Path | None,
    export_pdf: bool,
    audience: str,
    template: Path | None,
    base: Path | None,
    review: bool,
) -> None:
    """Prepare studies/{audience}/{N}.json + section images via Cursor agent (PREPARE_STUDY.md)."""
    try:
        if pages:
            page_range = parse_pages(pages)
            if first_study is None:
                first_study = study_number  # unused for math when pages given
        else:
            if first_study is None:
                raise click.UsageError(
                    "Provide --pages (e.g. 7-9) or --first-study so pages can be computed."
                )
            page_range = default_pages(study_number, first_study)

        if is_last:
            proximo = None
        else:
            if proximo_numero is None or not proximo_titulo or not proximo_base:
                raise click.UsageError(
                    "Not --last: require --proximo-numero, --proximo-titulo, and --proximo-base "
                    "(or pass --last)."
                )
            bases = [b.strip() for b in proximo_base.split(";") if b.strip()]
            proximo = {
                "numero": proximo_numero,
                "titulo": proximo_titulo,
                "base_biblica": bases,
            }

        tmpl = resolve_template_path(template, base)
        style = section_style_for_study(study_number)
        click.echo(
            f"Preparing estudio {study_number} from {pdf.name} pages {page_range[0]}–{page_range[1]}…"
        )
        click.echo(f"Section style: {style.id} — {style.name}")
        click.echo(f"Audience: {audience}")
        paths = run_prepare_agent(
            study=study_number,
            pdf=pdf,
            pages=page_range,
            omit_proximo=is_last,
            proximo=proximo,
            model=model,
            api_key=api_key,
            stream=stream,
            audience=audience,
        )
        stamp_study_audience(
            paths["json"],
            audience,
            template=tmpl,
            project_root=project_root(),
        )
        click.secho("Prepare OK:", fg="green")
        for label, path in paths.items():
            click.echo(f"  {label}: {path.relative_to(project_root())}")

        cmd = suggested_build_command(study_number, audience=audience)
        click.echo("\nNext:")
        click.echo(cmd)

        if build:
            data = json.loads(paths["json"].read_text(encoding="utf-8"))
            title = data.get("titulo") or "TITLE"
            out = output or (
                project_root() / "bible-studies" / f"{study_number} - {title}.pptx"
            )
            click.echo(f"\nBuilding {out}…")
            build_study(
                paths["json"],
                out,
                base=tmpl,
                audience=audience,
                export_pdf=export_pdf,
            )
            if review:
                from mz_bible_study.review import ReviewError, run_review_agent

                click.secho("\n=== Agent review ===", fg="cyan")
                try:
                    report = run_review_agent(
                        studies=[study_number],
                        source_pdf=pdf,
                        expect_pptx=True,
                        model=model,
                        api_key=api_key,
                        stream=stream,
                        audience=audience,
                    )
                    click.secho(
                        f"Review PASS — {report.relative_to(project_root())}",
                        fg="green",
                    )
                except ReviewError as exc:
                    raise click.ClickException(str(exc)) from exc
    except PrepareError as exc:
        raise click.ClickException(str(exc)) from exc
