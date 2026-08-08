"""Load project `.env` into os.environ (does not override existing vars)."""
from __future__ import annotations

from pathlib import Path

from mz_bible_study.paths import project_root

_LOADED = False


def load_env(*, dotenv_path: Path | None = None) -> Path | None:
    """Load `.env` from the project root once. Returns the path if loaded."""
    global _LOADED
    if _LOADED:
        return None
    path = dotenv_path or (project_root() / ".env")
    if not path.exists():
        _LOADED = True
        return None
    try:
        from dotenv import load_dotenv
    except ImportError as exc:
        raise RuntimeError(
            "python-dotenv is required to load .env. "
            "Run: pip install -e '.[prepare]'"
        ) from exc
    load_dotenv(path, override=False)
    _LOADED = True
    return path


def resolve_cursor_api_key(explicit: str | None = None) -> str | None:
    """Return API key from CLI arg, environment, or project `.env`."""
    import os

    if explicit:
        return explicit
    load_env()
    return os.environ.get("CURSOR_API_KEY") or None
