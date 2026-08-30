from __future__ import annotations

from pathlib import Path

import click

from mz_bible_study.export_pdf import export_one


@click.command("export-pdf")
@click.argument(
    "pptx",
    nargs=-1,
    required=True,
    type=click.Path(exists=True, dir_okay=False, path_type=Path),
)
def export_pdf_cmd(pptx: tuple[Path, ...]) -> None:
    """Export PPTX file(s) to sibling PDF via Microsoft PowerPoint (macOS)."""
    ok = 0
    for path in pptx:
        if path.name.startswith("~$"):
            click.echo(f"SKIP: {path.name}")
            continue
        try:
            export_one(path)
            ok += 1
        except Exception as exc:
            raise click.ClickException(f"{path}: {exc}") from exc
    if not ok:
        raise SystemExit(1)
