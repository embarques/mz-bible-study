//! Golden path: minimal Study JSON + 3 tiny PNGs → build → validate OK.
//!
//! This is the main regression net for juniors touching the builder.
//! It uses the real youth master template from the repo (or `lamad-cli/template/`).

use std::fs;
use std::path::{Path, PathBuf};

use lamad::build::build_study;
use lamad::job::Audience;
use lamad::paths;
use lamad::validate::validate_pptx;

/// 1×1 PNG (valid file; section slides only need *a* PNG to copy into media).
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn write_minimal_fixture(root: &Path) -> PathBuf {
    let media = root.join("studies/youth/media");
    fs::create_dir_all(&media).expect("mkdir media");
    let imgs = [
        media.join("9001-section1.png"),
        media.join("9001-section2.png"),
        media.join("9001-section3.png"),
    ];
    for p in &imgs {
        fs::write(p, TINY_PNG).expect("write png");
    }

    let json_path = root.join("studies/youth/9001.json");
    // Short Spanish-ish content so packing stays on few slides.
    let json = serde_json::json!({
        "numero": 9001,
        "titulo": "PRUEBA DORADA",
        "base_biblica": "Juan 3:16",
        "audience": "youth",
        "lectura": {
            "cita": "Juan 3:16",
            "versiculos": [
                "16 Porque de tal manera amó Dios al mundo, que ha dado a su Hijo unigénito."
            ]
        },
        "propositos": [
            "Conocer el amor de Dios.",
            "Creer en su Hijo.",
            "Vivir con esperanza."
        ],
        "idea_principal": "Dios amó al mundo y envió a su Hijo.",
        "para_memorizar": {
            "texto": "Porque de tal manera amó Dios al mundo.",
            "cita": "Juan 3:16"
        },
        "comentario": "Este pasaje resume el evangelio con claridad. El amor de Dios es la fuente.",
        "introduccion": "Hoy veremos el corazón del mensaje cristiano en un solo versículo conocido.",
        "puntos": [
            {
                "n": 1,
                "titulo": "El amor de Dios",
                "rango": "Juan 3:16a",
                "texto_biblico": ["16 Porque de tal manera amó Dios al mundo,"],
                "A": {"titulo": "Amor que inicia", "cuerpo": "Dios toma la iniciativa. Su amor no depende de nuestro mérito."},
                "B": {"titulo": "Amor que da", "cuerpo": "El amor se demuestra al dar. Dios dio a su Hijo por nosotros."}
            },
            {
                "n": 2,
                "titulo": "El Hijo dado",
                "rango": "Juan 3:16b",
                "texto_biblico": ["16 que ha dado a su Hijo unigénito,"],
                "A": {"titulo": "Un don único", "cuerpo": "El Hijo unigénito es el don central de la salvación."},
                "B": {"titulo": "Un don costoso", "cuerpo": "La cruz muestra el precio del regalo de Dios."}
            },
            {
                "n": 3,
                "titulo": "La fe que recibe",
                "rango": "Juan 3:16c",
                "texto_biblico": ["16 para que todo aquel que en él cree, no se pierda, mas tenga vida eterna."],
                "A": {"titulo": "Creer", "cuerpo": "La fe recibe lo que Dios ofrece en Cristo."},
                "B": {"titulo": "Vida eterna", "cuerpo": "El resultado de creer es vida eterna, no condenación."}
            }
        ],
        "conclusion": "Recibamos este amor con fe sencilla y vivamos agradecidos.",
        "proximo": null,
        "section_images": [
            "studies/youth/media/9001-section1.png",
            "studies/youth/media/9001-section2.png",
            "studies/youth/media/9001-section3.png"
        ],
        "section_style": {"id": "parchment-warm", "name": "Warm parchment panel"}
    });
    fs::write(&json_path, serde_json::to_string_pretty(&json).unwrap()).expect("write json");
    json_path
}

#[test]
fn golden_build_then_validate() {
    let root = paths::project_root().expect("project root");
    // Ensure MZBS_ROOT points at the repo so template + studies resolve together.
    std::env::set_var("MZBS_ROOT", &root);

    let json = write_minimal_fixture(&root);
    let out_dir = root.join("generated/golden-test");
    fs::create_dir_all(&out_dir).unwrap();
    let pptx = out_dir.join("9001 - PRUEBA DORADA.pptx");

    let template = paths::master_template(Audience::Youth).expect("youth template");
    let built = build_study(&json, &pptx, Some(&template), Audience::Youth, false)
        .expect("build_study should succeed");
    assert!(built.is_file(), "pptx missing: {}", built.display());

    validate_pptx(&built).expect("validate must print OK path");

    // Cleanup generated deck + fixture JSON/media so we don't litter the tree.
    let _ = fs::remove_file(&built);
    let _ = fs::remove_file(&json);
    for i in 1..=3 {
        let _ = fs::remove_file(root.join(format!("studies/youth/media/9001-section{i}.png")));
    }
}
