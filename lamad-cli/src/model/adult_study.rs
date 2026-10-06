//! Typed model for adult study JSON (`studies/adult/{N}.json`).

use serde::de;
use serde::{Deserialize, Deserializer, Serialize};

use super::study::{Lectura, Numero, Proximo, StringOrList};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextoAureo {
    pub texto: String,
    pub cita: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatosGenerales {
    pub autor: String,
    pub personajes: String,
    pub fecha: String,
    pub lugar: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definicion {
    pub termino: String,
    pub texto: String,
    /// Optional verse/ref shown in the chevron, e.g. `(Mateo 6:1)` or `12:1`.
    #[serde(default)]
    pub referencia: Option<String>,
    /// Optional left-icon motif hint for image generation (e.g. "clock", "mask").
    #[serde(default)]
    pub icono_izq: Option<String>,
    /// Optional right-icon motif hint for image generation.
    #[serde(default)]
    pub icono_der: Option<String>,
}

/// One lettered point body (title + slides + texto bíblico).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BloqueAdult {
    pub titulo: String,
    pub texto_slides: Vec<String>,
    pub texto_biblico: Lectura,
}

/// Lettered point with explicit `letra` (`"A"`, `"B"`, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PuntoAdult {
    pub letra: String,
    pub titulo: String,
    pub texto_slides: Vec<String>,
    pub texto_biblico: Lectura,
}

impl PuntoAdult {
    pub fn from_bloque(letra: &str, b: BloqueAdult) -> Self {
        Self {
            letra: letra.to_string(),
            titulo: b.titulo,
            texto_slides: b.texto_slides,
            texto_biblico: b.texto_biblico,
        }
    }

    pub fn as_bloque(&self) -> BloqueAdult {
        BloqueAdult {
            titulo: self.titulo.clone(),
            texto_slides: self.texto_slides.clone(),
            texto_biblico: self.texto_biblico.clone(),
        }
    }
}

/// One DESARROLLO tema: title + range + definiciones + variable lettered points.
#[derive(Debug, Clone, Serialize)]
pub struct TemaAdult {
    pub titulo: String,
    pub rango: String,
    pub definiciones: Vec<Definicion>,
    /// Lettered points A…N (stop at Afianzamiento in the print).
    pub puntos: Vec<PuntoAdult>,
}

impl TemaAdult {
    pub fn puntos(&self) -> &[PuntoAdult] {
        &self.puntos
    }
}

impl<'de> Deserialize<'de> for TemaAdult {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            titulo: String,
            rango: String,
            #[serde(default)]
            definiciones: Vec<Definicion>,
            #[serde(default)]
            puntos: Option<Vec<PuntoAdult>>,
            #[serde(rename = "A")]
            a: Option<BloqueAdult>,
            #[serde(rename = "B")]
            b: Option<BloqueAdult>,
            #[serde(rename = "C")]
            c: Option<BloqueAdult>,
            #[serde(rename = "D")]
            d: Option<BloqueAdult>,
            #[serde(rename = "E")]
            e: Option<BloqueAdult>,
            #[serde(rename = "F")]
            f: Option<BloqueAdult>,
        }

        let raw = Raw::deserialize(deserializer)?;
        let puntos = if let Some(p) = raw.puntos {
            if p.is_empty() {
                return Err(de::Error::custom(
                    "tema.puntos must be non-empty when present",
                ));
            }
            p
        } else {
            // Legacy: A/B (and optional C–F) object keys.
            let mut out = Vec::new();
            for (letra, block) in [
                ("A", raw.a),
                ("B", raw.b),
                ("C", raw.c),
                ("D", raw.d),
                ("E", raw.e),
                ("F", raw.f),
            ] {
                if let Some(b) = block {
                    out.push(PuntoAdult::from_bloque(letra, b));
                }
            }
            if out.is_empty() {
                return Err(de::Error::custom(
                    "tema needs `puntos` array or legacy A/B (…F) blocks",
                ));
            }
            out
        };

        Ok(TemaAdult {
            titulo: raw.titulo,
            rango: raw.rango,
            definiciones: raw.definiciones,
            puntos,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdultStudy {
    pub numero: Numero,
    pub titulo: String,
    pub base_biblica: StringOrList,
    pub lectura_antifonal: Vec<Lectura>,
    pub objetivos: Vec<String>,
    pub pensamiento_central: String,
    pub texto_aureo: TextoAureo,
    pub ensenanza: String,
    pub datos_generales: DatosGenerales,
    pub introduccion_slides: Vec<String>,
    pub temas: Vec<TemaAdult>,
    pub proximo: Proximo,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    /// One composed PNG per tema for DEFINICIÓN Y ETIMOLOGÍA card slides
    /// (paths relative to repo root unless absolute). Length must equal
    /// `temas.len()` when present.
    #[serde(default)]
    pub definicion_images: Option<Vec<String>>,
    /// Full-bleed scenic art for intro / tema / point-title slides.
    #[serde(default)]
    pub scenic_images: Option<AdultScenicImages>,
}

/// Paths for adult full-bleed image slides (16:9 cinematic PNGs, no baked text).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdultScenicImages {
    pub intro_header: String,
    /// One per tema.
    pub tema: Vec<String>,
    /// Order: tema1.A, tema1.B, …, tema2.A, … (all puntos in order).
    pub ab: Vec<String>,
}

impl AdultStudy {
    pub fn numero_string(&self) -> String {
        self.numero.to_string()
    }

    pub fn base_biblica_lines(&self) -> Vec<String> {
        self.base_biblica.as_lines()
    }

    pub fn punto_count(&self) -> usize {
        self.temas.iter().map(|t| t.puntos.len()).sum()
    }

    /// Quote body for the Texto Áureo SmartArt slot (no citation, no parens).
    pub fn texto_aureo_quote(&self) -> String {
        let mut quote = self.texto_aureo.texto.trim().to_string();
        if !quote.starts_with('"') && !quote.starts_with('“') {
            quote = format!("“{quote}");
        }
        if !quote.ends_with('"') && !quote.ends_with('”') {
            quote.push('”');
        }
        quote
    }

    /// Citation for the Texto Áureo footer slot — never parentheses.
    pub fn texto_aureo_cita_plain(&self) -> String {
        self.texto_aureo
            .cita
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim_end_matches('.')
            .trim()
            .to_string()
    }

    pub fn definiciones_text(&self, defs: &[Definicion]) -> String {
        defs.iter()
            .map(|d| {
                let term = d.termino.trim();
                let body = d.texto.trim();
                if term.is_empty() {
                    body.to_string()
                } else if term.ends_with('.') {
                    format!("{term} {body}")
                } else {
                    format!("{term}. {body}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// TEMA I, TEMA II, … (Roman).
pub fn tema_label(idx: usize) -> String {
    const ROMAN: &[&str] = &[
        "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII",
    ];
    let r = ROMAN.get(idx).copied().unwrap_or("?");
    format!("TEMA {r}")
}
