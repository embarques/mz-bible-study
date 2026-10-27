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
    ///
    /// Agents sometimes emit a map (`{"1":"…","2":"…"}`) instead of an array —
    /// accepted and coerced. Host prepare stamps the canonical array anyway.
    #[serde(default, deserialize_with = "deserialize_definicion_images")]
    pub definicion_images: Option<Vec<String>>,
    /// Full-bleed scenic art for intro / tema / point-title slides.
    /// Flat agent maps (`{"intro-header":…,"tema-1":…}`) are ignored → `None`
    /// (host stamps the canonical shape).
    #[serde(default, deserialize_with = "deserialize_scenic_images")]
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

/// Accept array **or** index-keyed object (`{"1":"a.png","2":"b.png"}`).
fn deserialize_definicion_images<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(coerce_definicion_images(v.unwrap_or(serde_json::Value::Null)))
}

fn coerce_definicion_images(v: serde_json::Value) -> Option<Vec<String>> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::Array(arr) => {
            let out: Vec<String> = arr
                .into_iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect();
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        }
        serde_json::Value::Object(map) => {
            let mut pairs: Vec<(i64, String)> = map
                .into_iter()
                .filter_map(|(k, val)| {
                    let path = val.as_str()?.to_string();
                    let idx = k.parse::<i64>().ok().unwrap_or(i64::MAX);
                    Some((idx, path))
                })
                .collect();
            if pairs.is_empty() {
                return None;
            }
            pairs.sort_by_key(|(i, _)| *i);
            Some(pairs.into_iter().map(|(_, p)| p).collect())
        }
        // Junk shapes → ignore; host stamps later.
        _ => None,
    }
}

/// Accept canonical `{intro_header,tema,ab}` or treat other shapes as absent.
fn deserialize_scenic_images<'de, D>(
    deserializer: D,
) -> Result<Option<AdultScenicImages>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(coerce_scenic_images(v.unwrap_or(serde_json::Value::Null)))
}

fn coerce_scenic_images(v: serde_json::Value) -> Option<AdultScenicImages> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::Object(ref map)
            if map.contains_key("intro_header")
                && map.contains_key("tema")
                && map.contains_key("ab") =>
        {
            serde_json::from_value(v).ok()
        }
        // Flat agent maps (`intro-header` / `tema-1` / `ab-1A`) — drop; host stamps.
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal_adult(extra: serde_json::Value) -> serde_json::Value {
        let mut base = json!({
            "numero": 6,
            "titulo": "T",
            "base_biblica": ["1 Samuel 24"],
            "lectura_antifonal": [{"cita": "1 S 24", "versiculos": ["1 x"]}],
            "objetivos": ["a", "b", "c"],
            "pensamiento_central": "p",
            "texto_aureo": {"texto": "q", "cita": "1 S 24:1"},
            "ensenanza": "e",
            "datos_generales": {
                "autor": "a",
                "personajes": "p",
                "fecha": "f",
                "lugar": "l"
            },
            "introduccion_slides": ["intro"],
            "temas": [{
                "titulo": "T1",
                "rango": "1 S 24",
                "definiciones": [],
                "A": {
                    "titulo": "A",
                    "texto_slides": ["body"],
                    "texto_biblico": {"cita": "1 S 24:1", "versiculos": ["1 x"]}
                }
            }],
            "proximo": {"numero": 7, "titulo": "N", "base_biblica": ["x"]}
        });
        if let (Some(obj), Some(ext)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in ext {
                obj.insert(k.clone(), v.clone());
            }
        }
        base
    }

    #[test]
    fn definicion_images_accepts_index_map() {
        let v = minimal_adult(json!({
            "definicion_images": {"2": "b.png", "1": "a.png"}
        }));
        let s: AdultStudy = serde_json::from_value(v).expect("parse");
        assert_eq!(
            s.definicion_images.as_deref(),
            Some(["a.png".to_string(), "b.png".to_string()].as_slice())
        );
    }

    #[test]
    fn scenic_images_flat_map_becomes_none() {
        let v = minimal_adult(json!({
            "scenic_images": {
                "intro-header": "x.png",
                "tema-1": "t.png",
                "ab-1A": "a.png"
            }
        }));
        let s: AdultStudy = serde_json::from_value(v).expect("parse");
        assert!(s.scenic_images.is_none());
    }

    #[test]
    fn scenic_images_canonical_ok() {
        let v = minimal_adult(json!({
            "scenic_images": {
                "intro_header": "i.png",
                "tema": ["t1.png"],
                "ab": ["a.png"]
            }
        }));
        let s: AdultStudy = serde_json::from_value(v).expect("parse");
        let sc = s.scenic_images.expect("scenic");
        assert_eq!(sc.intro_header, "i.png");
        assert_eq!(sc.tema, vec!["t1.png"]);
        assert_eq!(sc.ab, vec!["a.png"]);
    }
}
