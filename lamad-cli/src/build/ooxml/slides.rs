//! Slide paths, duplication, allocation, and presentation order.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use regex::Regex;

pub fn slide_path(build: &Path, num: u32) -> PathBuf {
    build
        .join("ppt")
        .join("slides")
        .join(format!("slide{num}.xml"))
}

pub fn list_slide_nums(build: &Path) -> Result<Vec<u32>> {
    let dir = build.join("ppt").join("slides");
    let mut nums = Vec::new();
    if !dir.is_dir() {
        return Ok(nums);
    }
    for entry in fs::read_dir(&dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if let Some(digits) = stem.strip_prefix("slide") {
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(n) = digits.parse::<u32>() {
                    nums.push(n);
                }
            }
        }
    }
    nums.sort_unstable();
    Ok(nums)
}

/// Copy slide XML + rels to a new `slideN`. Strips notes rels (never leaves
/// `notesSlide{old}` on the clone — see AGENTS.md package-integrity rules).
pub fn duplicate_slide(build: &Path, proto: u32) -> Result<u32> {
    let nums = list_slide_nums(build)?;
    let dst = nums.iter().copied().max().map(|m| m + 1).unwrap_or(1);
    let src_xml = slide_path(build, proto);
    let dst_xml = slide_path(build, dst);
    if !src_xml.is_file() {
        bail!("prototype slide missing: {}", src_xml.display());
    }
    fs::copy(&src_xml, &dst_xml)
        .with_context(|| format!("copy {} -> {}", src_xml.display(), dst_xml.display()))?;

    let rels_dir = build.join("ppt").join("slides").join("_rels");
    let src_rels = rels_dir.join(format!("slide{proto}.xml.rels"));
    let dst_rels = rels_dir.join(format!("slide{dst}.xml.rels"));
    if src_rels.is_file() {
        let mut rels = fs::read_to_string(&src_rels)
            .with_context(|| format!("read {}", src_rels.display()))?;
        let notes_re =
            Regex::new(r#"<Relationship\b[^>]*Type="[^"]*notesSlide"[^>]*/>"#).unwrap();
        rels = notes_re.replace_all(&rels, "").into_owned();
        fs::write(&dst_rels, rels).with_context(|| format!("write {}", dst_rels.display()))?;
    }

    let ct_path = build.join("[Content_Types].xml");
    let mut ct =
        fs::read_to_string(&ct_path).with_context(|| format!("read {}", ct_path.display()))?;
    let part = format!("/ppt/slides/slide{dst}.xml");
    if !ct.contains(&part) {
        let override_tag = format!(
            r#"<Override PartName="{part}" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        );
        ct = ct.replacen("</Types>", &format!("{override_tag}</Types>"), 1);
        fs::write(&ct_path, ct).with_context(|| format!("write {}", ct_path.display()))?;
    }
    Ok(dst)
}

/// Create `count` slides by duplicating `proto` (never reuses proto in-place).
pub fn allocate_slides(build: &Path, proto: u32, count: usize) -> Result<Vec<u32>> {
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(duplicate_slide(build, proto)?);
    }
    Ok(out)
}

/// Remove `notesSlide` relationships whose target number does not match the
/// slide number. Some shipped adult templates reference stale notes parts.
pub fn fix_notes_slide_rels(build: &Path) -> Result<()> {
    let rels_dir = build.join("ppt").join("slides").join("_rels");
    if !rels_dir.is_dir() {
        return Ok(());
    }
    let notes_re =
        Regex::new(r#"<Relationship\b[^>]*Type="[^"]*notesSlide"[^>]*/>"#).unwrap();
    let target_re = Regex::new(r"notesSlide(\d+)").unwrap();

    for entry in fs::read_dir(&rels_dir).with_context(|| format!("read_dir {}", rels_dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(sn) = name
            .strip_prefix("slide")
            .and_then(|s| s.strip_suffix(".xml.rels"))
        else {
            continue;
        };
        let mut rels =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut changed = false;
        if let Some(caps) = target_re.captures(&rels) {
            if &caps[1] != sn {
                rels = notes_re.replace_all(&rels, "").into_owned();
                changed = true;
            }
        }
        if changed {
            fs::write(&path, rels).with_context(|| format!("write {}", path.display()))?;
        }
    }
    Ok(())
}

/// Rebuild presentation slide rels + `sldIdLst` to exactly this order
/// (string-safe — never `ElementTree.write()` this file).
pub fn set_active_order(build: &Path, slide_nums: &[u32]) -> Result<()> {
    if slide_nums.is_empty() {
        bail!("active order is empty");
    }
    for &n in slide_nums {
        let p = slide_path(build, n);
        if !p.is_file() {
            bail!("active slide missing on disk: slide{n}.xml");
        }
    }

    let rels_path = build.join("ppt").join("_rels").join("presentation.xml.rels");
    let mut rels =
        fs::read_to_string(&rels_path).with_context(|| format!("read {}", rels_path.display()))?;

    let drop_re = Regex::new(
        r#"<(?:[\w]+:)?Relationship\b[^>]*Type="[^"]*/relationships/slide"[^>]*/>\s*"#,
    )
    .unwrap();
    rels = drop_re.replace_all(&rels, "").into_owned();

    let id_re = Regex::new(r#"\bId="rId(\d+)""#).unwrap();
    let used: Vec<u32> = id_re
        .captures_iter(&rels)
        .filter_map(|c| c[1].parse::<u32>().ok())
        .collect();
    let start_id = used.iter().copied().max().map(|m| m + 1).unwrap_or(1);

    let mut rids: Vec<String> = Vec::with_capacity(slide_nums.len());
    let mut chunks = String::new();
    for (i, &n) in slide_nums.iter().enumerate() {
        let rid = format!("rId{}", start_id + i as u32);
        chunks.push_str(&format!(
            r#"<Relationship Id="{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{n}.xml"/>"#
        ));
        rids.push(rid);
    }

    if !rels.contains("</Relationships>") {
        bail!("presentation.xml.rels missing </Relationships>");
    }
    rels = rels.replacen("</Relationships>", &format!("{chunks}</Relationships>"), 1);
    fs::write(&rels_path, rels).with_context(|| format!("write {}", rels_path.display()))?;

    let pres_path = build.join("ppt").join("presentation.xml");
    let pres =
        fs::read_to_string(&pres_path).with_context(|| format!("read {}", pres_path.display()))?;
    let mut items = String::new();
    for (i, rid) in rids.iter().enumerate() {
        items.push_str(&format!(r#"<p:sldId id="{}" r:id="{rid}"/>"#, 256 + i));
    }
    let sldid_re = Regex::new(r"(?s)<p:sldIdLst>.*?</p:sldIdLst>").unwrap();
    let mut count = 0usize;
    let new_pres = sldid_re.replace(&pres, |_: &regex::Captures| {
        count += 1;
        format!("<p:sldIdLst>{items}</p:sldIdLst>")
    });
    if count != 1 {
        bail!("failed to rewrite p:sldIdLst");
    }
    fs::write(&pres_path, new_pres.into_owned())
        .with_context(|| format!("write {}", pres_path.display()))?;
    Ok(())
}
