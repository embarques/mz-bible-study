//! Typed model for adult study JSON (`studies/adult/{N}.json`).

use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BloqueAdult {
    pub titulo: String,
    pub texto_slides: Vec<String>,
    pub texto_biblico: Lectura,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemaAdult {
    pub titulo: String,
    pub rango: String,
    pub definiciones: Vec<Definicion>,
    #[serde(rename = "A")]
    pub a: BloqueAdult,
    #[serde(rename = "B")]
    pub b: BloqueAdult,
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
}

impl AdultStudy {
    pub fn numero_string(&self) -> String {
        self.numero.to_string()
    }

    pub fn base_biblica_lines(&self) -> Vec<String> {
        self.base_biblica.as_lines()
    }

    pub fn texto_aureo_diagram_text(&self) -> String {
        let mut quote = self.texto_aureo.texto.trim().to_string();
        if !quote.starts_with('"') && !quote.starts_with('“') {
            quote = format!("“{quote}");
        }
        if !quote.ends_with('"') && !quote.ends_with('”') {
            quote.push('”');
        }
        format!("{quote} ({})", self.texto_aureo.cita)
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
