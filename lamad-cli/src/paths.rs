//! Project root and default asset paths.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::Config;
use crate::job::Audience;

/// Crate directory name in the monorepo (not the binary name — that is `lamad`).
const CRATE_DIR: &str = "lamad-cli";
const CRATE_DIR_LEGACY: &str = "mzbs";

/// Optional path overrides from `config.toml` (relative → project root; absolute as-is).
#[derive(Debug, Clone, Default)]
pub struct PathOverrides {
    pub scans_dir: Option<PathBuf>,
    pub studies_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub generated_dir: Option<PathBuf>,
}

static PATH_OVERRIDES: OnceLock<PathOverrides> = OnceLock::new();

/// Call once after [`Config::load`] so path helpers honor config.
pub fn init_from_config(cfg: &Config) {
    let _ = PATH_OVERRIDES.set(PathOverrides {
        scans_dir: cfg.scans_dir.clone(),
        studies_dir: cfg.studies_dir.clone(),
        output_dir: cfg.output_dir.clone(),
        generated_dir: cfg.generated_dir.clone(),
    });
}

fn overrides() -> PathOverrides {
    PATH_OVERRIDES.get().cloned().unwrap_or_default()
}

/// Resolve a configured path: absolute stays absolute; relative joins project root.
pub fn resolve_configured(configured: Option<&PathBuf>, default_rel: &str) -> Result<PathBuf> {
    let root = project_root()?;
    match configured {
        Some(p) if p.is_absolute() => Ok(p.clone()),
        Some(p) => Ok(root.join(p)),
        None => Ok(root.join(default_rel)),
    }
}

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
            search.push(dir.join(CRATE_DIR));
            search.push(dir.join(CRATE_DIR_LEGACY));
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

    // Prefer a root that also has AGENTS.md / python/ (monorepo) over crate dir alone.
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
    for crate_name in [CRATE_DIR, CRATE_DIR_LEGACY] {
        let crate_tmpl = root
            .join(crate_name)
            .join("template")
            .join(audience.as_str())
            .join("master-template.pptx");
        if crate_tmpl.is_file() {
            return Ok(crate_tmpl);
        }
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

/// Finished decks root (`bible-studies/` by default).
///
/// Layout under this root:
/// `{audience}/{N}/powerpoint/{N} - {TITLE}.pptx`
/// `{audience}/{N}/pdf/{N} - {TITLE}.pdf`
pub fn bible_studies_dir() -> Result<PathBuf> {
    let o = overrides();
    let path = resolve_configured(o.output_dir.as_ref(), "bible-studies")?;
    std::fs::create_dir_all(&path).with_context(|| format!("mkdir {}", path.display()))?;
    Ok(path)
}

/// `output/{audience}/{N}/`
pub fn study_output_dir(audience: Audience, study: u32) -> Result<PathBuf> {
    let path = bible_studies_dir()?
        .join(audience.as_str())
        .join(study.to_string());
    std::fs::create_dir_all(path.join("powerpoint"))?;
    std::fs::create_dir_all(path.join("pdf"))?;
    Ok(path)
}

/// `output/{audience}/{N}/powerpoint/{N} - {TITLE}.pptx`
pub fn study_pptx_path(audience: Audience, study: u32, title: &str) -> Result<PathBuf> {
    let dir = study_output_dir(audience, study)?.join("powerpoint");
    Ok(dir.join(format!("{study} - {title}.pptx")))
}

/// `output/{audience}/{N}/pdf/{N} - {TITLE}.pdf`
pub fn study_pdf_path(audience: Audience, study: u32, title: &str) -> Result<PathBuf> {
    let dir = study_output_dir(audience, study)?.join("pdf");
    Ok(dir.join(format!("{study} - {title}.pdf")))
}

/// Root of all audience study content (`studies/` by default).
pub fn studies_root() -> Result<PathBuf> {
    let o = overrides();
    let path = resolve_configured(o.studies_dir.as_ref(), "studies")?;
    std::fs::create_dir_all(&path).with_context(|| format!("mkdir {}", path.display()))?;
    Ok(path)
}

pub fn studies_dir(audience: Audience) -> Result<PathBuf> {
    let path = studies_root()?.join(audience.as_str());
    std::fs::create_dir_all(path.join("media"))
        .with_context(|| format!("mkdir {}", path.display()))?;
    Ok(path)
}

/// Adult study folder: `studies/adult/{N}/` (section tree + images/).
pub fn adult_study_dir(study: u32) -> Result<PathBuf> {
    let path = studies_dir(Audience::Adult)?.join(study.to_string());
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn generated_dir() -> Result<PathBuf> {
    let o = overrides();
    let path = resolve_configured(o.generated_dir.as_ref(), "generated")?;
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn scans_dir() -> Result<PathBuf> {
    let o = overrides();
    if o.scans_dir.is_some() {
        let path = resolve_configured(o.scans_dir.as_ref(), "scans")?;
        ensure_scan_trays(&path)?;
        return Ok(path);
    }

    let root = project_root()?;
    // Prefer repo-root scans/ (data beside lamad-cli/). Fall back to crate tray.
    let root_scans = root.join("scans");
    let crate_scans = root.join(CRATE_DIR).join("scans");
    let legacy = root.join(CRATE_DIR_LEGACY).join("scans");

    let path = if root_scans.is_dir() {
        root_scans
    } else if crate_scans.is_dir() {
        crate_scans
    } else if legacy.is_dir() {
        legacy
    } else {
        root_scans
    };
    ensure_scan_trays(&path)?;
    Ok(path)
}

fn ensure_scan_trays(scans: &Path) -> Result<()> {
    for tray in ["pending", "complete", "error"] {
        std::fs::create_dir_all(scans.join(tray))
            .with_context(|| format!("mkdir {}", scans.join(tray).display()))?;
    }
    Ok(())
}
