//! Low-level OOXML helpers — port of `python/mz_bible_study/build/study.py`
//! (the OOXML plumbing half: zip/unzip, slide duplication/ordering, shape
//! text surgery, diagram text surgery).
//!
//! HARD RULES (see AGENTS.md):
//! - `[Content_Types].xml` and `ppt/_rels/presentation.xml.rels` are edited
//!   with plain STRING surgery only — never through a DOM writer that could
//!   emit `ns0:` prefixes.
//! - Zipping only skips `.DS_Store`, `._*`, `~$*` — never `startswith(".")`
//!   (that would drop `_rels/.rels` and corrupt the package).
//! - `duplicate_slide` strips the `notesSlide` relationship from the clone's
//!   `.rels` (never leaves `notesSlide{old}` pointing at the wrong slide).
//! - `a:solidFill` must be inserted BEFORE `a:latin`/`a:ea`/`a:cs` inside
//!   `a:rPr` — PowerPoint ignores a fill appended after the typeface.
//! - Shapes named `Arc*` (retired dashed/dotted chrome) are always stripped
//!   from section-chrome slides.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;

// ---------------------------------------------------------------------------
// Zip / unzip
// ---------------------------------------------------------------------------

/// Extract a `.pptx` package into `dest` (wiping `dest` first).
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

// ---------------------------------------------------------------------------
// Slide allocation / ordering
// ---------------------------------------------------------------------------

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
    let mut next_id = used.iter().copied().max().map(|m| m + 1).unwrap_or(1);

    let mut rids: Vec<String> = Vec::with_capacity(slide_nums.len());
    let mut chunks = String::new();
    for &n in slide_nums {
        let rid = format!("rId{next_id}");
        next_id += 1;
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

// ---------------------------------------------------------------------------
// Tiny non-nesting XML scanner used for shape/run/paragraph surgery.
//
// The tags we touch here (a:r, a:rPr, a:t, a:p, a:pPr, p:txBody, p:sp, ...)
// never nest a same-named descendant in the slide XML this template
// produces, so a linear "find matching close tag" scan is sufficient and
// avoids pulling in a full DOM (which risks emitting `nsN:` prefixes).
// ---------------------------------------------------------------------------
mod xml {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    pub struct Elem {
        pub start: usize,
        /// End of the opening tag (position right after its `>`).
        pub open_end: usize,
        /// End of the whole element (position right after the closing tag,
        /// or right after `/>` for self-closing elements).
        pub end: usize,
        pub self_closing: bool,
    }

    impl Elem {
        pub fn open_tag<'a>(&self, s: &'a str) -> &'a str {
            &s[self.start..self.open_end]
        }
        pub fn inner<'a>(&self, s: &'a str, tag: &str) -> &'a str {
            if self.self_closing {
                ""
            } else {
                let close_len = 3 + tag.len(); // "</" + tag + ">"
                &s[self.open_end..self.end - close_len]
            }
        }
        pub fn whole<'a>(&self, s: &'a str) -> &'a str {
            &s[self.start..self.end]
        }
    }

    fn is_boundary(b: Option<u8>) -> bool {
        matches!(b, None | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/'))
    }

    /// Find the next occurrence of `<tag` (name-boundary safe, e.g. searching
    /// for `a:r` will not match `a:rPr`) at or after `from`, and parse the
    /// full element (self-closing or paired).
    pub fn next_element(s: &str, tag: &str, from: usize) -> Option<Elem> {
        let needle = format!("<{tag}");
        let mut search_from = from;
        loop {
            let rel = s.get(search_from..)?.find(&needle)?;
            let start = search_from + rel;
            let after = start + needle.len();
            if is_boundary(s.as_bytes().get(after).copied()) {
                let gt = s.get(after..)?.find('>')? + after;
                let self_closing = s.as_bytes()[gt - 1] == b'/';
                if self_closing {
                    return Some(Elem {
                        start,
                        open_end: gt + 1,
                        end: gt + 1,
                        self_closing: true,
                    });
                }
                let close_needle = format!("</{tag}>");
                let close_rel = s.get(gt + 1..)?.find(&close_needle)?;
                let close_pos = gt + 1 + close_rel;
                return Some(Elem {
                    start,
                    open_end: gt + 1,
                    end: close_pos + close_needle.len(),
                    self_closing: false,
                });
            }
            search_from = after;
        }
    }

    /// All (possibly nested-in-other-tags) occurrences of `tag` in `s`, in
    /// document order, found by repeatedly scanning past each match.
    pub fn all_elements(s: &str, tag: &str) -> Vec<Elem> {
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some(el) = next_element(s, tag, pos) {
            pos = el.end.max(el.start + 1);
            out.push(el);
        }
        out
    }

    pub fn escape_text(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    /// Build a `<a:t>` element, adding `xml:space="preserve"` when needed.
    pub fn build_t(text: &str) -> String {
        let preserve = text.is_empty()
            || text.starts_with(' ')
            || text.ends_with(' ')
            || text != text.trim();
        let escaped = escape_text(text);
        if preserve {
            format!(r#"<a:t xml:space="preserve">{escaped}</a:t>"#)
        } else {
            format!("<a:t>{escaped}</a:t>")
        }
    }

    pub fn run_text(run: &str) -> String {
        match next_element(run, "a:t", 0) {
            Some(t) => decode_entities(t.inner(run, "a:t")),
            None => String::new(),
        }
    }

    fn decode_entities(s: &str) -> String {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    }

    /// Set/unset `b="1"` on an `<a:rPr ...>` opening tag string, in place.
    pub fn set_bold_attr(open_tag: &str, bold: Option<bool>) -> String {
        match bold {
            None => open_tag.to_string(),
            Some(true) => {
                let re = Regex::new(r#"\sb="[^"]*""#).unwrap();
                if re.is_match(open_tag) {
                    re.replace(open_tag, r#" b="1""#).into_owned()
                } else {
                    insert_attr(open_tag, "b", "1")
                }
            }
            Some(false) => {
                let re = Regex::new(r#"\sb="[^"]*""#).unwrap();
                re.replace(open_tag, "").into_owned()
            }
        }
    }

    pub fn set_sz_attr(open_tag: &str, sz: u32) -> String {
        let re = Regex::new(r#"\ssz="[^"]*""#).unwrap();
        if re.is_match(open_tag) {
            re.replace(open_tag, format!(r#" sz="{sz}""#)).into_owned()
        } else {
            insert_attr(open_tag, "sz", &sz.to_string())
        }
    }

    fn insert_attr(open_tag: &str, name: &str, value: &str) -> String {
        // open_tag looks like `<a:rPr` or `<a:rPr foo="1"` possibly ending in
        // `/>` or `>`; insert right after the tag name.
        let tag_end = open_tag
            .find(|c: char| c == ' ' || c == '/' || c == '>')
            .unwrap_or(open_tag.len());
        format!(
            r#"{} {}="{}"{}"#,
            &open_tag[..tag_end],
            name,
            value,
            &open_tag[tag_end..]
        )
    }

    /// Force `typeface="..."` on the given direct-child tag (e.g. `a:latin`)
    /// inside `children`, creating the element (self-closing) if missing.
    pub fn force_typeface(children: &str, tag: &str, typeface: &str) -> String {
        if let Some(el) = next_element(children, tag, 0) {
            let open = el.open_tag(children);
            let re = Regex::new(r#"\stypeface="[^"]*""#).unwrap();
            let new_open = if re.is_match(open) {
                re.replace(open, format!(r#" typeface="{typeface}""#))
                    .into_owned()
            } else {
                insert_attr(open, "typeface", typeface)
            };
            let mut out = String::with_capacity(children.len());
            out.push_str(&children[..el.start]);
            out.push_str(&new_open);
            out.push_str(&children[el.open_end..el.end]);
            out.push_str(&children[el.end..]);
            out
        } else {
            format!(r#"{children}<{tag} typeface="{typeface}"/>"#)
        }
    }

    /// Remove the direct-child `<a:solidFill>` from `rpr_children` (ignoring
    /// any `a:solidFill` nested inside e.g. `a:ln`), then, if `rgb` is set,
    /// insert a fresh one right before the first `a:latin`/`a:ea`/`a:cs`/
    /// `a:sym` child (or at the end if none) — solidFill MUST precede the
    /// typeface elements or PowerPoint ignores the color.
    pub fn set_run_color(rpr_children: &str, rgb: Option<&str>) -> String {
        let mut children = rpr_children.to_string();
        // Remove only a TOP-LEVEL a:solidFill (direct child), not one nested
        // inside a:ln/etc — scan top-level siblings only.
        let mut pos = 0usize;
        while let Some(el) = next_element(&children, "a:solidFill", pos) {
            // Confirm this is a top-level sibling: walk siblings from 0.
            if is_top_level(&children, el.start) {
                children = format!("{}{}", &children[..el.start], &children[el.end..]);
                break;
            }
            pos = el.end;
        }
        let Some(rgb) = rgb else { return children };
        let fill = format!(r#"<a:solidFill><a:srgbClr val="{rgb}"/></a:solidFill>"#);
        let insert_at = ["a:latin", "a:ea", "a:cs", "a:sym"]
            .iter()
            .filter_map(|t| top_level_element(&children, t))
            .map(|e| e.start)
            .min();
        match insert_at {
            Some(pos) => format!("{}{}{}", &children[..pos], fill, &children[pos..]),
            None => format!("{children}{fill}"),
        }
    }

    /// True if the element starting at `target` is a top-level sibling (not
    /// nested inside an earlier top-level element) of `s`.
    fn is_top_level(s: &str, target: usize) -> bool {
        let mut pos = 0usize;
        while pos < target {
            match next_sibling(s, pos) {
                Some(el) if el.start == target => return true,
                Some(el) if el.end <= target => pos = el.end,
                _ => return false,
            }
        }
        pos == target
    }

    /// Find the first top-level sibling element with this tag name.
    fn top_level_element(s: &str, tag: &str) -> Option<Elem> {
        let mut pos = 0usize;
        loop {
            let el = next_sibling(s, pos)?;
            if s[el.start..el.open_end].trim_start_matches('<').starts_with(tag) {
                return Some(el);
            }
            pos = el.end;
        }
    }

    /// Parse the next top-level sibling element starting at or after `pos`
    /// (assumes `s[pos..]` begins at a sibling boundary — i.e. only ever
    /// call with `pos = 0` or a previous sibling's `.end`).
    fn next_sibling(s: &str, pos: usize) -> Option<Elem> {
        let rest = s.get(pos..)?;
        let trimmed_offset = rest.len() - rest.trim_start().len();
        let start = pos + trimmed_offset;
        if start >= s.len() {
            return None;
        }
        if s.as_bytes().get(start) != Some(&b'<') {
            return None;
        }
        let name_start = start + 1;
        let mut j = name_start;
        let bytes = s.as_bytes();
        while j < s.len() && !matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>') {
            j += 1;
        }
        let tag = &s[name_start..j];
        next_element(s, tag, start)
    }
}

use xml::Elem;

// ---------------------------------------------------------------------------
// Shape lookup
// ---------------------------------------------------------------------------

/// Locate the `<p:sp>...</p:sp>` block whose `cNvPr` has `name="name"`.
fn shape_block(slide_xml: &str, name: &str) -> Result<(usize, usize)> {
    let needle = format!(r#"name="{name}""#);
    let name_pos = slide_xml
        .find(&needle)
        .ok_or_else(|| anyhow!("shape not found: {name}"))?;
    let start = slide_xml[..name_pos]
        .rfind("<p:sp")
        .ok_or_else(|| anyhow!("no enclosing <p:sp> for shape: {name}"))?;
    let rel_end = slide_xml[name_pos..]
        .find("</p:sp>")
        .ok_or_else(|| anyhow!("no closing </p:sp> for shape: {name}"))?;
    let end = name_pos + rel_end + "</p:sp>".len();
    Ok((start, end))
}

fn replace_range(s: &str, start: usize, end: usize, with: &str) -> String {
    format!("{}{}{}", &s[..start], with, &s[end..])
}

/// Apply `f` to the shape block named `name` within `slide_xml`, splicing
/// the result back in.
fn transform_shape(
    slide_xml: &str,
    name: &str,
    f: impl FnOnce(&str) -> Result<String>,
) -> Result<String> {
    let (start, end) = shape_block(slide_xml, name)?;
    let new_block = f(&slide_xml[start..end])?;
    Ok(replace_range(slide_xml, start, end, &new_block))
}

/// Locate `<p:txBody>` (or, rarely, `<a:txBody>`) within a shape block.
fn find_txbody(shape_xml: &str) -> Result<(Elem, &'static str)> {
    if let Some(el) = xml::next_element(shape_xml, "p:txBody", 0) {
        return Ok((el, "p:txBody"));
    }
    if let Some(el) = xml::next_element(shape_xml, "a:txBody", 0) {
        return Ok((el, "a:txBody"));
    }
    bail!("shape has no txBody")
}

const DEFAULT_RUN: &str = r#"<a:r><a:rPr lang="es-MX"/><a:t></a:t></a:r>"#;

/// Build a new run from `sample` (an existing `<a:r>...</a:r>` string, or
/// `None` to fall back to a bare run), setting its text and — when
/// requested — bold / fill color / forced size+typeface.
fn clone_run(
    sample: Option<&str>,
    text: &str,
    bold: Option<bool>,
    color: Option<Option<&str>>,
    force_sz_typeface: Option<(u32, &str)>,
) -> Result<String> {
    let sample = sample.unwrap_or(DEFAULT_RUN);
    let rpr = xml::next_element(sample, "a:rPr", 0)
        .ok_or_else(|| anyhow!("run has no a:rPr"))?;
    let open = rpr.open_tag(sample);
    let mut new_open = xml::set_bold_attr(open, bold);
    let mut children = if rpr.self_closing {
        String::new()
    } else {
        rpr.inner(sample, "a:rPr").to_string()
    };

    if let Some(rgb) = color {
        children = xml::set_run_color(&children, rgb);
    }
    if let Some((sz, typeface)) = force_sz_typeface {
        new_open = xml::set_sz_attr(&new_open, sz);
        for tag in ["a:latin", "a:ea", "a:cs"] {
            children = xml::force_typeface(&children, tag, typeface);
        }
    }

    let new_rpr = if children.is_empty() && rpr.self_closing {
        // still self-closing (open tag already ends with "/>")
        new_open
    } else {
        // ensure open tag doesn't self-close now that we (may) have children
        let opening = if new_open.ends_with("/>") {
            format!("{}>", &new_open[..new_open.len() - 2])
        } else {
            new_open
        };
        format!("{opening}{children}</a:rPr>")
    };

    Ok(format!("<a:r>{new_rpr}{}</a:r>", xml::build_t(text)))
}

/// First `<a:r>...</a:r>` anywhere inside `s` (used as a style sample).
fn first_run(s: &str) -> Option<String> {
    xml::next_element(s, "a:r", 0).map(|e| e.whole(s).to_string())
}

/// All `<a:r>...</a:r>` runs anywhere inside `s`, in document order.
fn all_runs(s: &str) -> Vec<String> {
    xml::all_elements(s, "a:r")
        .into_iter()
        .map(|e| e.whole(s).to_string())
        .collect()
}

/// First `<a:pPr>...` (self-closing or paired) inside the first `<a:p>`
/// paragraph of `content`, if any.
fn first_para_ppr(content: &str) -> Option<String> {
    let p = xml::next_element(content, "a:p", 0)?;
    let inner = p.inner(content, "a:p");
    xml::next_element(inner, "a:pPr", 0).map(|e| e.whole(inner).to_string())
}

// ---------------------------------------------------------------------------
// Generic paragraph-body replacement (title/section/AB/content shapes)
// ---------------------------------------------------------------------------

/// Replace shape text with a single run, preserving first-run formatting
/// and the first paragraph's `pPr` (port of `set_simple_text`).
fn set_simple_text_block(shape_xml: &str, text: &str, bold: Option<bool>) -> Result<String> {
    let (tb, tag) = find_txbody(shape_xml)?;
    let content = tb.inner(shape_xml, tag);
    let p_start = xml::next_element(content, "a:p", 0).map(|e| e.start);
    let head = match p_start {
        Some(p) => &content[..p],
        None => content,
    };
    let sample = first_run(content);
    let ppr = first_para_ppr(content).unwrap_or_default();
    let run = clone_run(sample.as_deref(), text, bold, None, None)?;
    let new_paragraph = format!(r#"<a:p>{ppr}{run}<a:endParaRPr lang="es-DO"/></a:p>"#);
    let new_content = format!("{head}{new_paragraph}");
    let open_tag = tb.open_tag(shape_xml);
    let new_txbody = format!("{open_tag}{new_content}</{tag}>");
    Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
}

const REF_PATTERN: &str = r"(\((?:v\.?\s*\d+|[1-3]?\s*[A-Za-zÁÉÍÓÚáéíóúñÑ][A-Za-zÁÉÍÓÚáéíóúñÑ\s]+\s+\d+[^\)]*)\)|(?:[1-3]?\s*[A-Za-zÁÉÍÓÚáéíóúñÑ][A-Za-zÁÉÍÓÚáéíóúñÑ\.]*(?:\s+[A-Za-zÁÉÍÓÚáéíóúñÑ\.]+)*)\s+\d+:\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)";

/// Body text with inline scripture references (`(Efesios 2:1)`, `Josué
/// 10:7-14`, ...) bolded; everything else regular weight. When
/// `force_tnr_42` is set, every run is forced to Times New Roman 42pt
/// (Conclusión — see AGENTS.md hard rule).
fn set_body_with_refs_block(shape_xml: &str, text: &str, force_tnr_42: bool) -> Result<String> {
    let (tb, tag) = find_txbody(shape_xml)?;
    let content = tb.inner(shape_xml, tag);
    let p_start = xml::next_element(content, "a:p", 0).map(|e| e.start);
    let head = match p_start {
        Some(p) => &content[..p],
        None => content,
    };
    let sample = first_run(content);
    let ppr = first_para_ppr(content).unwrap_or_default();

    let ref_re = Regex::new(REF_PATTERN).context("compile REF_RE")?;
    let mut parts: Vec<(String, bool)> = Vec::new();
    let mut pos = 0usize;
    for m in ref_re.find_iter(text) {
        if m.start() > pos {
            parts.push((text[pos..m.start()].to_string(), false));
        }
        parts.push((m.as_str().to_string(), true));
        pos = m.end();
    }
    if pos < text.len() {
        parts.push((text[pos..].to_string(), false));
    }
    if parts.is_empty() {
        parts.push((text.to_string(), false));
    }

    let force = force_tnr_42.then_some((4200u32, "Times New Roman"));
    let mut runs = String::new();
    for (chunk, is_ref) in parts {
        if chunk.is_empty() {
            continue;
        }
        let bold = Some(is_ref);
        runs.push_str(&clone_run(sample.as_deref(), &chunk, bold, None, force)?);
    }
    let end_sz = if force_tnr_42 { r#" sz="4200""# } else { "" };
    let new_paragraph = format!(r#"<a:p>{ppr}{runs}<a:endParaRPr lang="es-DO"{end_sz}/></a:p>"#);
    let new_content = format!("{head}{new_paragraph}");
    let open_tag = tb.open_tag(shape_xml);
    let new_txbody = format!("{open_tag}{new_content}</{tag}>");
    Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
}

// ---------------------------------------------------------------------------
// Title / Próximo slides
// ---------------------------------------------------------------------------

/// Fill the title (or Próximo) slide: número, título, and Base Bíblica
/// (label bold, citation lines regular — never bold the verses here).
pub fn set_title_slide(path: &Path, numero: &str, titulo: &str, base: &[String]) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, "TextBox 4", |b| set_simple_text_block(b, numero, Some(true)))?;
    // Verlag Black title — never force bold=true; leave template weight as-is.
    let xml = transform_shape(&xml, "CuadroTexto 13", |b| set_simple_text_block(b, titulo, Some(false)))?;
    let xml = transform_shape(&xml, "TextBox 7", |b| set_base_biblica_block(b, base))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

fn set_base_biblica_block(shape_xml: &str, base: &[String]) -> Result<String> {
    let (tb, tag) = find_txbody(shape_xml)?;
    let content = tb.inner(shape_xml, tag);
    let sample = first_run(content);

    let mut p = String::from(r#"<a:p><a:pPr algn="ctr"/>"#);
    p.push_str(&clone_run(sample.as_deref(), "Base Bíblica:", Some(true), None, None)?);
    for line in base {
        p.push_str("<a:br/>");
        p.push_str(&clone_run(sample.as_deref(), line, Some(false), None, None)?);
    }
    p.push_str("</a:p>");

    let open_tag = tb.open_tag(shape_xml);
    let new_txbody = format!("{open_tag}{p}</{tag}>");
    Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
}

// ---------------------------------------------------------------------------
// Lectura / Texto Bíblico verses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerseKind {
    Lectura,
    Texto,
}

/// Split `"18 Y el niño creció..."` into `("18", " Y el niño creció...")`.
fn split_verse(v: &str) -> Result<(String, String)> {
    let re = Regex::new(r"(?s)^(\d+)\s+(.*)$").unwrap();
    let caps = re
        .captures(v.trim_start())
        .ok_or_else(|| anyhow!("verse must start with number: {:.60}", v))?;
    Ok((caps[1].to_string(), format!(" {}", &caps[2])))
}

struct VerseSamples {
    cite: Option<String>,
    num: Option<String>,
    body: Option<String>,
}

/// Pick cite/number/body run samples by content, not fixed index —
/// continuation Lectura/Texto slides start with a number run, so
/// index-based picking would swap colors (port of `pick_verse_samples`).
fn pick_verse_samples(runs: &[String]) -> VerseSamples {
    let mut sample_num: Option<String> = None;
    let mut sample_body: Option<String> = None;
    let mut sample_cite: Option<String> = None;

    for r in runs {
        let text = xml::run_text(r);
        let stripped = text.trim();
        if stripped.is_empty() {
            continue;
        }
        if !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit()) {
            if sample_num.is_none() {
                sample_num = Some(r.clone());
            }
            continue;
        }
        if sample_cite.is_none() && stripped.contains(':') && stripped.chars().count() < 40 {
            sample_cite = Some(r.clone());
            continue;
        }
        if sample_body.is_none() && (text.starts_with(' ') || stripped.chars().count() > 15) {
            sample_body = Some(r.clone());
        }
    }

    if sample_num.is_none() {
        sample_num = runs
            .iter()
            .find(|r| {
                let t = xml::run_text(r);
                let s = t.trim();
                !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
            })
            .cloned()
            .or_else(|| runs.first().cloned());
    }
    if sample_body.is_none() {
        sample_body = runs
            .iter()
            .find(|r| {
                let t = xml::run_text(r);
                let s = t.trim();
                !(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            })
            .cloned()
            .or_else(|| runs.last().cloned());
    }
    if sample_cite.is_none() {
        sample_cite = sample_num.clone();
    }

    VerseSamples {
        cite: sample_cite,
        num: sample_num,
        body: sample_body,
    }
}

/// Fill a Lectura/Texto Bíblico slide with whole verses (+ optional
/// citation on the first slide of a range). One paragraph per citation /
/// per verse — no soft `a:br` breaks (matches known-good decks).
pub fn set_verses(path: &Path, verses: &[String], citation: Option<&str>, kind: VerseKind) -> Result<()> {
    let shape_name = match kind {
        VerseKind::Lectura => "CuadroTexto 5",
        VerseKind::Texto => "TextBox 4",
    };
    let accent = match kind {
        VerseKind::Lectura => "FF0000",
        VerseKind::Texto => "FFFF00",
    };
    let body_rgb: Option<&str> = match kind {
        VerseKind::Lectura => None, // default black — leave inherited
        VerseKind::Texto => Some("FFFFFF"),
    };

    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape_name, |shape_xml| {
        let (tb, tag) = find_txbody(shape_xml)?;
        let content = tb.inner(shape_xml, tag);
        let runs = all_runs(content);
        if runs.is_empty() {
            bail!("no sample runs in shape {shape_name}");
        }
        let samples = pick_verse_samples(&runs);

        let p_start = xml::next_element(content, "a:p", 0)
            .map(|e| e.start)
            .unwrap_or(content.len());
        let head = &content[..p_start];
        let ppr = first_para_ppr(content).unwrap_or_default();

        let mut body = String::new();
        if let Some(cite) = citation {
            let r = clone_run(samples.cite.as_deref(), cite, Some(true), Some(Some(accent)), None)?;
            body.push_str(&format!("<a:p>{ppr}{r}</a:p>"));
        }
        for v in verses {
            let (num, verse_body) = split_verse(v)?;
            let r_num = clone_run(samples.num.as_deref(), &num, Some(true), Some(Some(accent)), None)?;
            // body_rgb=None means "leave inherited" (lectura black body) —
            // color=None (outer Option) skips touching the fill entirely.
            let color_opt = body_rgb.map(Some);
            let r_body = clone_run(samples.body.as_deref(), &verse_body, Some(false), color_opt, None)?;
            body.push_str(&format!("<a:p>{ppr}{r_num}{r_body}</a:p>"));
        }

        let new_content = format!("{head}{body}");
        let open_tag = tb.open_tag(shape_xml);
        let new_txbody = format!("{open_tag}{new_content}</{tag}>");
        Ok(replace_range(shape_xml, tb.start, tb.end, &new_txbody))
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

// ---------------------------------------------------------------------------
// Section chrome (1-/2-/3- title slides)
// ---------------------------------------------------------------------------

/// Fill a section-chrome slide's title (`N- TITLE`) and verse range, and
/// strip any retired `Arc*` dashed/dotted chrome shapes.
pub fn set_section_chrome(path: &Path, title: &str, rango: &str, n: u32) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let title_text = format!("{n}- {title}");
    let xml = transform_shape(&xml, "CuadroTexto 8", |b| {
        set_simple_text_block(b, &title_text, Some(true))
    })?;
    let xml = transform_shape(&xml, "CuadroTexto 3", |b| set_simple_text_block(b, rango, Some(true)))?;
    let xml = strip_arc_shapes(&xml);
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

/// Remove any `<p:sp>...</p:sp>` block whose shape name starts with `Arc`
/// (retired yellow dashed/dotted chrome — see AGENTS.md).
fn strip_arc_shapes(xml: &str) -> String {
    let mut s = xml.to_string();
    loop {
        let Some(pos) = s.find(r#"name="Arc"#) else { break };
        let Some(start) = s[..pos].rfind("<p:sp") else { break };
        let Some(rel_end) = s[pos..].find("</p:sp>") else { break };
        let end = pos + rel_end + "</p:sp>".len();
        s.replace_range(start..end, "");
    }
    s
}

// ---------------------------------------------------------------------------
// A/B point bodies
// ---------------------------------------------------------------------------

/// Set the `N.A- TITLE` / `N.B- TITLE` heading (bold Times New Roman ~40pt
/// — preserve the template's bold; we only ever set bold=true here).
pub fn set_ab_title(path: &Path, title: &str) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, "Título 1", |b| set_simple_text_block(b, title, Some(true)))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

// ---------------------------------------------------------------------------
// Comentario / Introducción / A-B / Conclusión bodies
// ---------------------------------------------------------------------------

/// Set a content-body shape's text, regular weight with bold inline
/// scripture references. `shape` defaults to `Marcador de contenido 2`;
/// pass `Some("CuadroTexto 5")` for Conclusión. `conclusion=true` forces
/// Times New Roman 42pt on every run (AGENTS.md hard rule — the template
/// often leaves Verlag Light here, which is wrong).
pub fn set_content_body(path: &Path, text: &str, shape: Option<&str>, conclusion: bool) -> Result<()> {
    let shape_name = shape.unwrap_or("Marcador de contenido 2");
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let fallback_conclusion_shape = "CuadroTexto 5";
    let has_primary = xml.contains(&format!(r#"name="{shape_name}""#));
    let effective_name = if !has_primary && conclusion {
        fallback_conclusion_shape
    } else {
        shape_name
    };
    let xml = transform_shape(&xml, effective_name, |b| {
        set_body_with_refs_block(b, text, conclusion)
    })?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

// ---------------------------------------------------------------------------
// Diagrams: Propósitos / Idea principal / Para memorizar
// ---------------------------------------------------------------------------

/// Replace the longest / purpose-like `<a:t>` nodes (in document order)
/// with `texts`, in both `data_path` and (if given) `drawing_path`. When
/// `force_drawing_sz` is set, every `a:rPr@sz >= 5000` in `drawing_path` is
/// forced down to it (Propósitos must render at 36pt — see AGENTS.md).
pub fn replace_diagram_texts(
    data_path: &Path,
    drawing_path: Option<&Path>,
    texts: &[String],
    force_drawing_sz: Option<u32>,
) -> Result<()> {
    let xml = fs::read_to_string(data_path).with_context(|| format!("read {}", data_path.display()))?;
    let new_xml = replace_longest_texts(&xml, texts)
        .with_context(|| format!("diagram text count mismatch in {}", data_path.display()))?;
    fs::write(data_path, new_xml).with_context(|| format!("write {}", data_path.display()))?;

    if let Some(dp) = drawing_path {
        if dp.is_file() {
            let dxml = fs::read_to_string(dp).with_context(|| format!("read {}", dp.display()))?;
            let dxml = replace_longest_texts(&dxml, texts).unwrap_or(dxml);
            let dxml = match force_drawing_sz {
                Some(sz) => force_min_sz(&dxml, 5000, sz),
                None => dxml,
            };
            fs::write(dp, dxml).with_context(|| format!("write {}", dp.display()))?;
        }
    }
    Ok(())
}

/// Force every `<a:rPr ... sz="NNNN" ...>` with `sz >= threshold` down to
/// `new_sz` (Propósitos template runs at 5600 → must become 3600, see
/// AGENTS.md hard rule).
pub fn force_propositos_36pt(drawing2: &Path) -> Result<()> {
    let xml = fs::read_to_string(drawing2).with_context(|| format!("read {}", drawing2.display()))?;
    let xml = force_min_sz(&xml, 5000, 3600);
    fs::write(drawing2, xml).with_context(|| format!("write {}", drawing2.display()))
}

fn force_min_sz(xml: &str, threshold: u32, new_sz: u32) -> String {
    let re = Regex::new(r#"(<a:rPr\b[^>]*\bsz=")(\d+)("[^>]*>)"#).unwrap();
    re.replace_all(xml, |caps: &regex::Captures| {
        let sz: u32 = caps[2].parse().unwrap_or(0);
        if sz >= threshold {
            format!("{}{}{}", &caps[1], new_sz, &caps[3])
        } else {
            caps[0].to_string()
        }
    })
    .into_owned()
}

fn replace_longest_texts(xml: &str, texts: &[String]) -> Result<String> {
    let nodes = xml::all_elements(xml, "a:t");
    let non_empty: Vec<Elem> = nodes
        .iter()
        .copied()
        .filter(|e| !e.inner(xml, "a:t").trim().is_empty())
        .collect();

    let mut candidates: Vec<Elem> = non_empty
        .iter()
        .copied()
        .filter(|e| e.inner(xml, "a:t").trim().chars().count() > 15)
        .take(texts.len())
        .collect();

    if candidates.len() != texts.len() {
        let n = texts.len();
        candidates = non_empty
            .iter()
            .rev()
            .take(n)
            .rev()
            .copied()
            .collect();
    }
    if candidates.len() != texts.len() {
        bail!(
            "need {} text node(s), found {}",
            texts.len(),
            candidates.len()
        );
    }

    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0usize;
    for (el, text) in candidates.iter().zip(texts.iter()) {
        out.push_str(&xml[cursor..el.start]);
        out.push_str(&xml::build_t(text));
        cursor = el.end;
    }
    out.push_str(&xml[cursor..]);
    Ok(out)
}

/// Overwrite the first `<a:t>` node whose trimmed text is short (`< 40`
/// chars) and contains `:` — the diagram-footer citation slot (e.g. Para
/// Memorizar's `2 Reyes 4:21`).
pub fn set_diagram_citation(data_path: &Path, cite: &str) -> Result<()> {
    let xml = fs::read_to_string(data_path).with_context(|| format!("read {}", data_path.display()))?;
    let nodes = xml::all_elements(&xml, "a:t");
    let target = nodes.into_iter().find(|e| {
        let t = e.inner(&xml, "a:t");
        let s = t.trim();
        !s.is_empty() && s.chars().count() < 40 && s.contains(':')
    });
    let Some(el) = target else { return Ok(()) };
    let new_xml = format!(
        "{}{}{}",
        &xml[..el.start],
        xml::build_t(cite),
        &xml[el.end..]
    );
    fs::write(data_path, new_xml).with_context(|| format!("write {}", data_path.display()))
}

// ---------------------------------------------------------------------------
// Section images
// ---------------------------------------------------------------------------

/// Copy the 3 section-image PNGs into `ppt/media/image4.png..image6.png`.
pub fn replace_section_images(build: &Path, images: &[PathBuf]) -> Result<()> {
    if images.len() != 3 {
        bail!("exactly 3 section images required, got {}", images.len());
    }
    for (i, img) in images.iter().enumerate() {
        let dest = build
            .join("ppt")
            .join("media")
            .join(format!("image{}.png", i + 4));
        if !img.is_file() {
            bail!("image not found: {}", img.display());
        }
        fs::copy(img, &dest)
            .with_context(|| format!("copy {} -> {}", img.display(), dest.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_verse_ok() {
        let (n, b) = split_verse("18 Y el niño creció.").unwrap();
        assert_eq!(n, "18");
        assert_eq!(b, " Y el niño creció.");
    }

    #[test]
    fn set_simple_text_roundtrip() {
        let shape = r#"<p:sp><p:nvSpPr><p:cNvPr id="9" name="TextBox 4"/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="es-DO" sz="5000" b="1"><a:latin typeface="Gill Sans MT"/></a:rPr><a:t>16</a:t></a:r></a:p></p:txBody></p:sp>"#;
        let out = set_simple_text_block(shape, "17", Some(true)).unwrap();
        assert!(out.contains("<a:t>17</a:t>"));
        assert!(out.contains(r#"b="1""#));
        assert!(out.contains("Gill Sans MT"));
    }

    #[test]
    fn shape_block_finds_named_shape() {
        let slide = r#"<p:spTree><p:sp><p:nvSpPr><p:cNvPr id="1" name="A"/></p:nvSpPr><p:txBody><a:p><a:r><a:t>x</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="2" name="B"/></p:nvSpPr><p:txBody><a:p><a:r><a:t>y</a:t></a:r></a:p></p:txBody></p:sp></p:spTree>"#;
        let (s, e) = shape_block(slide, "B").unwrap();
        assert!(slide[s..e].contains(r#"name="B""#));
        assert!(!slide[s..e].contains(r#"name="A""#));
    }

    #[test]
    fn strip_arc_removes_only_arc_shapes() {
        let slide = r#"<p:spTree><p:sp><p:nvSpPr><p:cNvPr id="1" name="Arc 3421"/></p:nvSpPr><p:txBody><a:p/></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="2" name="Keep"/></p:nvSpPr><p:txBody><a:p/></p:txBody></p:sp></p:spTree>"#;
        let out = strip_arc_shapes(slide);
        assert!(!out.contains("Arc 3421"));
        assert!(out.contains("Keep"));
    }

    #[test]
    fn run_color_inserted_before_latin() {
        let children = r#"<a:latin typeface="Calibri"/>"#;
        let out = xml::set_run_color(children, Some("FF0000"));
        let fill_pos = out.find("solidFill").unwrap();
        let latin_pos = out.find("a:latin").unwrap();
        assert!(fill_pos < latin_pos, "solidFill must precede latin: {out}");
    }

    #[test]
    fn run_color_ignores_ln_nested_fill() {
        let children = r#"<a:ln><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln><a:solidFill><a:srgbClr val="C32D2E"/></a:solidFill><a:latin typeface="Gill Sans MT"/>"#;
        let out = xml::set_run_color(children, Some("FFFFFF"));
        // Only the direct-child solidFill (the second one) should be swapped;
        // the a:ln outline color must be untouched.
        assert!(out.contains(r#"val="000000""#), "outline color must survive: {out}");
        assert!(out.contains(r#"val="FFFFFF""#));
        assert!(!out.contains(r#"val="C32D2E""#));
    }
}
