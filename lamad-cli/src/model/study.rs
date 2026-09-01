//! Serde structs mirroring the study JSON shape described in
//! `PREPARE_STUDY.md` / `AGENTS.md`. `crate::build::apply_study` deserializes
//! study JSON into [`Study`] and drives slide-filling off these typed
//! fields — edit *this* file when the JSON shape changes, and
//! `crate::build::study` when the slide order changes.
//!
//! The `*_packs()` methods below are the single place that decides "pre-packed
//! `*_slides` field, or pack it now with `crate::pack`" for each either-shaped
//! field (`comentario` vs `comentario_slides`, etc.) — callers should never
//! need to duplicate that fallback logic.

use serde::{Deserialize, Deserializer, Serialize};

use crate::pack;

/// Default budget (chars) for whole-verse packs (Lectura / Texto Bíblico).
pub const VERSE_BUDGET: usize = 280;
/// Default budget (chars) for sentence packs (comentario/intro/A-B/conclusión).
pub const SENTENCE_BUDGET: usize = 380;

/// A field that may be authored as a single `;`-joined string or as an
/// already-split array (e.g. `base_biblica`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrList {
    One(String),
    Many(Vec<String>),
}

impl StringOrList {
    /// Split `;`-joined text into trimmed, non-empty lines; pass arrays
    /// through untouched.
    pub fn into_lines(self) -> Vec<String> {
        match self {
            StringOrList::One(s) => s
                .split(';')
                .map(|b| b.trim().to_string())
                .filter(|b| !b.is_empty())
                .collect(),
            StringOrList::Many(v) => v,
        }
    }

    pub fn as_lines(&self) -> Vec<String> {
        self.clone().into_lines()
    }
}

/// `numero` may be authored as a bare integer (`17`) or a string (`"17B"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Numero {
    Int(i64),
    Text(String),
}

impl std::fmt::Display for Numero {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Numero::Int(n) => write!(f, "{n}"),
            Numero::Text(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lectura {
    pub cita: String,
    pub versiculos: Vec<String>,
}

/// Agent JSON may use a flat verse list or `{ cita, versiculos }` (same as Lectura).
fn parse_verse_list_value(v: &serde_json::Value) -> Result<Vec<String>, String> {
    match v {
        serde_json::Value::Array(arr) => arr
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "expected string verses".to_string())
            })
            .collect(),
        serde_json::Value::Object(_) => {
            let vers = v
                .get("versiculos")
                .and_then(|v| v.as_array())
                .ok_or("verse block needs versiculos array")?;
            Ok(string_array(vers))
        }
        _ => Err("expected verse array or {cita, versiculos} object".to_string()),
    }
}

fn string_array(arr: &[serde_json::Value]) -> Vec<String> {
    arr.iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// `lectura_slides` / `texto_slides` may be string[], string[][], or
/// `{cita, versiculos}[]` (agent-prepacked slides).
fn parse_verse_slides_value(v: &serde_json::Value) -> Result<Vec<Vec<String>>, String> {
    let arr = v
        .as_array()
        .ok_or("expected slide array")?;
    if arr.is_empty() {
        return Ok(vec![]);
    }
    match &arr[0] {
        serde_json::Value::String(_) => Ok(arr
            .iter()
            .filter_map(|item| {
                item.as_str().map(|s| {
                    s.split('\n')
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .map(str::to_string)
                        .collect()
                })
            })
            .collect()),
        serde_json::Value::Array(_) => serde_json::from_value(v.clone()).map_err(|e| e.to_string()),
        serde_json::Value::Object(_) => arr
            .iter()
            .map(|slide| {
                slide
                    .get("versiculos")
                    .and_then(|v| v.as_array())
                    .map(|vers| string_array(vers))
                    .ok_or_else(|| "slide object missing versiculos".to_string())
            })
            .collect(),
        _ => Err("unexpected slide element type".to_string()),
    }
}

fn deserialize_texto_biblico<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    parse_verse_list_value(&v).map_err(serde::de::Error::custom)
}

fn deserialize_verse_slides_option<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<Vec<String>>>, D::Error>
where
    D: Deserializer<'de>,
{
    let v: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match v {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => parse_verse_slides_value(&v)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParaMemorizar {
    pub texto: String,
    /// Often lives only in the diagram footer — don't duplicate it into
    /// `texto` when this is set (see AGENTS.md).
    #[serde(default)]
    pub cita: Option<String>,
}

/// One `A` or `B` sub-point body: either pre-packed `slides`, or `cuerpo`
/// for the builder to pack (`pack_sentences`, budget 380).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbBlock {
    pub titulo: String,
    #[serde(default)]
    pub cuerpo: Option<String>,
    #[serde(default)]
    pub slides: Option<Vec<String>>,
}

impl AbBlock {
    /// One slide of body text per entry: pre-packed `slides` if present,
    /// else `cuerpo` packed with `pack::pack_sentences` (budget 380).
    pub fn ab_packs(&self) -> Vec<String> {
        if let Some(slides) = &self.slides {
            return slides.clone();
        }
        pack::pack_sentences(self.cuerpo.as_deref().unwrap_or_default(), SENTENCE_BUDGET)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Punto {
    pub n: u32,
    pub titulo: String,
    pub rango: String,
    #[serde(deserialize_with = "deserialize_texto_biblico")]
    pub texto_biblico: Vec<String>,
    /// Pre-packed whole-verse groups, one `Vec<String>` per Texto slide.
    #[serde(
        default,
        alias = "texto_biblico_slides",
        deserialize_with = "deserialize_verse_slides_option"
    )]
    pub texto_slides: Option<Vec<Vec<String>>>,
    #[serde(rename = "A")]
    pub a: AbBlock,
    #[serde(rename = "B")]
    pub b: AbBlock,
}

impl Punto {
    /// One whole-verse group per Texto Bíblico slide: pre-packed
    /// `texto_slides` if present, else `texto_biblico` packed with
    /// `pack::pack_verses` (budget 280).
    pub fn punto_texto_packs(&self) -> Vec<Vec<String>> {
        if let Some(slides) = &self.texto_slides {
            return slides.clone();
        }
        pack::pack_verses(&self.texto_biblico, VERSE_BUDGET)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proximo {
    pub numero: Numero,
    pub titulo: String,
    #[serde(default)]
    pub base_biblica: Option<StringOrList>,
}

/// Full study JSON, as produced by `lamad prepare` / the prepare agent and
/// consumed by `lamad build` (see `crate::build::apply_study`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Study {
    pub numero: Numero,
    pub titulo: String,
    #[serde(default)]
    pub base_biblica: Option<StringOrList>,

    pub lectura: Lectura,
    /// Pre-packed whole-verse groups; else the builder packs with
    /// `pack_verses` (budget 280).
    #[serde(default, deserialize_with = "deserialize_verse_slides_option")]
    pub lectura_slides: Option<Vec<Vec<String>>>,

    /// Exactly 3 purpose strings.
    pub propositos: Vec<String>,
    pub idea_principal: String,
    pub para_memorizar: ParaMemorizar,

    #[serde(default)]
    pub comentario: Option<String>,
    #[serde(default)]
    pub comentario_slides: Option<Vec<String>>,

    #[serde(default)]
    pub introduccion: Option<String>,
    #[serde(default)]
    pub introduccion_slides: Option<Vec<String>>,

    /// Exactly 3 development points.
    pub puntos: Vec<Punto>,

    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub conclusion_slides: Option<Vec<String>>,

    /// Omit / null on the last study in a batch (no Próximo slide).
    #[serde(default)]
    pub proximo: Option<Proximo>,

    /// Exactly 3 section-image paths, relative to the repo root unless
    /// absolute.
    pub section_images: Vec<String>,

    #[serde(default)]
    pub audience: Option<String>,
    /// Optional explicit template override (relative to repo root unless
    /// absolute) — takes precedence over the audience's master template.
    #[serde(default)]
    pub template: Option<String>,
}

impl Study {
    pub fn base_biblica_lines(&self) -> Vec<String> {
        self.base_biblica
            .as_ref()
            .map(|b| b.as_lines())
            .unwrap_or_default()
    }

    pub fn numero_string(&self) -> String {
        self.numero.to_string()
    }

    /// One whole-verse group per Lectura slide: pre-packed `lectura_slides`
    /// if present, else `lectura.versiculos` packed with
    /// `pack::pack_verses` (budget 280).
    pub fn lectura_packs(&self) -> Vec<Vec<String>> {
        if let Some(slides) = &self.lectura_slides {
            return slides.clone();
        }
        pack::pack_verses(&self.lectura.versiculos, VERSE_BUDGET)
    }

    /// One slide of body text per entry: pre-packed `comentario_slides` if
    /// present, else `comentario` packed with `pack::pack_sentences`
    /// (budget 380).
    pub fn comentario_packs(&self) -> Vec<String> {
        packed_or_sentences(&self.comentario, &self.comentario_slides)
    }

    /// Same fallback as [`Study::comentario_packs`], for `introduccion` /
    /// `introduccion_slides`.
    pub fn intro_packs(&self) -> Vec<String> {
        packed_or_sentences(&self.introduccion, &self.introduccion_slides)
    }

    /// Same fallback as [`Study::comentario_packs`], for `conclusion` /
    /// `conclusion_slides`.
    pub fn conclusion_packs(&self) -> Vec<String> {
        packed_or_sentences(&self.conclusion, &self.conclusion_slides)
    }
}

/// Shared `{field}` / `{field}_slides` fallback: pre-packed slides win;
/// otherwise pack the raw text now with `pack::pack_sentences`.
fn packed_or_sentences(text: &Option<String>, slides: &Option<Vec<String>>) -> Vec<String> {
    if let Some(slides) = slides {
        return slides.clone();
    }
    pack::pack_sentences(text.as_deref().unwrap_or_default(), SENTENCE_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_or_list_splits_semicolons() {
        let v = StringOrList::One("Juan 3:16; Romanos 5:8".to_string());
        assert_eq!(
            v.into_lines(),
            vec!["Juan 3:16".to_string(), "Romanos 5:8".to_string()]
        );
    }

    #[test]
    fn numero_display_matches_authored_form() {
        assert_eq!(Numero::Int(17).to_string(), "17");
        assert_eq!(Numero::Text("17B".to_string()).to_string(), "17B");
    }

    #[test]
    fn deserializes_agent_verse_block_shapes() {
        let json = serde_json::json!({
            "numero": 24,
            "titulo": "SUPERA EL RECHAZO",
            "lectura": {"cita": "Génesis 45:1-7", "versiculos": ["1 verse"]},
            "lectura_slides": [
                {"cita": "Génesis 45:1-7", "versiculos": ["1 verse"]},
                {"cita": null, "versiculos": ["2 verse", "3 verse"]}
            ],
            "propositos": ["a", "b", "c"],
            "idea_principal": "idea",
            "para_memorizar": {"texto": "texto"},
            "comentario_slides": ["c"],
            "introduccion_slides": ["i"],
            "puntos": [
                {
                    "n": 1, "titulo": "T1", "rango": "R1",
                    "texto_biblico": {"cita": "Gén 1:1", "versiculos": ["1 x", "2 y"]},
                    "texto_biblico_slides": [
                        {"cita": "Gén 1:1", "versiculos": ["1 x"]},
                        {"cita": null, "versiculos": ["2 y"]}
                    ],
                    "A": {"titulo": "A1", "cuerpo": "a body"},
                    "B": {"titulo": "B1", "cuerpo": "b body"}
                },
                {
                    "n": 2, "titulo": "T2", "rango": "R2", "texto_biblico": ["2 x"],
                    "A": {"titulo": "A2", "cuerpo": "a body"},
                    "B": {"titulo": "B2", "cuerpo": "b body"}
                },
                {
                    "n": 3, "titulo": "T3", "rango": "R3", "texto_biblico": ["3 x"],
                    "A": {"titulo": "A3", "cuerpo": "a body"},
                    "B": {"titulo": "B3", "cuerpo": "b body"}
                }
            ],
            "conclusion_slides": ["end"],
            "section_images": ["a.png", "b.png", "c.png"]
        });
        let study: Study = serde_json::from_value(json).expect("deserialize");
        assert_eq!(study.lectura_packs().len(), 2);
        assert_eq!(study.puntos[0].texto_biblico.len(), 2);
        assert_eq!(study.puntos[0].punto_texto_packs().len(), 2);
    }

    #[test]
    fn deserializes_minimal_study() {
        let json = serde_json::json!({
            "numero": 17,
            "titulo": "UN ABRAZO QUE DA VIDA",
            "base_biblica": "2 Reyes 4:18-37",
            "lectura": {"cita": "2 Reyes 4:18-21", "versiculos": ["18 ...", "19 ..."]},
            "propositos": ["a", "b", "c"],
            "idea_principal": "idea",
            "para_memorizar": {"texto": "texto"},
            "comentario": "comentario",
            "introduccion": "intro",
            "puntos": [
                {
                    "n": 1, "titulo": "T1", "rango": "R1", "texto_biblico": ["1 x"],
                    "A": {"titulo": "A1", "cuerpo": "a body"},
                    "B": {"titulo": "B1", "cuerpo": "b body"}
                },
                {
                    "n": 2, "titulo": "T2", "rango": "R2", "texto_biblico": ["2 x"],
                    "A": {"titulo": "A2", "cuerpo": "a body"},
                    "B": {"titulo": "B2", "cuerpo": "b body"}
                },
                {
                    "n": 3, "titulo": "T3", "rango": "R3", "texto_biblico": ["3 x"],
                    "A": {"titulo": "A3", "cuerpo": "a body"},
                    "B": {"titulo": "B3", "cuerpo": "b body"}
                }
            ],
            "conclusion": "concl",
            "section_images": ["a.png", "b.png", "c.png"]
        });
        let study: Study = serde_json::from_value(json).expect("deserialize");
        assert_eq!(study.numero_string(), "17");
        assert_eq!(study.base_biblica_lines(), vec!["2 Reyes 4:18-37".to_string()]);
        assert_eq!(study.puntos.len(), 3);
    }
}
