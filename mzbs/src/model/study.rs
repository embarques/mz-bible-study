//! Serde structs mirroring the study JSON shape described in
//! `PREPARE_STUDY.md` / `AGENTS.md`. Optional helpers — `crate::build`
//! itself works directly off `serde_json::Value` for flexibility (many
//! fields are either-shaped: `comentario` vs `comentario_slides`,
//! `base_biblica` string-or-array, etc.).

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Punto {
    pub n: u32,
    pub titulo: String,
    pub rango: String,
    pub texto_biblico: Vec<String>,
    /// Pre-packed whole-verse groups, one `Vec<String>` per Texto slide.
    #[serde(default)]
    pub texto_slides: Option<Vec<Vec<String>>>,
    #[serde(rename = "A")]
    pub a: AbBlock,
    #[serde(rename = "B")]
    pub b: AbBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proximo {
    pub numero: Numero,
    pub titulo: String,
    #[serde(default)]
    pub base_biblica: Option<StringOrList>,
}

/// Full study JSON, as produced by `mzbs prepare` / the prepare agent and
/// consumed by `mzbs build` (see `crate::build::apply_study`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Study {
    pub numero: Numero,
    pub titulo: String,
    #[serde(default)]
    pub base_biblica: Option<StringOrList>,

    pub lectura: Lectura,
    /// Pre-packed whole-verse groups; else the builder packs with
    /// `pack_verses` (budget 280).
    #[serde(default)]
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
