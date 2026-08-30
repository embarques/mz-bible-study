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


def _run_review(
    *,
    studies: list[int],
    pdf: Path,
    expect_pptx: bool,
    model: str,
    api_key: str | None,
    stream: bool,
    audience: str,
) -> None:
    from mz_bible_study.review import ReviewError, run_review_agent

    click.secho("\n=== Agent review ===", fg="cyan")
    try:
        report = run_review_agent(
            studies=studies,
            source_pdf=pdf,
            expect_pptx=expect_pptx,
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


def _prepare_one_study(
    *,
    pdf: Path,
    study_number: int,
    page_range: tuple[int, int],
    is_last: bool,
    proximo: dict | None,
    next_pages: tuple[int, int] | None,
    model: str,
    api_key: str | None,
    stream: bool,
    audience: str,
    tmpl: Path | None,
    do_build: bool,
    output: Path | None,
    export_pdf: bool,
    review: bool,
) -> Path | None:
    style = section_style_for_study(study_number)
    click.echo(
        f"Preparing estudio {study_number} from {pdf.name} pages "
        f"{page_range[0]}–{page_range[1]}…"
    )
    click.echo(f"Section style: {style.id} — {style.name}")
    click.echo(f"Audience: {audience}")

    paths = run_prepare_agent(
        study=study_number,
        pdf=pdf,
        pages=page_range,
        omit_proximo=is_last,
        proximo=proximo,
        next_pages=next_pages,
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

    click.echo("\nNext:")
    click.echo(suggested_build_command(study_number, audience=audience))

    if not do_build:
        return None

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
        _run_review(
            studies=[study_number],
            pdf=pdf,
            expect_pptx=True,
            model=model,
            api_key=api_key,
            stream=stream,
            audience=audience,
        )
    return out


def _prepare_batch(
    *,
    pdf: Path,
    from_study: int,
    to_study: int,
    do_build: bool,
    export_pdf: bool,
    model: str,
    api_key: str | None,
    stream: bool,
    audience: str,
    tmpl: Path | None,
    stop_on_error: bool,
    review: bool,
) -> None:
    studies = list(range(from_study, to_study + 1))
    click.echo(
        f"Batch PDF {pdf.name}: estudios {from_study}–{to_study} "
        f"({len(studies)} studies); audience={audience}; "
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
        click.echo(f"  Audience: {audience}")

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
                audience=audience,
            )
            stamp_study_audience(
                paths["json"],
                audience,
                template=tmpl,
                project_root=project_root(),
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
                    base=tmpl,
                    audience=audience,
                    export_pdf=export_pdf,
                )
                built.append(out)
            else:
                click.echo("  Next:\n" + suggested_build_command(study, audience=audience))
        except (PrepareError, Exception) as exc:
            msg = f"estudio {study}: {exc}"
            errors.append(msg)
            click.secho(f"FAIL: {msg}", fg="red", err=True)
            if stop_on_error:
                raise click.ClickException(msg) from exc

    click.echo("")
    if errors:
        click.secho(f"Finished with {len(errors)} error(s).", fg="red")
        for err in errors:
            click.echo(f"  - {err}")
        raise SystemExit(1)

    click.secho(
        f"Done: {len(studies)} prepared"
        + (f", {len(built)} built" if do_build else "")
        + ".",
        fg="green",
    )

    if review:
        _run_review(
            studies=studies,
            pdf=pdf,
            expect_pptx=do_build,
            model=model,
            api_key=api_key,
            stream=stream,
            audience=audience,
        )


@click.command("prepare")
@click.option(
    "--pdf",
    required=True,
    type=click.Path(exists=True, dir_okay=False, path_type=Path),
    help="Source PDF (one study or a batch with 3 content pages per estudio).",
)
@click.option(
    "--from",
    "from_study",
    default=None,
    type=int,
    help="First estudio number in the PDF (batch mode; requires --to).",
)
@click.option(
    "--to",
    "to_study",
    default=None,
    type=int,
    help="Last estudio number in the PDF (batch mode; requires --from).",
)
@click.option(
    "-n",
    "--study",
    "study_number",
    default=None,
    type=int,
    help="Estudio number to prepare (single-study mode; omit when using --from/--to).",
)
@click.option(
    "--pages",
    default=None,
    help="Page range in the PDF, e.g. 7-9 (single mode). Default: derived from --first-study.",
)
@click.option(
    "--first-study",
    default=None,
    type=int,
    help="First estudio number in the PDF (single mode page math when --pages omitted).",
)
@click.option(
    "--last/--not-last",
    "is_last",
    default=False,
    help="Single mode: last study in the PDF → omit Próximo.",
)
@click.option(
    "--proximo-numero",
    type=int,
    default=None,
    help="Single mode: next study number (if not --last).",
)
@click.option(
    "--proximo-titulo",
    default=None,
    help="Single mode: next study title (if not --last).",
)
@click.option(
    "--proximo-base",
    default=None,
    help="Single mode: next study base bíblica (if not --last). Use ; between citations.",
)
@click.option(
    "--build/--no-build",
    "do_build",
    default=None,
    help="Also build pptx (+ PDF when enabled). Batch default: on. Single default: off.",
)
@click.option(
    "--prepare-only",
    is_flag=True,
    default=False,
    help="Batch alias for --no-build (prepare JSON/images only).",
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
    help="Stream agent assistant text while preparing.",
)
@audience_option
@template_option
@base_alias_option
@click.option(
    "--stop-on-error/--continue-on-error",
    default=True,
    help="Batch mode: stop if one study fails (default).",
)
@click.option(
    "--review/--no-review",
    default=True,
    help="Run agent QA after prepare (single: when --build; batch: end of run).",
)
@click.option(
    "-o",
    "--output",
    type=click.Path(dir_okay=False, path_type=Path),
    default=None,
    help="Single mode + --build: output pptx path (default: bible-studies/{N} - {TITLE}.pptx).",
)
def prepare_cmd(
    pdf: Path,
    from_study: int | None,
    to_study: int | None,
    study_number: int | None,
    pages: str | None,
    first_study: int | None,
    is_last: bool,
    proximo_numero: int | None,
    proximo_titulo: str | None,
    proximo_base: str | None,
    do_build: bool | None,
    prepare_only: bool,
    export_pdf: bool,
    model: str,
    api_key: str | None,
    stream: bool,
    audience: str,
    template: Path | None,
    base: Path | None,
    stop_on_error: bool,
    review: bool,
    output: Path | None,
) -> None:
    """Prepare study JSON + section images from a PDF (single or batch via --from/--to)."""
    is_batch = from_study is not None or to_study is not None

    if is_batch:
        if from_study is None or to_study is None:
            raise click.UsageError("Batch mode requires both --from and --to.")
        if study_number is not None:
            raise click.UsageError("Use --from/--to for batch mode, or -n/--study for one estudio.")
        if to_study < from_study:
            raise click.UsageError("--to must be >= --from.")
        if prepare_only and do_build is True:
            raise click.UsageError("Use either --build or --prepare-only, not both.")
        build_flag = False if prepare_only else (True if do_build is None else do_build)
        tmpl = resolve_template_path(template, base)
        try:
            _prepare_batch(
                pdf=pdf,
                from_study=from_study,
                to_study=to_study,
                do_build=build_flag,
                export_pdf=export_pdf,
                model=model,
                api_key=api_key,
                stream=stream,
                audience=audience,
                tmpl=tmpl,
                stop_on_error=stop_on_error,
                review=review,
            )
        except PrepareError as exc:
            raise click.ClickException(str(exc)) from exc
        return

    if study_number is None:
        raise click.UsageError(
            "Single-study mode: pass -n/--study, or use --from/--to for a batch."
        )

    build_flag = False if do_build is None else do_build
    if prepare_only:
        if build_flag:
            raise click.UsageError("--prepare-only cannot be used with --build in single-study mode.")
        build_flag = False

    try:
        if pages:
            page_range = parse_pages(pages)
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
        _prepare_one_study(
            pdf=pdf,
            study_number=study_number,
            page_range=page_range,
            is_last=is_last,
            proximo=proximo,
            next_pages=None,
            model=model,
            api_key=api_key,
            stream=stream,
            audience=audience,
            tmpl=tmpl,
            do_build=build_flag,
            output=output,
            export_pdf=export_pdf,
            review=review,
        )
    except PrepareError as exc:
        raise click.ClickException(str(exc)) from exc
