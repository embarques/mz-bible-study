from __future__ import annotations

from pathlib import Path

import click

from mz_bible_study.build.study import build_study
from mz_bible_study.paths import master_template


@click.command("build")
@click.argument("study_json", type=click.Path(exists=True, dir_okay=False, path_type=Path))
@click.option(
    "-o",
    "--output",
    required=True,
    type=click.Path(dir_okay=False, path_type=Path),
    help="Output .pptx path.",
)
@click.option(
    "--base",
    type=click.Path(exists=True, dir_okay=False, path_type=Path),
    default=None,
    help="Prototype deck to clone (default: template/master-template.pptx).",
)
@click.option(
    "--export-pdf/--no-export-pdf",
    default=False,
    help="Also export sibling PDF via Microsoft PowerPoint (macOS).",
)
def build_cmd(
    study_json: Path,
    output: Path,
    base: Path | None,
    export_pdf: bool,
) -> None:
    """Build a study deck from JSON + section images."""
    try:
        build_study(
            study_json,
            output,
            base=base or master_template(),
            export_pdf=export_pdf,
        )
    except SystemExit:
        raise
    except Exception as exc:
        raise click.ClickException(str(exc)) from exc
