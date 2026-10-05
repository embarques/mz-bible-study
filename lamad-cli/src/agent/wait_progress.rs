//! Human-readable prepare steps while a Cursor cloud agent is RUNNING.

use crate::agent::client::Artifact;
use crate::job::Audience;
use crate::progress;

/// Same basenames as [`crate::agent::prepare::adult_image_basenames`] (keep in sync).
fn adult_image_basenames() -> &'static [&'static str] {
    &[
        "intro-header.png",
        "tema-1.png",
        "tema-2.png",
        "tema-3.png",
        "ab-1A.png",
        "ab-1B.png",
        "ab-2A.png",
        "ab-2B.png",
        "ab-3A.png",
        "ab-3B.png",
        "definicion-1.png",
        "definicion-2.png",
        "definicion-3.png",
    ]
}

/// Tracks JSON + image artifacts and drives spinner / checklist updates.
pub struct CloudAgentProgress {
    study: u32,
    audience: Audience,
    /// When true, agent only writes JSON; host generates images afterward.
    json_only: bool,
    images_total: usize,
    json_done: bool,
    images_done: usize,
    last_announced_images: usize,
    plan_printed: bool,
}

impl CloudAgentProgress {
    pub fn new(study: u32, audience: Audience, json_only: bool) -> Self {
        let images_total = if json_only {
            0
        } else {
            match audience {
                Audience::Youth => 3,
                Audience::Adult => adult_image_basenames().len(),
            }
        };
        Self {
            study,
            audience,
            json_only,
            images_total,
            json_done: false,
            images_done: 0,
            last_announced_images: 0,
            plan_printed: false,
        }
    }

    /// Print the expected steps once (under the current study).
    pub fn print_plan(&mut self) {
        if self.plan_printed {
            return;
        }
        self.plan_printed = true;
        if !progress::at_least(3) {
            return;
        }
        let aud = self.audience.as_str();
        let eta = progress::prepare_eta(self.audience, false, self.json_only);
        let (lo, hi) = eta.per_study_min;
        progress::phase(format!(
            "Cursor agent — estudio {} ({aud}). This step usually takes {}.",
            self.study, eta.agent_work
        ));
        eprintln!("      What the agent is doing now:");
        eprintln!("        1. Read the PDF page images (already uploaded)");
        eprintln!(
            "        2. Write artifacts/{}.json",
            self.study
        );
        if self.json_only {
            eprintln!("        3. Finish the cloud run (no images — host does those next)");
            eprintln!(
                "      After that: host parallel images + build · whole study about {lo}–{hi} min."
            );
        } else {
            match self.audience {
                Audience::Youth => {
                    eprintln!(
                        "        3. Create 3 section images ({n}-section1.png … section3.png)",
                        n = self.study
                    );
                }
                Audience::Adult => {
                    eprintln!(
                        "        3. Create {} images (intro, tema, A/B, definición)",
                        self.images_total
                    );
                }
            }
            eprintln!("        4. Finish the cloud run");
            eprintln!(
                "      After that: download + build ({}) · whole study about {lo}–{hi} min.",
                eta.download_build
            );
        }
        eprintln!("      The spinner below shows which of these steps is active.");
    }

    /// Update from latest artifact list.
    /// Returns `(spinner_label, new_notes)` — notes are printed above the spinner.
    pub fn on_artifacts(
        &mut self,
        status: &str,
        artifacts: &[Artifact],
    ) -> (String, Vec<String>) {
        let json_name = format!("{}.json", self.study);
        let mut json = false;
        let mut imgs = 0usize;
        let mut notes = Vec::new();

        for art in artifacts {
            let base = art
                .path
                .rsplit('/')
                .next()
                .unwrap_or(art.path.as_str())
                .to_lowercase();
            if base == json_name.to_lowercase() || art.path.ends_with(&format!("/{json_name}")) {
                json = true;
            }
            if self.counts_as_image(&base) {
                imgs += 1;
            }
        }
        imgs = imgs.min(self.images_total);

        if json && !self.json_done {
            self.json_done = true;
            notes.push(format!(
                "Step 3/4 · JSON ready ({}.json)",
                self.study
            ));
        }
        if imgs > self.last_announced_images {
            self.images_done = imgs;
            self.last_announced_images = imgs;
            notes.push(format!(
                "Step 3/4 · images {imgs}/{}",
                self.images_total
            ));
        } else {
            self.images_done = imgs;
        }

        (self.spinner_label(status), notes)
    }

    pub fn spinner_label(&self, status: &str) -> String {
        let s = status.to_uppercase();
        let sub = if self.json_only {
            if !self.json_done {
                format!("writing {}.json", self.study)
            } else {
                "closing run (host images next)".to_string()
            }
        } else if !self.json_done {
            format!("3a writing {}.json", self.study)
        } else if self.images_done < self.images_total {
            format!("3b images {}/{}", self.images_done, self.images_total)
        } else {
            "3c closing run".to_string()
        };
        format!("NOW Step 3/4 — Agent · {sub} · {s}")
    }

    fn counts_as_image(&self, basename_lower: &str) -> bool {
        let is_img = basename_lower.ends_with(".png")
            || basename_lower.ends_with(".jpg")
            || basename_lower.ends_with(".jpeg")
            || basename_lower.ends_with(".webp");
        if !is_img {
            return false;
        }
        match self.audience {
            Audience::Youth => {
                basename_lower.contains("section1")
                    || basename_lower.contains("section2")
                    || basename_lower.contains("section3")
                    || basename_lower.contains(&format!("{}-section", self.study))
            }
            Audience::Adult => adult_image_basenames().iter().any(|name| {
                let stem = name.trim_end_matches(".png");
                basename_lower == *name
                    || basename_lower.starts_with(stem)
                    || basename_lower.contains(stem)
            }),
        }
    }
}
