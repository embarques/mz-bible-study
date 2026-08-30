"""Rotating section-image design families — one assigned per estudio.

Prepare agents must follow the assigned family so every deck is styled and
adjacent studies do not share the same look.
"""
from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class SectionStyle:
    id: str
    name: str
    # Short recipe for image generation (baked into the PNG, not PowerPoint shapes)
    recipe: str


# Keep this list long enough that consecutive estudios never feel cloned.
SECTION_STYLES: tuple[SectionStyle, ...] = (
    SectionStyle(
        id="parchment-warm",
        name="Warm parchment panel",
        recipe=(
            "Aged cream/gold parchment soft fade on the LEFT ~40%; cinematic warm oil-painting "
            "biblical scene on the right; subject right/center-right; empty calm left for dark navy title."
        ),
    ),
    SectionStyle(
        id="lavender-mist",
        name="Cool lavender mist",
        recipe=(
            "Cool lavender/lilac mist dissolve on the LEFT ~40% into moody dusk watercolor; "
            "subject right; empty misty left for title; purple-blue twilight palette."
        ),
    ),
    SectionStyle(
        id="sage-paper",
        name="Sage paper wash",
        recipe=(
            "Soft sage-green / cream paper wash on the LEFT ~40%; bright Mediterranean daylight "
            "illustration on the right; clean luminous look; empty sage left for title."
        ),
    ),
    SectionStyle(
        id="ink-wash",
        name="Charcoal ink wash",
        recipe=(
            "Soft charcoal/ink-gray horizontal bloom on the LEFT ~40% like sumi wash; "
            "desaturated epic scene right; empty gray wash left for title; no color festivity."
        ),
    ),
    SectionStyle(
        id="dawn-gold",
        name="Dawn gold bloom",
        recipe=(
            "Radiant dawn-gold light bloom on the LEFT ~40% (soft sun haze, not a solid bar); "
            "scene emerges on the right in warm morning light; empty glowing left for title."
        ),
    ),
    SectionStyle(
        id="slate-fog",
        name="Cool slate fog",
        recipe=(
            "Cool slate-blue fog bank on the LEFT ~40%; rainy/overcast atmospheric painting right; "
            "empty fog left for title; muted teal-slate palette."
        ),
    ),
    SectionStyle(
        id="linen-cream",
        name="Linen cream panel",
        recipe=(
            "Textured linen/off-white fabric panel soft-edge on the LEFT ~40%; "
            "rich fresco-like scene right; empty linen left for title; quiet handmade feel."
        ),
    ),
    SectionStyle(
        id="ember-vignette",
        name="Ember night vignette",
        recipe=(
            "Deep warm vignette opening from LEFT with soft ember/amber glow (not solid black); "
            "night or torchlit scene right; leave LEFT lighter amber wash so dark navy title still reads."
        ),
    ),
    SectionStyle(
        id="aqua-glass",
        name="Soft aqua glass",
        recipe=(
            "Pale aqua/glass frost wash on the LEFT ~40%; clear hopeful daylight scene right; "
            "empty aqua left for title; airy cool palette (not purple, not parchment)."
        ),
    ),
    SectionStyle(
        id="terra-dust",
        name="Terra dust haze",
        recipe=(
            "Dusty terra-cotta / sandy haze dissolve LEFT ~40%; arid landscape scene right; "
            "empty dusty-cream left for title; earth pigment look."
        ),
    ),
)


def section_style_for_study(study: int) -> SectionStyle:
    """Deterministic unique family per estudio number (rotates through the catalog)."""
    if study < 1:
        study = 1
    return SECTION_STYLES[(study - 1) % len(SECTION_STYLES)]


def style_prompt_block(study: int) -> str:
    style = section_style_for_study(study)
    return (
        f"**ASSIGNED section style for Estudio {study} (mandatory — do not invent another):**\n"
        f"- id: `{style.id}`\n"
        f"- name: {style.name}\n"
        f"- recipe: {style.recipe}\n"
        f"- Apply this **same** family to all three section PNGs.\n"
        f"- Write JSON field `section_style`: "
        f'`{{"id": "{style.id}", "name": "{style.name}"}}`.\n'
        f"- Do **not** reuse a neighboring study’s look; this id is already unique for this number."
    )
