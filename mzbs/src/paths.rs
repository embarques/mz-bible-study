//! Project root and default asset paths.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::job::Audience;

fn looks_like_project_root(base: &Path) -> bool {
    let t = base.join("template");
    (t.join("youth").join("master-template.pptx")).is_file()
        || (t.join("master-template.pptx")).is_file()
        || (t.join("adult").join("master-template.pptx")).is_file()
}

/// Locate repo / data root (MZBS_ROOT, cwd walk, or binary-adjacent template/).
pub fn project_root() -> Result<PathBuf> {
    if let Ok(env) = std::env::var("MZBS_ROOT") {
        return Ok(PathBuf::from(&env)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(env)));
    }

    let cwd = std::env::current_dir()?;
    let mut search: Vec<PathBuf> = vec![cwd.clone()];
    search.extend(cwd.ancestors().map(|p| p.to_path_buf()));

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            search.push(dir.to_path_buf());
            search.push(dir.join("mzbs"));
            search.push(dir.join("../.."));
            search.push(dir.join("../../.."));
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    for base in search {
        let base = base.canonicalize().unwrap_or(base);
        if looks_like_project_root(&base) {
            candidates.push(base);
        }
    }

    // Prefer a root that also has AGENTS.md / python/ (monorepo) over mzbs/ alone.
    if let Some(best) = candidates.iter().find(|p| {
        p.join("AGENTS.md").is_file() || p.join("python").join("mz_bible_study").is_dir()
    }) {
        return Ok(best.clone());
    }
    if let Some(first) = candidates.into_iter().next() {
        return Ok(first);
    }

    Ok(cwd)
}

pub fn master_template(audience: Audience) -> Result<PathBuf> {
    let root = project_root()?;
    let preferred = root
        .join("template")
        .join(audience.as_str())
        .join("master-template.pptx");
    if preferred.is_file() {
        return Ok(preferred);
    }
    let crate_tmpl = root
        .join("mzbs")
        .join("template")
        .join(audience.as_str())
        .join("master-template.pptx");
    if crate_tmpl.is_file() {
        return Ok(crate_tmpl);
    }
    if audience == Audience::Youth {
        let legacy = root.join("template").join("master-template.pptx");
        if legacy.is_file() {
            return Ok(legacy);
        }
    }
    bail!(
        "No master template for audience={}. Expected {}",
        audience.as_str(),
        preferred.display()
    );
}

pub fn bible_studies_dir() -> Result<PathBuf> {
    Ok(project_root()?.join("bible-studies"))
}

pub fn studies_dir(audience: Audience) -> Result<PathBuf> {
    let path = project_root()?.join("studies").join(audience.as_str());
    std::fs::create_dir_all(path.join("media"))
        .with_context(|| format!("mkdir {}", path.display()))?;
    Ok(path)
}

pub fn generated_dir() -> Result<PathBuf> {
    let path = project_root()?.join("generated");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn scans_dir() -> Result<PathBuf> {
    let root = project_root()?;
    let a = root.join("mzbs").join("scans");
    let b = root.join("scans");
    if a.is_dir() {
        Ok(a)
    } else if b.is_dir() {
        Ok(b)
    } else {
        std::fs::create_dir_all(a.join("complete"))?;
        std::fs::create_dir_all(a.join("error"))?;
        Ok(root.join("mzbs").join("scans"))
    }
}
