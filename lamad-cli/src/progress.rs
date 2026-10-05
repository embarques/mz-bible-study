//! Terminal progress — rustup/cargo-style steps so long prepares don't look stuck.
//!
//! ## Verbosity (`-v` / `-vv` / …)
//!
//! | Flag | Level | Shows |
//! |------|-------|--------|
//! | (default) | 0 | `info:`, warnings, errors, final file list |
//! | `-v` | 1 | + plan / ok / phase |
//! | `-vv` | 2 | + `[n/m]` study steps, `>>> NOW Step`, ETA table |
//! | `-vvv` | 3 | + agent checklist / follow-up chatter |
//! | `-vvvv` | 4 | + build debug (`ab-title`, contrast, soft QC lines) |

use indicatif::{ProgressBar, ProgressStyle};
use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use crate::job::Audience;

static VERBOSITY: AtomicU8 = AtomicU8::new(0);

/// Set from CLI `-v` count (0 = default).
pub fn set_verbosity(level: u8) {
    VERBOSITY.store(level, Ordering::Relaxed);
}

pub fn verbosity() -> u8 {
    VERBOSITY.load(Ordering::Relaxed)
}

pub fn at_least(level: u8) -> bool {
    verbosity() >= level
}

/// Rough wall-clock guidance for volunteers (Cursor cloud prepare).
/// Not a promise — queue / model / retries can stretch times.
pub struct PrepareEta {
    /// Inclusive minutes for one full study (prepare + build + PDF).
    pub per_study_min: (u32, u32),
    pub discover: &'static str,
    pub upload_agent: &'static str,
    pub agent_work: &'static str,
    pub host_images: Option<&'static str>,
    pub download_build: &'static str,
}

pub fn prepare_eta(audience: Audience, prepare_only: bool, host_images: bool) -> PrepareEta {
    let mut eta = if host_images {
        match audience {
            Audience::Youth => PrepareEta {
                per_study_min: (3, 7),
                discover: "~15–40 sec",
                upload_agent: "~1 min",
                agent_work: "~1–3 min",
                host_images: Some("~30–90 sec (×3 parallel)"),
                download_build: "~1–2 min",
            },
            Audience::Adult => PrepareEta {
                per_study_min: (4, 9),
                discover: "~15–40 sec",
                upload_agent: "~1 min",
                agent_work: "~1–3 min",
                host_images: Some("~1–3 min (×4 parallel)"),
                download_build: "~1–2 min",
            },
        }
    } else {
        match audience {
            Audience::Youth => PrepareEta {
                per_study_min: (5, 10),
                discover: "~15–40 sec",
                upload_agent: "~1 min",
                agent_work: "~3–8 min",
                host_images: None,
                download_build: "~1–2 min",
            },
            Audience::Adult => PrepareEta {
                per_study_min: (8, 18),
                discover: "~15–40 sec",
                upload_agent: "~1 min",
                agent_work: "~5–15 min",
                host_images: None,
                download_build: "~1–2 min",
            },
        }
    };
    if prepare_only {
        eta.per_study_min.1 = eta.per_study_min.1.saturating_sub(1).max(eta.per_study_min.0);
        eta.download_build = "~30 sec";
    }
    eta
}

/// Print ETA block — shown at `-vv` or higher.
pub fn print_prepare_eta(
    audience: Audience,
    study_count: u32,
    prepare_only: bool,
    host_images: bool,
) {
    if !at_least(2) {
        return;
    }
    let eta = prepare_eta(audience, prepare_only, host_images);
    let (lo, hi) = eta.per_study_min;
    let aud = audience.as_str();

    eprintln!("  Estimated time ({aud}):");
    if study_count <= 1 {
        eprintln!("    Total for this study:  about {lo}–{hi} minutes");
    } else {
        let batch_lo = lo.saturating_mul(study_count);
        let batch_hi = hi.saturating_mul(study_count);
        eprintln!("    Per study:             about {lo}–{hi} minutes");
        eprintln!(
            "    All {study_count} studies:       about {batch_lo}–{batch_hi} minutes"
        );
    }
    eprintln!();
    eprintln!("    Step                         About");
    eprintln!("    ---------------------------  --------");
    eprintln!("    1. Find pages (OCR)          {}  ← local, parallel", eta.discover);
    eprintln!("    2. Upload + start agent      {}", eta.upload_agent);
    if host_images {
        eprintln!(
            "    3. Agent (JSON only)         {}  ← cloud writes text, not images",
            eta.agent_work
        );
        if let Some(img) = eta.host_images {
            eprintln!("    3b. Host images (parallel)  {img}  ← OpenAI on this machine");
        }
    } else {
        let agent_detail = match audience {
            Audience::Youth => "cloud AI writes JSON + 3 images",
            Audience::Adult => "cloud AI writes JSON + ~13 images (slow)",
        };
        eprintln!(
            "    3. Agent work (longest)      {}  ← {agent_detail}",
            eta.agent_work
        );
    }
    let finish_detail = if prepare_only {
        "download only (Rust; seconds)"
    } else {
        "download + PPTX build (Rust ~10s) + PDF (~1 min)"
    };
    eprintln!("    4. Finish                    {}  ← {finish_detail}", eta.download_build);
    eprintln!();
    if host_images {
        eprintln!("    Fast path: OPENAI_API_KEY set — Rust generates images in parallel.");
    } else {
        eprintln!("    Tip: set openai_api_key / OPENAI_API_KEY (even with Cursor) for the");
        eprintln!("    fast path — agent JSON only + host parallel images (~2–3× faster).");
    }
    eprintln!("    Note: Rust build is ~10s; wait is cloud/API image work, not the CLI.");
    eprintln!("    Resume skips work when JSON+images already exist. Watch >>> NOW Step N/4.");
}

/// Loud “you are here” marker — `-vv`+.
pub fn now_step(step: u32, total: u32, title: impl AsRef<str>, detail: impl AsRef<str>) {
    if !at_least(2) {
        return;
    }
    eprintln!();
    eprintln!(
        "  >>> NOW Step {step}/{total} — {}",
        title.as_ref()
    );
    let d = detail.as_ref();
    if !d.is_empty() {
        eprintln!("      {d}");
    }
}

/// High-level info line — always shown.
pub fn info(msg: impl AsRef<str>) {
    eprintln!("info: {}", msg.as_ref());
}

/// Numbered study / batch step — `-vv`+.
pub fn step(index: usize, total: usize, msg: impl AsRef<str>) {
    if !at_least(2) {
        return;
    }
    eprintln!("\n[{index}/{total}] {}", msg.as_ref());
}

/// Sub-step under the current study — `-v`+.
pub fn phase(msg: impl AsRef<str>) {
    if !at_least(1) {
        return;
    }
    eprintln!("  → {}", msg.as_ref());
}

/// Success line — `-v`+.
pub fn ok(msg: impl AsRef<str>) {
    if !at_least(1) {
        return;
    }
    eprintln!("  ✓ {}", msg.as_ref());
}

/// Warning / soft fail — always.
pub fn warn(msg: impl AsRef<str>) {
    eprintln!("  ! {}", msg.as_ref());
}

/// Extra agent chatter — `-vvv`+.
pub fn detail(msg: impl AsRef<str>) {
    if !at_least(3) {
        return;
    }
    eprintln!("      {}", msg.as_ref());
}

/// Build/QC noise (`ab-title`, contrast) — `-vvvv`+.
pub fn debug(msg: impl AsRef<str>) {
    if !at_least(4) {
        return;
    }
    eprintln!("  {}", msg.as_ref());
}

/// Format a duration like `12s`, `3m 05s`, `1h 02m`.
pub fn fmt_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Spinners always run (long waits need feedback); notes respect verbosity.
pub struct Spinner {
    bar: ProgressBar,
    started: Instant,
    finished: bool,
}

impl Spinner {
    pub fn start(msg: impl Into<Cow<'static, str>>) -> Self {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("  {spinner:.cyan} {msg} ({elapsed_precise})")
                .expect("spinner template")
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        bar.set_message(msg.into());
        bar.enable_steady_tick(Duration::from_millis(80));
        Self {
            bar,
            started: Instant::now(),
            finished: false,
        }
    }

    pub fn set_message(&self, msg: impl Into<Cow<'static, str>>) {
        self.bar.set_message(msg.into());
    }

    /// Checkpoint above the spinner — `-vvv`+.
    pub fn note(&self, msg: impl AsRef<str>) {
        if !at_least(3) {
            return;
        }
        self.bar.println(format!("  ✓ {}", msg.as_ref()));
    }

    pub fn succeed(mut self, msg: impl AsRef<str>) {
        let elapsed = fmt_elapsed(self.started.elapsed());
        self.bar.set_style(
            ProgressStyle::with_template("  {msg}").expect("finish template"),
        );
        if at_least(1) {
            self.bar
                .finish_with_message(format!("✓ {} ({elapsed})", msg.as_ref()));
        } else {
            self.bar.finish_and_clear();
        }
        self.finished = true;
    }

    pub fn fail(mut self, msg: impl AsRef<str>) {
        let elapsed = fmt_elapsed(self.started.elapsed());
        self.bar.set_style(
            ProgressStyle::with_template("  {msg}").expect("finish template"),
        );
        self.bar
            .finish_with_message(format!("✗ {} ({elapsed})", msg.as_ref()));
        self.finished = true;
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if !self.finished {
            let elapsed = fmt_elapsed(self.started.elapsed());
            if at_least(1) {
                self.bar
                    .finish_with_message(format!("✓ agent finished ({elapsed})"));
            } else {
                self.bar.finish_and_clear();
            }
        }
    }
}

pub fn bar(len: u64, msg: impl Into<Cow<'static, str>>) -> ProgressBar {
    let pb = ProgressBar::new(len);
    if at_least(1) {
        pb.set_style(
            ProgressStyle::with_template(
                "  {msg} [{bar:30.cyan/blue}] {pos}/{len} ({elapsed_precise})",
            )
            .expect("bar template")
            .progress_chars("=>-"),
        );
        pb.set_message(msg.into());
    } else {
        pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    }
    pb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_images_eta_is_faster() {
        let slow = prepare_eta(Audience::Adult, false, false);
        let fast = prepare_eta(Audience::Adult, false, true);
        assert!(fast.per_study_min.1 < slow.per_study_min.1);
        assert!(fast.host_images.is_some());
        assert!(slow.host_images.is_none());
    }

    #[test]
    fn verbosity_gates() {
        set_verbosity(0);
        assert!(!at_least(1));
        set_verbosity(2);
        assert!(at_least(2));
        assert!(!at_least(4));
        set_verbosity(0);
    }
}
