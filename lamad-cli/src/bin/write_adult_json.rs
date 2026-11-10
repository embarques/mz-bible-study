fn main() -> anyhow::Result<()> {
    let study: u32 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "9".into())
        .parse()?;
    // Project root = parent of lamad-cli when run from workspace
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    std::env::set_current_dir(&root)?;
    let model = lamad::content::load_adult_sections(study)?;
    let path = root.join(format!("studies/adult/{study}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&model)?)?;
    println!("{}", path.display());
    Ok(())
}
