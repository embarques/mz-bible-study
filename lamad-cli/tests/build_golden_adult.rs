//! Golden path: adult fixture JSON → build → validate OK.
//!
//! Requires the gitignored adult master template at
//! `lamad-cli/template/adult/master-template.pptx` (copy of the gold deck).

use std::fs;
use lamad::build::adult::study::build_study as build_adult_study;
use lamad::paths;
use lamad::validate::validate_pptx;

#[test]
fn golden_adult_build_then_validate() {
    let root = paths::project_root().expect("project root");
    std::env::set_var("MZBS_ROOT", &root);

    let template = match paths::master_template(lamad::job::Audience::Adult) {
        Ok(p) if p.is_file() => p,
        _ => {
            eprintln!("skip golden_adult_build_then_validate: adult template missing");
            return;
        }
    };

    let json = root.join("lamad-cli/tests/fixtures/adult/24.json");
    assert!(json.is_file(), "fixture missing: {}", json.display());

    let out_dir = root.join("generated/golden-adult-test");
    fs::create_dir_all(&out_dir).unwrap();
    let pptx = out_dir.join("24 - EL VALOR DE LA MODESTIA.pptx");

    let built = build_adult_study(&json, &pptx, Some(&template), false)
        .expect("adult build_study should succeed");
    assert!(built.is_file(), "pptx missing: {}", built.display());

    validate_pptx(&built).expect("validate must print OK");

    let _ = fs::remove_file(&built);
}

#[test]
fn adult_fixture_slide_counts_match_template() {
    let root = paths::project_root().expect("project root");
    let json_path = root.join("lamad-cli/tests/fixtures/adult/24.json");
    let text = fs::read_to_string(&json_path).expect("read fixture");
    let study: lamad::model::AdultStudy = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(study.lectura_antifonal.len(), 4);
    assert_eq!(study.introduccion_slides.len(), 4);
    assert_eq!(study.temas.len(), 3);
    assert_eq!(study.temas[0].a.texto_slides.len(), 4);
    assert_eq!(study.temas[0].b.texto_slides.len(), 5);
    assert_eq!(study.temas[1].a.texto_slides.len(), 4);
    assert_eq!(study.temas[1].b.texto_slides.len(), 5);
    assert_eq!(study.temas[2].a.texto_slides.len(), 5);
    assert_eq!(study.temas[2].b.texto_slides.len(), 7);
}
