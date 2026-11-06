//! One-shot: JSON → section folder tree.
//!
//! ```bash
//! cargo run --example expand_adult_sections -- 7 path/to/7.json
//! ```

use anyhow::Result;
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let study: u32 = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: expand_adult_sections <N> <json>"))?
        .parse()?;
    let json = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("usage: expand_adult_sections <N> <json>"))?,
    );
    // Prefer repo root when running from lamad-cli/
    if env::var_os("MZBS_ROOT").is_none() {
        let cwd = env::current_dir()?;
        if cwd.ends_with("lamad-cli") {
            env::set_var("MZBS_ROOT", cwd.parent().unwrap());
        }
    }
    let dir = lamad::content::expand_json_to_sections(&json, study)?;
    println!("wrote {}", dir.display());
    Ok(())
}
