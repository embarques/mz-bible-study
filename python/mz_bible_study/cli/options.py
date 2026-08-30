"""Shared Click options for audience + template path."""
from __future__ import annotations

from pathlib import Path
from typing import Callable, TypeVar

import click

from mz_bible_study.audience import AUDIENCES, DEFAULT_AUDIENCE

F = TypeVar("F", bound=Callable[..., object])


def audience_option(func: F) -> F:
    return click.option(
        "-a",
        "--audience",
        type=click.Choice(list(AUDIENCES), case_sensitive=False),
        default=DEFAULT_AUDIENCE,
        show_default=True,
        help="Report type / template set: youth (juveniles) or adult.",
    )(func)


def template_option(func: F) -> F:
    return click.option(
        "--template",
        type=click.Path(exists=True, dir_okay=False, path_type=Path),
        default=None,
        help="Explicit master PPTX to clone (overrides --audience built-in path).",
    )(func)


def base_alias_option(func: F) -> F:
    """Legacy alias for --template."""
    return click.option(
        "--base",
        type=click.Path(exists=True, dir_okay=False, path_type=Path),
        default=None,
        help="Alias for --template (legacy).",
    )(func)


def resolve_template_path(template: Path | None, base: Path | None) -> Path | None:
    return template or base
