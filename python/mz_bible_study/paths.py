"""Project root and default asset paths."""
from __future__ import annotations

import os
from pathlib import Path

from mz_bible_study.audience import Audience, DEFAULT_AUDIENCE, normalize_audience

_PKG_ROOT = Path(__file__).resolve().parent
# python/mz_bible_study → repo root (fallback if cwd has no template/)
_REPO_CANDIDATE = _PKG_ROOT.parent.parent


def _looks_like_project_root(base: Path) -> bool:
    t = base / "template"
    return (
        (t / "youth" / "master-template.pptx").exists()
        or (t / "master-template.pptx").exists()
        or (t / "adult" / "master-template.pptx").exists()
    )


def project_root() -> Path:
    """Locate the repo root that contains the template folder."""
    env = os.environ.get("MZBS_ROOT")
    if env:
        return Path(env).expanduser().resolve()

    cwd = Path.cwd().resolve()
    search = (cwd, *cwd.parents, _PKG_ROOT, *_PKG_ROOT.parents, _REPO_CANDIDATE)
    for base in search:
        if _looks_like_project_root(base):
            return base
    return _REPO_CANDIDATE


def master_template(audience: Audience | str = DEFAULT_AUDIENCE) -> Path:
    """Return the built-in master PPTX for an audience."""
    aud = normalize_audience(audience if isinstance(audience, str) else audience)
    root = project_root()
    preferred = root / "template" / aud / "master-template.pptx"
    if preferred.exists():
        return preferred
    if aud == "youth":
        legacy = root / "template" / "master-template.pptx"
        if legacy.exists():
            return legacy
    raise FileNotFoundError(
        f"No master template for audience={aud!r}. "
        f"Expected {preferred}"
        + (" or template/master-template.pptx (youth legacy)." if aud == "youth" else ".")
    )


def bible_studies_dir(audience: Audience | str | None = None) -> Path:
    """Finished decks. Flat `bible-studies/` for now (audience split later if needed)."""
    return project_root() / "bible-studies"


def studies_dir(audience: Audience | str = DEFAULT_AUDIENCE) -> Path:
    """Prepare inputs: ``studies/{audience}/`` (JSON, media, REVIEW)."""
    aud = normalize_audience(audience if isinstance(audience, str) else audience)
    path = project_root() / "studies" / aud
    path.mkdir(parents=True, exist_ok=True)
    (path / "media").mkdir(parents=True, exist_ok=True)
    return path


def generated_dir() -> Path:
    """Gitignored scratch: page rasters, PDF previews, unzipped build work."""
    path = project_root() / "generated"
    path.mkdir(parents=True, exist_ok=True)
    return path
