fn main() -> anyhow::Result<()> {
    let study: u32 = std::env::args().nth(1).unwrap_or_else(|| "9".into()).parse()?;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    std::env::set_current_dir(&root)?;
    let model = lamad::content::load_adult_sections(study)?;
    let img_dir = root.join(format!("studies/adult/{study}/images"));
    for (i, tema) in model.temas.iter().enumerate() {
        if tema.definiciones.is_empty() {
            continue;
        }
        let n = i + 1;
        let png = img_dir.join(format!("definicion-{n}.png"));
        let rows: Vec<(&str, &str)> = tema
            .definiciones
            .iter()
            .map(|d| (d.termino.as_str(), d.texto.as_str()))
            .collect();
        let fp = lamad::build::adult::terms_fingerprint(rows.iter().copied());
        lamad::build::adult::write_terms_sidecar(&png, &fp)?;
        println!("stamped definicion-{n}.terms = {fp}");
        if !lamad::build::adult::looks_like_locked_definicion_card(&png) {
            anyhow::bail!("LAYOUT FAIL definicion-{n}");
        }
        lamad::build::adult::assert_study_definicion_card(&png, rows.iter().copied())?;
        println!("QC PASS definicion-{n}");
    }
    Ok(())
}
