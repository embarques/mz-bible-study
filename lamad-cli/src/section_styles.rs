//! Rotating section-image design families (port of section_styles.py).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionStyle {
    pub id: &'static str,
    pub name: &'static str,
    pub recipe: &'static str,
}

pub static SECTION_STYLES: &[SectionStyle] = &[
    SectionStyle {
        id: "parchment-warm",
        name: "Warm parchment panel",
        recipe: "Aged cream/gold parchment soft fade on the LEFT ~40%; cinematic warm oil-painting biblical scene on the right; subject right/center-right; empty calm left for dark navy title.",
    },
    SectionStyle {
        id: "lavender-mist",
        name: "Cool lavender mist",
        recipe: "Cool lavender/lilac mist dissolve on the LEFT ~40% into moody dusk watercolor; subject right; empty misty left for title; purple-blue twilight palette.",
    },
    SectionStyle {
        id: "sage-paper",
        name: "Sage paper wash",
        recipe: "Soft sage-green / cream paper wash on the LEFT ~40%; bright Mediterranean daylight illustration on the right; clean luminous look; empty sage left for title.",
    },
    SectionStyle {
        id: "ink-wash",
        name: "Charcoal ink wash",
        recipe: "Soft charcoal/ink-gray horizontal bloom on the LEFT ~40% like sumi wash; desaturated epic scene right; empty gray wash left for title; no color festivity.",
    },
    SectionStyle {
        id: "dawn-gold",
        name: "Dawn gold bloom",
        recipe: "Radiant dawn-gold light bloom on the LEFT ~40% (soft sun haze, not a solid bar); scene emerges on the right in warm morning light; empty glowing left for title.",
    },
    SectionStyle {
        id: "slate-fog",
        name: "Cool slate fog",
        recipe: "Cool slate-blue fog bank on the LEFT ~40%; rainy/overcast atmospheric painting right; empty fog left for title; muted teal-slate palette.",
    },
    SectionStyle {
        id: "linen-cream",
        name: "Linen cream panel",
        recipe: "Textured linen/off-white fabric panel soft-edge on the LEFT ~40%; rich fresco-like scene right; empty linen left for title; quiet handmade feel.",
    },
    SectionStyle {
        id: "ember-vignette",
        name: "Ember night vignette",
        recipe: "Deep warm vignette opening from LEFT with soft ember/amber glow (not solid black); night or torchlit scene right; leave LEFT lighter amber wash so dark navy title still reads.",
    },
    SectionStyle {
        id: "aqua-glass",
        name: "Soft aqua glass",
        recipe: "Pale aqua/glass frost wash on the LEFT ~40%; clear hopeful daylight scene right; empty aqua left for title; airy cool palette (not purple, not parchment).",
    },
    SectionStyle {
        id: "terra-dust",
        name: "Terra dust haze",
        recipe: "Dusty terra-cotta / sandy haze dissolve LEFT ~40%; arid landscape scene right; empty dusty-cream left for title; earth pigment look.",
    },
];

pub fn section_style_for_study(study: u32) -> &'static SectionStyle {
    let n = if study < 1 { 1 } else { study };
    &SECTION_STYLES[((n - 1) as usize) % SECTION_STYLES.len()]
}

pub fn style_prompt_block(study: u32) -> String {
    let style = section_style_for_study(study);
    format!(
        "**ASSIGNED section style for Estudio {study} (mandatory — do not invent another):**\n\
         - id: `{id}`\n\
         - name: {name}\n\
         - recipe: {recipe}\n\
         - Apply this **same** family to all three section PNGs.\n\
         - Write JSON field `section_style`: `{{\"id\": \"{id}\", \"name\": \"{name}\"}}`.\n\
         - Do **not** reuse a neighboring study’s look; this id is already unique for this number.",
        id = style.id,
        name = style.name,
        recipe = style.recipe,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates() {
        assert_eq!(section_style_for_study(1).id, "parchment-warm");
        assert_eq!(section_style_for_study(11).id, "parchment-warm");
        assert_ne!(
            section_style_for_study(16).id,
            section_style_for_study(17).id
        );
    }
}
