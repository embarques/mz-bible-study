//! Locate and run bundled or system `pdftoppm` (poppler).

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths;

/// Resolve `pdftoppm` for prepare rasterization.
///
/// Order:
/// 1. `configured` from `config.toml` (`pdftoppm_path`) — absolute or relative
///    to project root / exe dir / cwd (tries `.exe` on Windows if needed)
/// 2. Bundled `{root|exe}/tools/pdftoppm` (`.exe` on Windows)
/// 3. `PATH`
pub fn resolve(configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(raw) = configured {
        match resolve_configured(raw) {
            Some(p) => return Ok(p),
            None => bail!(
                "pdftoppm_path is set to '{}' but that file was not found. \
                 Fix the path in config.toml (relative to the app folder) or install poppler.",
                raw.display()
            ),
        }
    }

    for cand in bundled_candidates() {
        if is_runnable(&cand) {
            return Ok(cand);
        }
    }

    if let Some(p) = which_on_path() {
        return Ok(p);
    }

    bail!(
        "pdftoppm not found. Bundle it under tools/ (see lamad-cli/tools/README.md), \
         set pdftoppm_path in config.toml, or install poppler on PATH."
    )
}

/// Prepend the binary's directory (and `../lib` / `lib`) so bundled DLLs/dylibs load.
pub fn configure_command(cmd: &mut Command, bin: &Path) {
    let Some(dir) = bin.parent() else {
        return;
    };
    let lib = dir.join("lib");
    let mut extras = vec![dir.to_path_buf()];
    if lib.is_dir() {
        extras.push(lib);
    }

    #[cfg(windows)]
    {
        prepend_env_path(cmd, "PATH", &extras);
    }
    #[cfg(target_os = "macos")]
    {
        prepend_env_path(cmd, "DYLD_LIBRARY_PATH", &extras);
        prepend_env_path(cmd, "PATH", &extras);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        prepend_env_path(cmd, "LD_LIBRARY_PATH", &extras);
        prepend_env_path(cmd, "PATH", &extras);
    }
}

fn prepend_env_path(cmd: &mut Command, key: &str, dirs: &[PathBuf]) {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let prefix = dirs
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(&sep.to_string());
    let combined = match std::env::var_os(key) {
        Some(existing) => {
            let mut s = prefix;
            s.push(sep);
            s.push_str(&existing.to_string_lossy());
            s
        }
        None => prefix,
    };
    cmd.env(key, combined);
}

fn resolve_configured(raw: &Path) -> Option<PathBuf> {
    let mut tries: Vec<PathBuf> = Vec::new();
    if raw.is_absolute() {
        tries.push(raw.to_path_buf());
    } else {
        if let Ok(root) = paths::project_root() {
            tries.push(root.join(raw));
            tries.push(root.join("lamad-cli").join(raw));
            tries.push(root.join("mzbs").join(raw)); // legacy folder
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                tries.push(dir.join(raw));
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            tries.push(cwd.join(raw));
        }
        tries.push(raw.to_path_buf());
    }

    for t in tries {
        if let Some(hit) = with_windows_exe_fallback(&t) {
            if is_runnable(&hit) {
                return Some(hit);
            }
        }
    }
    None
}

fn bundled_candidates() -> Vec<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["pdftoppm.exe", "pdftoppm"]
    } else {
        &["pdftoppm"]
    };

    let mut bases = Vec::new();
    if let Ok(root) = paths::project_root() {
        bases.push(root.join("tools"));
        bases.push(root.join("lamad-cli").join("tools"));
        bases.push(root.join("mzbs").join("tools")); // legacy folder
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            bases.push(dir.join("tools"));
        }
    }

    let mut out = Vec::new();
    for base in bases {
        for name in names {
            out.push(base.join(name));
        }
    }
    out
}

fn which_on_path() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["pdftoppm.exe", "pdftoppm"]
    } else {
        &["pdftoppm"]
    };
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            for name in names {
                let candidate = dir.join(name);
                if is_runnable(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    })
}

fn with_windows_exe_fallback(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    if cfg!(windows) {
        let lossy = path.to_string_lossy();
        if !lossy.to_ascii_lowercase().ends_with(".exe") {
            let alt = PathBuf::from(format!("{lossy}.exe"));
            if alt.is_file() {
                return Some(alt);
            }
        }
    }
    None
}

fn is_runnable(path: &Path) -> bool {
    path.is_file()
}

/// Human-readable status for `lamad doctor`.
pub fn doctor_line(configured: Option<&Path>) -> String {
    match resolve(configured) {
        Ok(p) => match smoke_test(&p) {
            Ok(()) => format!("pdftoppm: OK — {}", p.display()),
            Err(e) => format!(
                "pdftoppm: FOUND but failed to run ({}) — {e}",
                p.display()
            ),
        },
        Err(e) => format!("pdftoppm: MISSING — {e}"),
    }
}

fn smoke_test(bin: &Path) -> Result<()> {
    let mut cmd = Command::new(bin);
    configure_command(&mut cmd, bin);
    let out = cmd.arg("-v").output();
    match out {
        Ok(o) => {
            let msg = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            if msg.contains("Library not loaded")
                || (msg.contains("not found") && msg.contains("DLL"))
            {
                bail!("{}", msg.trim());
            }
            Ok(())
        }
        Err(e) => bail!("{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_list_nonempty() {
        assert!(!bundled_candidates().is_empty());
    }

    #[test]
    fn missing_configured_path_is_none() {
        let p = PathBuf::from("/tmp/definitely-missing-mzbs-pdftoppm-xyz");
        assert!(with_windows_exe_fallback(&p).is_none());
    }
}
