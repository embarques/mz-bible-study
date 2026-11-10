//! Human-readable prepare steps while a Cursor cloud agent is RUNNING.

use crate::agent::client::Artifact;
use crate::job::Audience;
use crate::progress;
use std::collections::BTreeSet;

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

/// Tracks JSON / section folders + image artifacts and drives spinner updates.
pub struct CloudAgentProgress {
    study: u32,
    audience: Audience,
    /// When true, agent only writes text (JSON or section tree); host may do images.
    json_only: bool,
    images_total: usize,
    json_done: bool,
    images_done: usize,
    last_announced_images: usize,
    plan_printed: bool,
    /// Adult section folders already seen (e.g. `01-title`).
    sections_seen: BTreeSet<String>,
    last_section_count: usize,
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
            sections_seen: BTreeSet::new(),
            last_section_count: 0,
        }
    }

    /// Print the expected steps once (under the current study).
    pub fn print_plan(&mut self) {
        if self.plan_printed {
            return;
        }
        self.plan_printed = true;
        let aud = self.audience.as_str();
        let eta = progress::prepare_eta(self.audience, false, self.json_only);
        let (lo, hi) = eta.per_study_min;
        progress::info(format!(
            "Cursor agent — estudio {} ({aud}). Usually {}.",
            self.study, eta.agent_work
        ));
        match self.audience {
            Audience::Adult => {
                eprintln!("      Writes section folders one-by-one under artifacts/{}/ …", self.study);
                eprintln!("        manifest.toml → 01-title → 02-lectura → … → temas → images");
                eprintln!("      Spinner updates as each section folder appears (resume-safe).");
            }
            Audience::Youth => {
                eprintln!(
                    "      Extract study text → artifacts/{}.json, then 3 section images.",
                    self.study
                );
            }
        }
        eprintln!("      Whole study about {lo}–{hi} min after this step + build.");
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
        let prefix = format!("artifacts/{}/", self.study);
        let prefix_alt = format!("{}/", self.study);
        let mut found_sections = BTreeSet::new();

        for art in artifacts {
            let path = art.path.replace('\\', "/");
            let base = path
                .rsplit('/')
                .next()
                .unwrap_or(path.as_str())
                .to_lowercase();
            if base == json_name.to_lowercase() || path.ends_with(&format!("/{json_name}")) {
                json = true;
            }
            if self.counts_as_image(&base) {
                imgs += 1;
            }
            if self.audience == Audience::Adult {
                if let Some(rel) = path
                    .strip_prefix(&prefix)
                    .or_else(|| path.strip_prefix(&prefix_alt))
                {
                    let section = rel.split('/').next().unwrap_or("");
                    if section == "manifest.toml" {
                        found_sections.insert("manifest.toml".to_string());
                    } else if !section.is_empty() && section != "images" && section.contains('-') {
                        found_sections.insert(section.to_string());
                    }
                }
            }
        }
        imgs = imgs.min(self.images_total);

        if self.audience == Audience::Adult {
            for s in found_sections.difference(&self.sections_seen) {
                notes.push(format!("Step 3/4 · section ready: {s}"));
            }
            self.sections_seen = found_sections;
            self.last_section_count = self.sections_seen.len();
            if !self.sections_seen.is_empty() {
                self.json_done = true;
            }
        }

        if json && !self.json_done {
            self.json_done = true;
            notes.push(format!("Step 3/4 · JSON ready ({}.json)", self.study));
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
        let sub = if self.audience == Audience::Adult {
            if self.sections_seen.is_empty() && !self.json_done {
                format!("writing sections → artifacts/{}/ …", self.study)
            } else if !self.sections_seen.is_empty() {
                let last = self.sections_seen.iter().next_back().map(|s| s.as_str()).unwrap_or("…");
                if self.json_only || self.images_done >= self.images_total {
                    format!(
                        "sections {} done (latest {last})",
                        self.sections_seen.len()
                    )
                } else {
                    format!(
                        "sections {} · images {}/{} (latest {last})",
                        self.sections_seen.len(),
                        self.images_done,
                        self.images_total
                    )
                }
            } else if self.json_only {
                if !self.json_done {
                    format!("extracting text → {}.json", self.study)
                } else {
                    "closing run (host images next)".to_string()
                }
            } else if !self.json_done {
                format!("3a extracting text → {}.json", self.study)
            } else if self.images_done < self.images_total {
                format!("3b Cursor images {}/{}", self.images_done, self.images_total)
            } else {
                "3c closing run".to_string()
            }
        } else if self.json_only {
            if !self.json_done {
                format!("extracting text → {}.json", self.study)
            } else {
                "closing run (host images next)".to_string()
            }
        } else if !self.json_done {
            format!("3a extracting text → {}.json", self.study)
        } else if self.images_done < self.images_total {
            format!("3b Cursor images {}/{}", self.images_done, self.images_total)
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
