from __future__ import annotations

import click

from mz_bible_study.cli.build_cmd import build_cmd
from mz_bible_study.cli.export_pdf_cmd import export_pdf_cmd
from mz_bible_study.cli.prepare_cmd import prepare_cmd
from mz_bible_study.cli.review_cmd import review_cmd
from mz_bible_study.cli.validate_cmd import validate_cmd
from mz_bible_study.env import load_env
from mz_bible_study.version import __version__


@click.group(
    context_settings={"help_option_names": ["-h", "--help"]},
)
@click.version_option(version=__version__, prog_name="mzbs")
def cli() -> None:
    """Monte de Sion Bible-study PPTX toolkit."""
    load_env()


cli.add_command(prepare_cmd)
cli.add_command(build_cmd)
cli.add_command(validate_cmd)
cli.add_command(export_pdf_cmd)
cli.add_command(review_cmd)


def main() -> None:
    cli(prog_name="mzbs")


if __name__ == "__main__":
    main()
