//! Terminal progress — rustup/cargo-style steps so long prepares don't look stuck.
//!
//! Usage:
//! ```ignore
//! progress::info("preparing estudios 23–26 (4 studies)");
//! let _s = progress::spinner("Waiting for Cursor agent…");
//! // … work …
//! drop(_s); // prints ✓ + elapsed
//! progress::ok("Estudio 23 ready");
//! ```

use indicatif::{ProgressBar, ProgressStyle};
use std::borrow::Cow;
use std::time::{Duration, Instant};

/// High-level info line (like rustup's `info:`).
pub fn info(msg: impl AsRef<str>) {
    eprintln!("info: {}", msg.as_ref());
}

/// Numbered study / batch step: `[2/4] Estudio 24`.
pub fn step(index: usize, total: usize, msg: impl AsRef<str>) {
    eprintln!("\n[{index}/{total}] {}", msg.as_ref());
}

/// Sub-step under the current study (indent, no spinner).
pub fn phase(msg: impl AsRef<str>) {
    eprintln!("  → {}", msg.as_ref());
}

/// Success line.
pub fn ok(msg: impl AsRef<str>) {
    eprintln!("  ✓ {}", msg.as_ref());
}

/// Warning / soft fail (still continuing).
pub fn warn(msg: impl AsRef<str>) {
    eprintln!("  ! {}", msg.as_ref());
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

/// Indeterminate spinner that finishes with ✓ + elapsed on drop (or [`Spinner::fail`]).
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

    pub fn succeed(mut self, msg: impl AsRef<str>) {
        let elapsed = fmt_elapsed(self.started.elapsed());
        self.bar.set_style(
            ProgressStyle::with_template("  {msg}")
                .expect("finish template"),
        );
        self.bar
            .finish_with_message(format!("✓ {} ({elapsed})", msg.as_ref()));
        self.finished = true;
    }

    pub fn fail(mut self, msg: impl AsRef<str>) {
        let elapsed = fmt_elapsed(self.started.elapsed());
        self.bar.set_style(
            ProgressStyle::with_template("  {msg}")
                .expect("finish template"),
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
            self.bar.set_style(
                ProgressStyle::with_template("  {msg}")
                    .expect("finish template"),
            );
            self.bar
                .finish_with_message(format!("✓ done ({elapsed})"));
            self.finished = true;
        }
    }
}

/// Determinate bar for known totals (pages, artifacts, images).
pub fn bar(len: u64, msg: impl Into<Cow<'static, str>>) -> ProgressBar {
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::with_template(
            "  {msg} [{bar:30.cyan/blue}] {pos}/{len} ({elapsed_precise})",
        )
        .expect("bar template")
        .progress_chars("=>-"),
    );
    pb.set_message(msg.into());
    pb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_elapsed_buckets() {
        assert_eq!(fmt_elapsed(Duration::from_secs(9)), "9s");
        assert_eq!(fmt_elapsed(Duration::from_secs(65)), "1m 05s");
        assert_eq!(fmt_elapsed(Duration::from_secs(3723)), "1h 02m");
    }
}
