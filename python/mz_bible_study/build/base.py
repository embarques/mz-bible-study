"""Study builder protocol and registry."""
from __future__ import annotations

from pathlib import Path
from typing import Protocol

from mz_bible_study.audience import Audience, normalize_audience


class StudyBuilder(Protocol):
    """Audience-specific PPTX builder."""

    audience: Audience

    def build(
        self,
        study_path: Path,
        output: Path,
        *,
        base: Path,
        export_pdf: bool = False,
    ) -> Path:
        """Clone `base`, fill from JSON, write `output`."""
        ...


class AdultBuilderNotImplemented(Exception):
    """Adult report builder is a placeholder until the adult template is wired."""


def get_builder(audience: Audience | str) -> StudyBuilder:
    aud = normalize_audience(audience if isinstance(audience, str) else audience)
    if aud == "youth":
        from mz_bible_study.build.youth.builder import YouthBuilder

        return YouthBuilder()
    if aud == "adult":
        from mz_bible_study.build.adult.builder import AdultBuilder

        return AdultBuilder()
    raise ValueError(f"no builder for audience={aud!r}")
