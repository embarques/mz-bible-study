//! Adult-template OOXML fill helpers (shape names differ from youth).
//!
//! Fills preserve the corrected `master-template.pptx` run colours,
//! paragraph breaks, and layout geometry — only text is swapped.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::build::ooxml::body::{set_content_body_teaching, strip_arc_shapes};
use crate::build::ooxml::shape::{
    replace_text_preserving_runs, replace_text_single_run, set_simple_text_block, transform_shape,
};
use crate::build::ooxml::title::set_adult_title_slide;
use crate::paths;
use crate::Audience;

/// Soft cap per body paragraph on adult `Marcador de contenido 2` shapes.
/// See `LAYOUT_GUIDE.md` — navy footer bar is ~bottom 8–10% of slide;
/// content must stop above it (~360–400 chars per paragraph at ~42pt).
pub const ADULT_BODY_PARA_BUDGET: usize = 400;

#[derive(Debug, Clone, Copy)]
pub enum AbRefShape {
    CuadroTexto6,
    CuadroTexto4,
    Titulo6,
    Titulo7,
}

impl AbRefShape {
    pub fn name(self) -> &'static str {
        match self {
            Self::CuadroTexto6 => "CuadroTexto 6",
            Self::CuadroTexto4 => "CuadroTexto 4",
            Self::Titulo6 => "Título 6",
            Self::Titulo7 => "Título 7",
        }
    }
}

pub fn set_title_or_proximo(
    path: &Path,
    numero: &str,
    titulo: &str,
    base: &[String],
) -> Result<()> {
    set_adult_title_slide(path, numero, titulo, base)
}

fn fill_shape_single(path: &Path, shape: &str, text: &str) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape, |b| replace_text_single_run(b, text))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

fn fill_shape_preserving(path: &Path, shape: &str, text: &str) -> Result<()> {
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, shape, |b| replace_text_preserving_runs(b, text))?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))
}

pub fn set_ensenanza_datos(
    path: &Path,
    ensenanza: &str,
    datos: &crate::model::adult_study::DatosGenerales,
) -> Result<()> {
    fill_shape_preserving(path, "CuadroTexto 13", ensenanza)?;
    fill_shape_preserving(path, "CuadroTexto 9", &datos.personajes)?;
    fill_shape_preserving(path, "CuadroTexto 21", &datos.fecha)?;
    fill_shape_preserving(path, "CuadroTexto 32", &datos.lugar)?;
    fill_shape_preserving(path, "CuadroTexto 38", &datos.autor)?;
    Ok(())
}

/// TEMA image header — same chrome as youth section slides (orange bar,
/// title, gray line, verse), anchored at the **bottom-left** so longer
/// adult titles have room without covering the scene.
pub fn set_tema_header_image(path: &Path, tema_label: &str, titulo: &str, rango: &str) -> Result<()> {
    let title = format!("{tema_label}- {titulo}");
    // Adult image + Texto Bíblico citations never use parentheses.
    let cite = strip_citation_parens(rango);
    rewrite_as_youth_section_bottom(path, &title, &cite)
}

/// A/B title image slide — youth section chrome at bottom-left.
pub fn set_ab_title_header(
    path: &Path,
    point_label: &str,
    titulo: &str,
    cita: &str,
    _reference: AbRefShape,
) -> Result<()> {
    let title = format!("{point_label}- {titulo}");
    let cite = strip_citation_parens(cita);
    rewrite_as_youth_section_bottom(path, &title, &cite)
}

/// `MATEO 6:1-4` — never `(MATEO 6:1-4)` on image / Texto Bíblico chrome.
fn strip_citation_parens(cita: &str) -> String {
    cita.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim()
        .to_string()
}

/// Adult image slide = **exact youth section slide** (master slide 8), with
/// chrome shifted to the bottom for longer titles. Do not invent OOXML —
/// only: clone youth XML → strip Arc → move Y → swap text → keep image rid.
/// Scenic media was already written into that rid by `apply_scenic_image`.
fn rewrite_as_youth_section_bottom(path: &Path, title: &str, verse: &str) -> Result<()> {
    let current = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let embed = Regex::new(r#"r:embed="(rId\d+)""#)
        .unwrap()
        .captures(&current)
        .map(|c| c[1].to_string())
        .context("image slide has no r:embed to keep for youth-style rewrite")?;

    let mut xml = load_youth_section_slide_xml()?;
    xml = strip_arc_shapes(&xml);
    // Drop Office 2014+ ext chrome (creationId / decorative / designElem).
    // Leaving ns2/ns3/ns4 in an adult package gets remapped to a16/x3/x4 and
    // triggers PowerPoint "Repair" for some users — core p/a/r is enough.
    xml = strip_office_ext_cruft(&xml);
    // Youth section slides ship a full-bleed black `!!Rectangle` under the
    // photo. After we send the pic to back, that rect paints *over* the
    // scenic image → solid black slide. Remove it.
    xml = strip_named_shape(&xml, "!!Rectangle");

    // Same youth gaps; whole stack anchored above a 0.25" bottom margin.
    const SLIDE_H: i64 = 6_858_000;
    const MARGIN_BOTTOM: i64 = 228_600;
    const ORANGE_Y0: i64 = 402_336;
    const TITLE_Y0: i64 = 640_080;
    const GRAY_Y0: i64 = 2_862_072;
    const VERSE_Y0: i64 = 3_026_664;
    const VERSE_H: i64 = 1_463_040;
    let dy = SLIDE_H - MARGIN_BOTTOM - ((VERSE_Y0 - ORANGE_Y0) + VERSE_H) - ORANGE_Y0;

    xml = shift_shape_y(&xml, "OrangeBar", ORANGE_Y0 + dy)?;
    xml = shift_shape_y(&xml, "CuadroTexto 8", TITLE_Y0 + dy)?;
    xml = shift_shape_y(&xml, "GrayLine", GRAY_Y0 + dy)?;
    xml = shift_shape_y(&xml, "CuadroTexto 3", VERSE_Y0 + dy)?;

    // Point at this slide's media rid only — leave pic geometry/stretch alone.
    let blip_re = Regex::new(r#"r:embed="rId\d+""#).unwrap();
    xml = blip_re
        .replace(&xml, format!(r#"r:embed="{embed}""#).as_str())
        .into_owned();

    xml = transform_shape(&xml, "CuadroTexto 8", |b| {
        set_simple_text_block(b, title, Some(true))
    })?;
    xml = transform_shape(&xml, "CuadroTexto 3", |b| {
        set_simple_text_block(b, verse, Some(true))
    })?;

    // Long adult TEMA/A-B titles overflow the bottom-left panel and collide
    // with the verse under the gray line — shrink until they fit.
    let title_sz = title_font_sz_hundredths(title);
    xml = replace_first_run_sz(&xml, "CuadroTexto 8", title_sz);

    fs::write(path, xml).with_context(|| format!("write {}", path.display()))?;
    // Youth slide8 ships empty `<a:stretch />` + srcRect. That is a schema
    // problem for PowerPoint on this adult package: Repair → "couldn't read
    // some content … removed it". Normalize to one `<a:stretch><a:fillRect/>`
    // full-bleed pic (same as apply_scenic_image), then colour chrome only.
    force_pics_fullbleed(path)?;
    let tone = crate::build::apply_image_chrome_contrast(path)?;
    eprintln!(
        "  contrast {}: {:?} title_sz={}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
        tone,
        title_sz
    );
    Ok(())
}

/// Hundredths of a point for bottom-left image titles. Longer titles need
/// smaller type so they stay above the gray line / verse.
fn title_font_sz_hundredths(title: &str) -> u32 {
    let n = title.chars().count();
    if n > 70 {
        1800 // 18pt
    } else if n > 55 {
        2000
    } else if n > 42 {
        2400
    } else if n > 32 {
        2800
    } else {
        3200
    }
}

fn replace_first_run_sz(xml: &str, shape_name: &str, sz: u32) -> String {
    let marker = format!(r#"name="{shape_name}""#);
    let Some(pos) = xml.find(&marker) else {
        return xml.to_string();
    };
    let after = &xml[pos..];
    let Some(sz_rel) = after.find(r#"sz=""#) else {
        return xml.to_string();
    };
    let abs = pos + sz_rel;
    let rest = &xml[abs + 4..]; // after sz="
    let Some(end_q) = rest.find('"') else {
        return xml.to_string();
    };
    format!("{}{}{}", &xml[..abs + 4], sz, &rest[end_q..])
}

/// Remove the first `<p:sp>…</p:sp>` whose `cNvPr` name matches.
fn strip_named_shape(xml: &str, name: &str) -> String {
    let marker = format!(r#"name="{name}""#);
    let Some(name_pos) = xml.find(&marker) else {
        return xml.to_string();
    };
    let Some(sp_start) = xml[..name_pos].rfind("<p:sp") else {
        return xml.to_string();
    };
    let after = &xml[sp_start..];
    let Some(end_rel) = after.find("</p:sp>") else {
        return xml.to_string();
    };
    let sp_end = sp_start + end_rel + "</p:sp>".len();
    format!("{}{}", &xml[..sp_start], &xml[sp_end..])
}

/// Remove non-essential `a:extLst` / decorative markers and unused xmlns:ns*
/// declarations from a cloned youth section slide.
fn strip_office_ext_cruft(xml: &str) -> String {
    let mut s = xml.to_string();
    // Whole extLst blocks (creationId, decorative, designElem live here).
    let ext_re = Regex::new(r#"<a:extLst>[\s\S]*?</a:extLst>"#).unwrap();
    s = ext_re.replace_all(&s, "").into_owned();
    let p_ext_re = Regex::new(r#"<p:extLst>[\s\S]*?</p:extLst>"#).unwrap();
    s = p_ext_re.replace_all(&s, "").into_owned();
    // Unused youth-only namespace decls.
    for decl in [
        r#" xmlns:ns2="http://schemas.microsoft.com/office/drawing/2014/main""#,
        r#" xmlns:ns3="http://schemas.microsoft.com/office/drawing/2017/decorative""#,
        r#" xmlns:ns4="http://schemas.microsoft.com/office/powerpoint/2015/main""#,
        r#" xmlns:a16="http://schemas.microsoft.com/office/drawing/2014/main""#,
        r#" xmlns:x3="http://schemas.microsoft.com/office/drawing/2017/decorative""#,
        r#" xmlns:x4="http://schemas.microsoft.com/office/powerpoint/2015/main""#,
    ] {
        s = s.replace(decl, "");
    }
    s
}

fn load_youth_section_slide_xml() -> Result<String> {
    let pptx = paths::master_template(Audience::Youth)?;
    let file = fs::File::open(&pptx).with_context(|| format!("open {}", pptx.display()))?;
    let mut zip = zip::ZipArchive::new(file).with_context(|| format!("zip {}", pptx.display()))?;
    // Youth section prototypes are slides 8 / 12 / 16 — identical chrome.
    let mut entry = zip
        .by_name("ppt/slides/slide8.xml")
        .context("youth master-template missing ppt/slides/slide8.xml")?;
    let mut buf = String::new();
    use std::io::Read;
    entry
        .read_to_string(&mut buf)
        .context("read youth section slide8.xml")?;
    Ok(buf)
}

/// Set the first `<a:off … y="…"/>` after a shape's `name="…"`.
fn shift_shape_y(xml: &str, name: &str, new_y: i64) -> Result<String> {
    let marker = format!(r#"name="{name}""#);
    let Some(name_pos) = xml.find(&marker) else {
        bail!("shape {name} not found while shifting youth chrome");
    };
    let after = &xml[name_pos..];
    let Some(off_rel) = after.find("<a:off ") else {
        bail!("shape {name} has no <a:off>");
    };
    let off_start = name_pos + off_rel;
    let rest = &xml[off_start..];
    let Some(end_rel) = rest.find("/>") else {
        bail!("shape {name} <a:off> not self-closing");
    };
    let off_end = off_start + end_rel + 2;
    let off_tag = &xml[off_start..off_end];
    let x_re = Regex::new(r#"x="(\d+)""#).unwrap();
    let x = x_re
        .captures(off_tag)
        .map(|c| c[1].to_string())
        .context("shape off missing x")?;
    let new_off = format!(r#"<a:off x="{x}" y="{new_y}"/>"#);
    Ok(format!("{}{}{}", &xml[..off_start], new_off, &xml[off_end..]))
}

/// Replace a video-poster tema header (template slides 29 / 44) with the
/// clean still-image tema layout from slide 14. Half-stripping `p:video` /
/// `p14:media` leaves PowerPoint repair dialogs; cloning the known-good
/// image slide does not.
pub fn convert_video_tema_to_image_layout(
    build: &Path,
    slide_num: u32,
    proto: u32,
) -> Result<()> {
    let slide = crate::build::ooxml::slide_path(build, slide_num);
    let proto_slide = crate::build::ooxml::slide_path(build, proto);
    let rels_dir = build.join("ppt").join("slides").join("_rels");
    let rels_path = rels_dir.join(format!("slide{slide_num}.xml.rels"));
    let proto_rels_path = rels_dir.join(format!("slide{proto}.xml.rels"));

    let rels = if rels_path.is_file() {
        fs::read_to_string(&rels_path).with_context(|| format!("read {}", rels_path.display()))?
    } else {
        String::new()
    };

    // Keep the existing poster/image media file when present.
    let img_re = Regex::new(
        r#"Type="[^"]*/relationships/image"[^>]*Target="\.\./media/([^"]+)""#,
    )
    .unwrap();
    let media_name = if let Some(caps) = img_re.captures(&rels) {
        caps[1].to_string()
    } else {
        // Fallback: copy proto's media file under a new name.
        let proto_rels = fs::read_to_string(&proto_rels_path)
            .with_context(|| format!("read {}", proto_rels_path.display()))?;
        let proto_media = img_re
            .captures(&proto_rels)
            .map(|c| c[1].to_string())
            .context("tema image prototype has no image relationship")?;
        let media_dir = build.join("ppt").join("media");
        let new_name = next_media_png_name(&media_dir)?;
        fs::copy(media_dir.join(&proto_media), media_dir.join(&new_name)).with_context(|| {
            format!("copy proto media {} -> {}", proto_media, new_name)
        })?;
        new_name
    };

    let layout_re = Regex::new(
        r#"Type="[^"]*/relationships/slideLayout"[^>]*Target="([^"]+)""#,
    )
    .unwrap();
    let layout_target = if let Some(caps) = layout_re.captures(&rels) {
        caps[1].to_string()
    } else if proto_rels_path.is_file() {
        let proto_rels = fs::read_to_string(&proto_rels_path)
            .with_context(|| format!("read {}", proto_rels_path.display()))?;
        layout_re
            .captures(&proto_rels)
            .map(|c| c[1].to_string())
            .unwrap_or_else(|| "../slideLayouts/slideLayout2.xml".to_string())
    } else {
        "../slideLayouts/slideLayout2.xml".to_string()
    };

    // Image tema layout uses rId1=layout, rId3=image (matches slide 14).
    let new_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="{layout_target}"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/{media_name}"/></Relationships>"#
    );
    fs::copy(&proto_slide, &slide)
        .with_context(|| format!("copy {} -> {}", proto_slide.display(), slide.display()))?;
    fs::write(&rels_path, new_rels).with_context(|| format!("write {}", rels_path.display()))?;
    Ok(())
}

/// Delete unreferenced `ppt/media/*.mp4` left after converting video temas
/// to stills (orphans can confuse PowerPoint repair).
pub fn remove_orphan_videos(build: &Path) -> Result<()> {
    let media_dir = build.join("ppt").join("media");
    if !media_dir.is_dir() {
        return Ok(());
    }
    let mut referenced = std::collections::HashSet::new();
    let rels_root = build.join("ppt");
    fn walk_rels(dir: &Path, referenced: &mut std::collections::HashSet<String>) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk_rels(&path, referenced)?;
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.ends_with(".rels") {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            for caps in Regex::new(r#"Target="[^"]*/media/([^"]+\.mp4)""#)
                .unwrap()
                .captures_iter(&text)
            {
                referenced.insert(caps[1].to_string());
            }
        }
        Ok(())
    }
    walk_rels(&rels_root, &mut referenced)?;
    for entry in fs::read_dir(&media_dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.ends_with(".mp4") && !referenced.contains(name) {
            fs::remove_file(&path)
                .with_context(|| format!("remove orphan video {}", path.display()))?;
        }
    }
    Ok(())
}

pub fn set_ab_body_slide(path: &Path, point_title: &str, body: &str) -> Result<()> {
    set_ab_body_title(path, point_title)?;
    warn_if_over_budget(body);
    set_content_body_teaching(path, body, None)
}

/// A/B body header (`Título 1`). Gold uses `anchor="b"` + a soft `<a:br>` that
/// our leading-run fill turned into a lone `1.B -` on line 1 and the full
/// title on line 2 — bottom-anchored overflow then paints **above** the slide.
/// Put the whole title in one flow, top-anchor, and shrink font until it fits.
fn set_ab_body_title(path: &Path, title: &str) -> Result<()> {
    let sz = ab_body_title_font_sz(title);
    let xml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let xml = transform_shape(&xml, "Título 1", |b| {
        let mut s = set_simple_text_block(b, title, Some(true))?;
        // Grow downward if anything wraps — never off the top of the slide.
        s = s.replacen(r#"anchor="b""#, r#"anchor="t""#, 1);
        s = force_all_run_sz(&s, sz);
        Ok(s)
    })?;
    let xml = deepen_title_box_if_needed(&xml, "Título 1", title, sz)?;
    fs::write(path, xml).with_context(|| format!("write {}", path.display()))?;
    eprintln!(
        "  ab-title {}: sz={} ({})",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
        sz,
        title.chars().count()
    );
    Ok(())
}

/// Hundredths of a point — step down when the header would overflow.
fn ab_body_title_font_sz(title: &str) -> u32 {
    let n = title.chars().count();
    if n > 72 {
        2200 // 22pt
    } else if n > 58 {
        2600
    } else if n > 48 {
        3000
    } else if n > 36 {
        3400
    } else {
        3600
    }
}

fn force_all_run_sz(shape_xml: &str, sz: u32) -> String {
    // Only visible text runs — never rewrite endParaRPr (self-closing form
    // is easy to corrupt and breaks XML validation).
    let re = Regex::new(r#"<a:rPr\b[^>]*/?>"#).unwrap();
    re.replace_all(shape_xml, |caps: &regex::Captures| {
        let tag = caps[0].to_string();
        let self_close = tag.ends_with("/>");
        let inner = tag
            .trim_start_matches("<a:rPr")
            .trim_end_matches("/>")
            .trim_end_matches('>')
            .to_string();
        let attrs = if inner.contains("sz=\"") {
            Regex::new(r#"sz="\d+""#)
                .unwrap()
                .replace_all(&inner, format!(r#"sz="{sz}""#).as_str())
                .into_owned()
        } else {
            format!(r#"{inner} sz="{sz}""#)
        };
        if self_close {
            format!("<a:rPr{attrs}/>")
        } else {
            format!("<a:rPr{attrs}>")
        }
    })
    .into_owned()
}

/// Long two-line headers need a taller `Título 1` box so top-anchored text
/// does not collide with the body.
fn deepen_title_box_if_needed(
    xml: &str,
    shape: &str,
    title: &str,
    sz: u32,
) -> Result<String> {
    let n = title.chars().count();
    // Rough: >48 chars or ≤28pt usually wraps to 2 lines on the wide header.
    if n <= 48 && sz >= 3400 {
        return Ok(xml.to_string());
    }
    let marker = format!(r#"name="{shape}""#);
    let Some(name_pos) = xml.find(&marker) else {
        return Ok(xml.to_string());
    };
    // Stay inside this shape: from name → closing </p:sp>.
    let shape_end = xml[name_pos..]
        .find("</p:sp>")
        .map(|i| name_pos + i)
        .unwrap_or(xml.len());
    let shape_xml = &xml[name_pos..shape_end];
    // Prefer the spPr xfrm ext (shape frame), not nested text xfrm.
    let ext_re = Regex::new(r#"<a:ext cx="(\d+)" cy="(\d+)"\s*/>"#).unwrap();
    let Some(caps) = ext_re.captures(shape_xml) else {
        return Ok(xml.to_string());
    };
    let cx = &caps[1];
    let cy: i64 = caps[2].parse().unwrap_or(679_677);
    let new_cy = (cy + 280_000).min(1_100_000);
    let old_ext = caps.get(0).unwrap().as_str();
    let new_ext = format!(r#"<a:ext cx="{cx}" cy="{new_cy}"/>"#);
    let abs_start = name_pos + caps.get(0).unwrap().start();
    let abs_end = abs_start + old_ext.len();
    Ok(format!(
        "{}{}{}",
        &xml[..abs_start],
        new_ext,
        &xml[abs_end..]
    ))
}

pub fn set_definicion(path: &Path, text: &str) -> Result<()> {
    warn_if_over_budget(text);
    set_content_body_teaching(path, text, None)
}

/// Replace a definición slide with a full-bleed composed card PNG
/// (DEFINICIÓN Y ETIMOLOGÍA design). Clears old text chrome so it does
/// not double-render over the art.
pub fn set_definicion_image(build: &Path, slide_num: u32, image: &Path) -> Result<()> {
    if !image.is_file() {
        anyhow::bail!("definicion image not found: {}", image.display());
    }
    let media_dir = build.join("ppt").join("media");
    std::fs::create_dir_all(&media_dir)
        .with_context(|| format!("mkdir {}", media_dir.display()))?;

    let media_name = next_media_png_name(&media_dir)?;
    let dest = media_dir.join(&media_name);
    let raw = fs::read(image).with_context(|| format!("read {}", image.display()))?;
    // Full-width, top-aligned — never center-crop (that cuts the title off).
    let png = crate::build::ooxml::resize_definicion_png(&raw)
        .with_context(|| format!("resize definicion {}", image.display()))?;
    fs::write(&dest, png).with_context(|| format!("write {}", dest.display()))?;

    let slide = crate::build::ooxml::slide_path(build, slide_num);
    let xml = fs::read_to_string(&slide).with_context(|| format!("read {}", slide.display()))?;
    // PowerPoint is picky about relationship Ids — use numeric `rIdN` only
    // (never `rIdDefImg`); that form has triggered repair dialogs.
    let embed_rid = next_numeric_rid(&xml, &{
        let rels_path = build
            .join("ppt")
            .join("slides")
            .join("_rels")
            .join(format!("slide{slide_num}.xml.rels"));
        if rels_path.is_file() {
            fs::read_to_string(&rels_path).unwrap_or_default()
        } else {
            String::new()
        }
    });
    let new_xml = rewrite_slide_as_fullbleed_pic(&xml, &embed_rid)?;
    fs::write(&slide, new_xml).with_context(|| format!("write {}", slide.display()))?;

    let rels_path = build
        .join("ppt")
        .join("slides")
        .join("_rels")
        .join(format!("slide{slide_num}.xml.rels"));
    let mut rels = if rels_path.is_file() {
        fs::read_to_string(&rels_path).with_context(|| format!("read {}", rels_path.display()))?
    } else {
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#
            .to_string()
    };
    // Drop prior image relationships; keep layout (+ notes if present).
    let img_re = regex::Regex::new(
        r#"<Relationship\b[^>]*Type="[^"]*/relationships/image"[^>]*/>\s*"#,
    )
    .unwrap();
    rels = img_re.replace_all(&rels, "").into_owned();
    let rel_tag = format!(
        r#"<Relationship Id="{embed_rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/{media_name}"/>"#
    );
    if !rels.contains(&embed_rid) {
        rels = rels.replacen("</Relationships>", &format!("{rel_tag}</Relationships>"), 1);
    }
    fs::write(&rels_path, rels).with_context(|| format!("write {}", rels_path.display()))?;
    Ok(())
}

fn next_numeric_rid(slide_xml: &str, rels: &str) -> String {
    let re = Regex::new(r#"\brId(\d+)\b"#).unwrap();
    let mut max_n = 0u32;
    for caps in re.captures_iter(slide_xml).chain(re.captures_iter(rels)) {
        if let Ok(n) = caps[1].parse::<u32>() {
            max_n = max_n.max(n);
        }
    }
    format!("rId{}", max_n + 1)
}

fn next_media_png_name(media_dir: &Path) -> Result<String> {
    let mut max_n = 0u32;
    if media_dir.is_dir() {
        for entry in fs::read_dir(media_dir).with_context(|| format!("read_dir {}", media_dir.display()))? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(s) = name.to_str() else { continue };
            if let Some(rest) = s.strip_prefix("image") {
                if let Some(num) = rest.strip_suffix(".png") {
                    if let Ok(n) = num.parse::<u32>() {
                        max_n = max_n.max(n);
                    }
                }
            }
        }
    }
    Ok(format!("image{}.png", max_n + 1))
}

fn rewrite_slide_as_fullbleed_pic(slide_xml: &str, embed_rid: &str) -> Result<String> {
    // Keep the opening <p:sld ...> through nvGrpSpPr / grpSpPr, then a single
    // full-bleed pic, then the original trailer after </p:spTree>.
    let tree_start = slide_xml
        .find("<p:spTree>")
        .context("slide missing <p:spTree>")?;
    let tree_end = slide_xml
        .find("</p:spTree>")
        .context("slide missing </p:spTree>")?
        + "</p:spTree>".len();

    let head = &slide_xml[..tree_start];
    let tree = &slide_xml[tree_start..tree_end];
    let tail = &slide_xml[tree_end..];

    // Preserve group shape preamble (required by PPTX), excluding the
    // outer <p:spTree> tag which we re-open below.
    let grp_end = tree
        .find("</p:grpSpPr>")
        .context("slide missing </p:grpSpPr>")?
        + "</p:grpSpPr>".len();
    let preamble = tree
        .strip_prefix("<p:spTree>")
        .map(|s| &s[..grp_end - "<p:spTree>".len()])
        .context("spTree preamble strip failed")?;

    // EMUs for 13.333" × 7.5" (standard 16:9 widescreen).
    let pic = format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="2" name="DefinicionCard"/><p:cNvPicPr><a:picLocks noChangeAspect="0"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{embed_rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="12192000" cy="6858000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#
    );

    Ok(format!("{head}<p:spTree>{preamble}{pic}</p:spTree>{tail}"))
}

pub fn set_intro_body(path: &Path, text: &str) -> Result<()> {
    warn_if_over_budget(text);
    set_content_body_teaching(path, text, None)
}

/// Youth intro body slides include the Mount Zion logo at bottom-right
/// (`Imagen 8`). Adult intro body prototypes omit it — inject the same
/// geometry for intro slides only (not A/B / conclusión).
pub fn ensure_intro_body_logo(build: &Path, slide_num: u32, logo_png: &Path) -> Result<()> {
    if !logo_png.is_file() {
        bail!("intro logo not found: {}", logo_png.display());
    }
    let slide = crate::build::ooxml::slide_path(build, slide_num);
    let mut xml = fs::read_to_string(&slide).with_context(|| format!("read {}", slide.display()))?;
    if xml.contains("name=\"Imagen 8\"") || xml.contains("name=\"MzLogo\"") {
        return Ok(());
    }

    let media_dir = build.join("ppt").join("media");
    fs::create_dir_all(&media_dir)?;
    // Stable media name so every intro body slide can share one file.
    let media_name = "mzLogo.png";
    let dest = media_dir.join(media_name);
    if !dest.is_file() {
        let raw = fs::read(logo_png).with_context(|| format!("read {}", logo_png.display()))?;
        fs::write(&dest, raw).with_context(|| format!("write {}", dest.display()))?;
    }

    let rels_path = build
        .join("ppt")
        .join("slides")
        .join("_rels")
        .join(format!("slide{slide_num}.xml.rels"));
    let mut rels = if rels_path.is_file() {
        fs::read_to_string(&rels_path).with_context(|| format!("read {}", rels_path.display()))?
    } else {
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#
            .to_string()
    };

    // Reuse existing image rid pointing at mzLogo.png, or allocate next numeric.
    let logo_rid = if let Some(caps) = Regex::new(
        r#"Id="(rId\d+)"[^>]*Target="\.\./media/mzLogo\.png""#,
    )
    .unwrap()
    .captures(&rels)
    {
        caps[1].to_string()
    } else if let Some(caps) = Regex::new(
        r#"Target="\.\./media/mzLogo\.png"[^>]*Id="(rId\d+)""#,
    )
    .unwrap()
    .captures(&rels)
    {
        caps[1].to_string()
    } else {
        let rid = next_numeric_rid(&xml, &rels);
        let tag = format!(
            r#"<Relationship Id="{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/{media_name}"/>"#
        );
        rels = rels.replacen("</Relationships>", &format!("{tag}</Relationships>"), 1);
        fs::write(&rels_path, &rels).with_context(|| format!("write {}", rels_path.display()))?;
        rid
    };

    // Next free shape id on the slide.
    let id_re = Regex::new(r#"<p:cNvPr id="(\d+)""#).unwrap();
    let mut max_id = 1u32;
    for caps in id_re.captures_iter(&xml) {
        if let Ok(n) = caps[1].parse::<u32>() {
            max_id = max_id.max(n);
        }
    }
    let shape_id = max_id + 1;

    // Geometry copied from youth master-template intro body (`Imagen 8`).
    let pic = format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="{shape_id}" name="Imagen 8"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{logo_rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="11281505" y="6196152"/><a:ext cx="815245" cy="582318"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#
    );
    if !xml.contains("</p:spTree>") {
        bail!("slide {slide_num} missing </p:spTree>");
    }
    xml = xml.replacen("</p:spTree>", &format!("{pic}</p:spTree>"), 1);
    fs::write(&slide, xml).with_context(|| format!("write {}", slide.display()))?;
    if !rels_path.is_file() {
        fs::write(&rels_path, &rels).with_context(|| format!("write {}", rels_path.display()))?;
    }
    Ok(())
}

/// Give a `<p:pic>` exactly one `<a:stretch><a:fillRect/></a:stretch>` fill
/// mode, returning `None` when nothing had to change.
///
/// `CT_BlipFillProperties` is `a:blip? a:srcRect? (a:tile | a:stretch)?`, so a
/// second `<a:stretch>` sibling is a schema violation. PowerPoint rejects the
/// whole slide with "found a problem with content … Repair", and after
/// repairing it reports that content was removed. The template ships the
/// self-closing `<a:stretch/>` form, which a naive `contains("<a:stretch>")`
/// guard misses.
fn ensure_single_stretch(pic: &str) -> Option<String> {
    const STRETCH: &str = "<a:stretch><a:fillRect/></a:stretch>";
    let empty_re = Regex::new(r#"<a:stretch\s*/>"#).unwrap();
    let any_re = Regex::new(r#"<a:stretch[\s/>]"#).unwrap();

    // Collapse any duplicates the old code may have produced, then rewrite the
    // self-closing form into the explicit fillRect form.
    if any_re.is_match(pic) {
        let mut out = pic.to_string();
        if empty_re.is_match(&out) {
            // Drop every empty stretch; a full one is re-inserted below when
            // no explicit `<a:stretch>…</a:stretch>` survives.
            out = empty_re.replace_all(&out, "").into_owned();
        }
        if !out.contains("<a:stretch>") {
            out = insert_stretch(&out, STRETCH)?;
        }
        return if out == pic { None } else { Some(out) };
    }

    insert_stretch(pic, STRETCH)
}

/// Insert `stretch` as the last child of the pic's `<p:blipFill>`.
fn insert_stretch(pic: &str, stretch: &str) -> Option<String> {
    let close = pic.find("</p:blipFill>")?;
    Some(format!("{}{}{}", &pic[..close], stretch, &pic[close..]))
}

/// Force every `<p:pic>` on the slide to true full-bleed (edge-to-edge
/// 16:9) and strip `srcRect` crops. Matches the adult 2.A layout the user
/// approved — image fills the whole slide; text chrome sits on top.
pub fn force_pics_fullbleed(slide_path: &Path) -> Result<()> {
    let xml = fs::read_to_string(slide_path)
        .with_context(|| format!("read {}", slide_path.display()))?;
    // Simpler per-pic rewrite:
    let mut out = String::new();
    let mut rest = xml.as_str();
    let mut changed = false;
    while let Some(start) = rest.find("<p:pic") {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let end_rel = after
            .find("</p:pic>")
            .context("unclosed <p:pic>")?
            + "</p:pic>".len();
        let mut pic = after[..end_rel].to_string();
        // Remove srcRect crops that letterbox/zoom the photo.
        let src_re = Regex::new(r#"<a:srcRect\b[^/]*/>"#).unwrap();
        pic = src_re.replace_all(&pic, "").into_owned();
        // Force xfrm off/ext inside p:spPr (last xfrm in pic is the frame).
        if let Some(xfrm_start) = pic.rfind("<a:xfrm>") {
            if let Some(xfrm_end_rel) = pic[xfrm_start..].find("</a:xfrm>") {
                let xfrm_end = xfrm_start + xfrm_end_rel + "</a:xfrm>".len();
                let new_xfrm = r#"<a:xfrm><a:off x="0" y="0"/><a:ext cx="12192000" cy="6858000"/></a:xfrm>"#;
                pic = format!("{}{}{}", &pic[..xfrm_start], new_xfrm, &pic[xfrm_end..]);
                changed = true;
            }
        }
        // Ensure stretch fillRect exists. `CT_BlipFillProperties` permits at
        // most one fill-mode child, so an existing `<a:stretch/>` must be
        // rewritten in place instead of having a second one appended.
        if let Some(normalized) = ensure_single_stretch(&pic) {
            pic = normalized;
            changed = true;
        }
        out.push_str(&pic);
        rest = &after[end_rel..];
    }
    out.push_str(rest);
    if changed {
        fs::write(slide_path, &out).with_context(|| format!("write {}", slide_path.display()))?;
    }
    // Full-bleed pics that sit *after* title shapes in the tree paint over
    // the headings (TEMA / 1.A / …). Send scenic pics behind chrome.
    send_fullbleed_pics_to_back(slide_path)?;
    Ok(())
}

/// Move full-bleed scenic `<p:pic>` elements to just after `</p:grpSpPr>`
/// so title/chrome shapes paint on top. Skips footer logos (`Imagen 8`).
fn send_fullbleed_pics_to_back(slide_path: &Path) -> Result<()> {
    let xml = fs::read_to_string(slide_path)
        .with_context(|| format!("read {}", slide_path.display()))?;
    let tree_start = match xml.find("<p:spTree>") {
        Some(i) => i,
        None => return Ok(()),
    };
    let tree_end = match xml.find("</p:spTree>") {
        Some(i) => i,
        None => return Ok(()),
    };
    let head = &xml[..tree_start];
    let tree = &xml[tree_start..tree_end];
    let tail = &xml[tree_end..];

    let grp_end_rel = match tree.find("</p:grpSpPr>") {
        Some(i) => i + "</p:grpSpPr>".len(),
        None => return Ok(()),
    };
    let preamble = &tree[..grp_end_rel];
    let body = &tree[grp_end_rel..];

    let mut pics = Vec::new();
    let mut other = String::new();
    let mut rest = body;
    let mut moved = false;
    while let Some(start) = rest.find("<p:pic") {
        other.push_str(&rest[..start]);
        let after = &rest[start..];
        let end_rel = after
            .find("</p:pic>")
            .context("unclosed <p:pic>")?
            + "</p:pic>".len();
        let pic = &after[..end_rel];
        let name = Regex::new(r#"name="([^"]*)""#)
            .unwrap()
            .captures(pic)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        let is_logo = name == "Imagen 8" || name == "MzLogo" || name.contains("Logo");
        let is_fullbleed = pic.contains(r#"x="0""#)
            && pic.contains(r#"y="0""#)
            && pic.contains(r#"cx="12192000""#)
            && pic.contains(r#"cy="6858000""#);
        if is_fullbleed && !is_logo {
            pics.push(pic.to_string());
            moved = true;
        } else {
            other.push_str(pic);
        }
        rest = &after[end_rel..];
    }
    other.push_str(rest);

    if !moved {
        return Ok(());
    }
    // `preamble` includes `<p:spTree>…</p:grpSpPr>`; `tail` starts at `</p:spTree>`.
    let new_xml = format!("{head}{preamble}{}{other}{tail}", pics.join(""));
    fs::write(slide_path, new_xml).with_context(|| format!("write {}", slide_path.display()))?;
    Ok(())
}

/// Replace the slide's scenic image media. When `fullbleed` is true (intro /
/// 2.A-style slides), force edge-to-edge geometry and put the pic behind
/// chrome. When false (TEMA header / inset 1.A), **keep template geometry**
/// so the top title bar stays visible and the photo sits in its frame.
pub fn apply_scenic_image(
    build: &Path,
    slide_num: u32,
    image: &Path,
    fullbleed: bool,
) -> Result<()> {
    if !image.is_file() {
        bail!("scenic image not found: {}", image.display());
    }
    let slide = crate::build::ooxml::slide_path(build, slide_num);
    let rels_path = build
        .join("ppt")
        .join("slides")
        .join("_rels")
        .join(format!("slide{slide_num}.xml.rels"));
    let mut rels = fs::read_to_string(&rels_path)
        .with_context(|| format!("read {}", rels_path.display()))?;

    // Drop video relationships (tema II/III templates).
    let video_re = Regex::new(
        r#"<Relationship\b[^>]*Type="[^"]*/relationships/(?:video|media)"[^>]*/>\s*"#,
    )
    .unwrap();
    rels = video_re.replace_all(&rels, "").into_owned();

    let img_re = Regex::new(
        r#"Type="[^"]*/relationships/image"[^>]*Target="\.\./media/([^"]+)""#,
    )
    .unwrap();
    let media_name = img_re
        .captures(&rels)
        .map(|c| c[1].to_string())
        .context("slide has no image relationship to replace")?;

    let dest = build.join("ppt").join("media").join(&media_name);
    let raw = fs::read(image).with_context(|| format!("read {}", image.display()))?;
    // Cover-crop for scenic art (text is OOXML chrome, not baked in).
    let png = crate::build::ooxml::resize_section_png(&raw)
        .with_context(|| format!("resize scenic {}", image.display()))?;
    fs::write(&dest, png).with_context(|| format!("write {}", dest.display()))?;
    fs::write(&rels_path, &rels).with_context(|| format!("write {}", rels_path.display()))?;

    // Strip leftover video XML if this was a video-poster slide.
    let mut xml = fs::read_to_string(&slide).with_context(|| format!("read {}", slide.display()))?;
    let video_nv = Regex::new(r#"<p:video[^>]*>[\s\S]*?</p:video>"#).unwrap();
    xml = video_nv.replace_all(&xml, "").into_owned();
    let video_file = Regex::new(r#"<a:videoFile\b[^>]*/?>"#).unwrap();
    xml = video_file.replace_all(&xml, "").into_owned();
    let p14_ext = Regex::new(
        r#"<p:ext\b[^>]*>\s*<p14:media\b[^>]*/?>\s*</p:ext>"#,
    )
    .unwrap();
    xml = p14_ext.replace_all(&xml, "").into_owned();
    let p14_media = Regex::new(r#"<p14:media\b[^>]*/?>"#).unwrap();
    xml = p14_media.replace_all(&xml, "").into_owned();
    let timing = Regex::new(r#"<p:timing>[\s\S]*?</p:timing>"#).unwrap();
    xml = timing.replace_all(&xml, "").into_owned();
    xml = xml.replace("<p:extLst></p:extLst>", "");
    xml = xml.replace("<p:extLst/>", "");
    let media_click = Regex::new(
        r#"<a:hlinkClick\b[^>]*action="ppaction://media"[^>]*/?>"#,
    )
    .unwrap();
    xml = media_click.replace_all(&xml, "").into_owned();
    fs::write(&slide, xml).with_context(|| format!("write {}", slide.display()))?;

    if fullbleed {
        force_pics_fullbleed(&slide)?;
    }
    Ok(())
}

/// Back-compat alias — full-bleed scenic swap.
pub fn apply_scenic_fullbleed(build: &Path, slide_num: u32, image: &Path) -> Result<()> {
    apply_scenic_image(build, slide_num, image, true)
}

fn warn_if_over_budget(text: &str) {
    use crate::build::ooxml::shape::split_teaching_paragraphs;
    for (i, para) in split_teaching_paragraphs(text).iter().enumerate() {
        if para.chars().count() > ADULT_BODY_PARA_BUDGET {
            eprintln!(
                "warning: adult body paragraph {} has {} chars (budget {}) — may overflow",
                i + 1,
                para.chars().count(),
                ADULT_BODY_PARA_BUDGET
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ensure_single_stretch;

    const FULL: &str = "<a:stretch><a:fillRect/></a:stretch>";

    #[test]
    fn rewrites_self_closing_stretch_instead_of_adding_a_second() {
        let pic = concat!(
            r#"<p:pic><p:blipFill><a:blip r:embed="rId3"></a:blip>"#,
            "<a:stretch/></p:blipFill><p:spPr/></p:pic>"
        );
        let out = ensure_single_stretch(pic).expect("should rewrite");
        assert_eq!(out.matches("<a:stretch").count(), 1);
        assert!(out.contains(FULL));
    }

    #[test]
    fn inserts_stretch_when_blip_fill_has_none() {
        let pic = r#"<p:pic><p:blipFill><a:blip r:embed="rId3"/></p:blipFill></p:pic>"#;
        let out = ensure_single_stretch(pic).expect("should insert");
        assert_eq!(out.matches("<a:stretch").count(), 1);
        assert!(out.contains(FULL));
    }

    #[test]
    fn leaves_an_already_correct_stretch_alone() {
        let pic = format!(
            r#"<p:pic><p:blipFill><a:blip r:embed="rId3"/>{FULL}</p:blipFill></p:pic>"#
        );
        assert!(ensure_single_stretch(&pic).is_none());
    }

    #[test]
    fn collapses_a_duplicated_stretch_pair() {
        let pic = format!(
            r#"<p:pic><p:blipFill><a:blip r:embed="rId3"/>{FULL}<a:stretch/></p:blipFill></p:pic>"#
        );
        let out = ensure_single_stretch(&pic).expect("should collapse");
        assert_eq!(out.matches("<a:stretch").count(), 1);
        assert!(out.contains(FULL));
    }
}
