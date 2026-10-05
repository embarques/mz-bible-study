from __future__ import annotations

from pathlib import Path

import click

from mz_bible_study.validate import validate_pptx


@click.command("validate")
@click.argument("pptx", type=click.Path(exists=True, dir_okay=False, path_type=Path))
def validate_cmd(pptx: Path) -> None:
    """Validate a Bible-study PPTX package (must print OK)."""
    code = validate_pptx(pptx)
    if code != 0:
        raise SystemExit(code)
