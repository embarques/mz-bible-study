"""Project root and default asset paths."""
from __future__ import annotations

import os
from pathlib import Path

_PKG_ROOT = Path(__file__).resolve().parent
_REPO_CANDIDATE = _PKG_ROOT.parent


def project_root() -> Path:
    """Locate the repo root that contains `template/master-template.pptx`."""
    env = os.environ.get("MZBS_ROOT")
    if env:
        return Path(env).expanduser().resolve()

    cwd = Path.cwd().resolve()
    for base in (cwd, *cwd.parents, _REPO_CANDIDATE):
        if (base / "template" / "master-template.pptx").exists():
            return base
    return _REPO_CANDIDATE


def master_template() -> Path:
    return project_root() / "template" / "master-template.pptx"


def bible_studies_dir() -> Path:
    return project_root() / "bible-studies"


def generated_dir() -> Path:
    """Gitignored scratch: page rasters, PDF previews, unzipped build work."""
    path = project_root() / "generated"
    path.mkdir(parents=True, exist_ok=True)
    return path
