"""Study audience (report type): youth vs adult."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Literal

Audience = Literal["youth", "adult"]
AUDIENCES: tuple[Audience, ...] = ("youth", "adult")
DEFAULT_AUDIENCE: Audience = "youth"


def normalize_audience(value: str | None) -> Audience:
    """Return a valid audience; default youth when missing/blank."""
    if value is None or not str(value).strip():
        return DEFAULT_AUDIENCE
    key = str(value).strip().lower()
    if key not in AUDIENCES:
        raise ValueError(
            f"unknown audience {value!r}; expected one of: {', '.join(AUDIENCES)}"
        )
    return key  # type: ignore[return-value]


def stamp_study_audience(
    json_path: Path,
    audience: str | None,
    *,
    template: Path | None = None,
    project_root: Path | None = None,
) -> Audience:
    """Write ``audience`` (and optional ``template``) into a study JSON file."""
    aud = normalize_audience(audience)
    data = json.loads(json_path.read_text(encoding="utf-8"))
    data["audience"] = aud
    if template is not None:
        tmpl = template.resolve()
        if project_root is not None:
            try:
                data["template"] = str(tmpl.relative_to(project_root.resolve()))
            except ValueError:
                data["template"] = str(tmpl)
        else:
            data["template"] = str(tmpl)
    else:
        data.pop("template", None)
    json_path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return aud
