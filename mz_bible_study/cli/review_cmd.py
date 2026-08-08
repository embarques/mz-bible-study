from __future__ import annotations

from pathlib import Path

import click

from mz_bible_study.paths import project_root
from mz_bible_study.review import ReviewError, run_review_agent


@click.command("review")
@click.option(
    "--from",
    "from_study",
    required=True,
    type=int,
    help="First estudio number to review.",
)
@click.option(
    "--to",
    "to_study",
    required=True,
    type=int,
    help="Last estudio number to review.",
)
@click.option(
    "--pdf",
    type=click.Path(exists=True, dir_okay=False, path_type=Path),
    default=None,
    help="Optional source PDF for content double-check.",
)
@click.option(
    "--expect-pptx/--json-only",
    default=True,
    help="Require built pptx (default) or only JSON/images.",
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
    help="Stream agent assistant text while reviewing.",
)
def review_cmd(
    from_study: int,
    to_study: int,
    pdf: Path | None,
    expect_pptx: bool,
    model: str,
    api_key: str | None,
    stream: bool,
) -> None:
    """Agent QA: double-check prepared/built studies (writes studies/REVIEW.md)."""
    if to_study < from_study:
        raise click.UsageError("--to must be >= --from")
    studies = list(range(from_study, to_study + 1))
    click.echo(f"Reviewing estudios {from_study}–{to_study}…")
    try:
        report = run_review_agent(
            studies=studies,
            source_pdf=pdf,
            expect_pptx=expect_pptx,
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
