//! Unzip / zip PPTX packages + fix `ns0:` corruption (string surgery only).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn unzip_pptx(src: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        fs::remove_dir_all(dest).with_context(|| format!("rm -rf {}", dest.display()))?;
    }
    fs::create_dir_all(dest).with_context(|| format!("mkdir {}", dest.display()))?;
    let file = fs::File::open(src).with_context(|| format!("open {}", src.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("not a valid zip/pptx: {}", src.display()))?;
    zip.extract(dest)
        .with_context(|| format!("extract {} -> {}", src.display(), dest.display()))?;
    Ok(())
}

/// Zip `src_dir` into `dest` (a `.pptx`). Only skips `.DS_Store`, `._*`,
/// `~$*` — NEVER `startswith(".")`, which would drop `_rels/.rels` and
/// corrupt the package (see AGENTS.md package-integrity rules).
pub fn zip_pptx(src_dir: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        fs::remove_file(dest).with_context(|| format!("rm {}", dest.display()))?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(dest).with_context(|| format!("create {}", dest.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut entries: Vec<PathBuf> = walkdir::WalkDir::new(src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();
    entries.sort();

    for path in entries {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name == ".DS_Store" || name.starts_with("._") || name.starts_with("~$") {
            continue;
        }
        let rel = path
            .strip_prefix(src_dir)
            .with_context(|| format!("strip_prefix {}", path.display()))?;
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        zip.start_file(rel_str, options)
            .with_context(|| format!("zip start_file {}", path.display()))?;
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        std::io::Write::write_all(&mut zip, &bytes)
            .with_context(|| format!("zip write {}", path.display()))?;
    }
    zip.finish().context("finish zip")?;
    Ok(())
}

/// Rewrite `ns0:Relationships` / `ns0:Types` corruption seen on some clones.
/// String surgery only — never re-serialize these two files with a DOM writer.
/// Rewrite `ns2:` / `ns3:` … prefixes on every `.xml` part under `build`.
pub fn fix_package_ns_prefixes(build: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(build)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("xml") {
            continue;
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        if !text.contains("xmlns:ns") && !text.contains("ns0:") {
            continue;
        }
        let fixed = super::xml::fix_ns_prefixes(&text);
        if fixed != text {
            fs::write(path, fixed).with_context(|| format!("write {}", path.display()))?;
        }
    }
    Ok(())
}

pub fn fix_package_ns0(build: &Path) -> Result<()> {
    let rels_path = build.join("ppt").join("_rels").join("presentation.xml.rels");
    if rels_path.is_file() {
        let mut text = fs::read_to_string(&rels_path)
            .with_context(|| format!("read {}", rels_path.display()))?;
        if text.contains("ns0:") || text.contains("xmlns:ns0=") {
            text = text.replace("ns0:", "");
            text = text.replace(
                r#"xmlns:ns0="http://schemas.openxmlformats.org/package/2006/relationships""#,
                r#"xmlns="http://schemas.openxmlformats.org/package/2006/relationships""#,
            );
            if !text.contains(r#"xmlns="http://schemas.openxmlformats.org/package/2006/relationships""#)
            {
                text = text.replacen(
                    "<Relationships>",
                    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
                    1,
                );
            }
            fs::write(&rels_path, text)
                .with_context(|| format!("write {}", rels_path.display()))?;
        }
    }

    let ct_path = build.join("[Content_Types].xml");
    if ct_path.is_file() {
        let mut ct =
            fs::read_to_string(&ct_path).with_context(|| format!("read {}", ct_path.display()))?;
        if ct.contains("ns0:") {
            ct = ct.replace("ns0:", "");
            ct = ct.replace(
                r#"xmlns:ns0="http://schemas.openxmlformats.org/package/2006/content-types""#,
                r#"xmlns="http://schemas.openxmlformats.org/package/2006/content-types""#,
            );
            fs::write(&ct_path, ct).with_context(|| format!("write {}", ct_path.display()))?;
        }
    }
    Ok(())
}
