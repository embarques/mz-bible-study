//! Low-level OOXML helpers — port of `python/mz_bible_study/build/study.py`
//! (zip/unzip, slide duplication/ordering, shape text surgery, diagrams).
//!
//! ## For contributors (start here)
//!
//! | Task | Module |
//! |------|--------|
//! | Unzip / rezip / fix `ns0:` | [`zip`] |
//! | Duplicate slides / set order | [`slides`] |
//! | Title / Base Bíblica | [`title`] |
//! | Lectura / Texto verses | [`verses`] |
//! | Section chrome, A/B, body | [`body`] |
//! | Propósitos / Idea diagrams | [`diagrams`] |
//! | Section PNG media | [`images`] |
//! | Shape text helpers | [`shape`] |
//! | Tiny XML scanner | [`xml`] |
//!
//! ## HARD RULES (see AGENTS.md)
//!
//! - `[Content_Types].xml` and `ppt/_rels/presentation.xml.rels` — **string
//!   surgery only** (never a DOM writer that emits `ns0:`).
//! - Zipping only skips `.DS_Store`, `._*`, `~$*` — never `startswith(".")`.
//! - `duplicate_slide` strips the `notesSlide` relationship from the clone.
//! - `a:solidFill` must be inserted **before** `a:latin`/`a:ea`/`a:cs` in `a:rPr`.
//! - Shapes named `Arc*` are always stripped from section-chrome slides.

pub mod body;
pub mod diagrams;
pub mod images;
pub(crate) mod shape;
pub mod slides;
pub mod title;
pub mod verses;
pub(crate) mod xml;
pub mod zip;

pub use body::{set_ab_title, set_content_body, set_section_chrome};
pub use diagrams::{force_propositos_36pt, replace_diagram_texts, set_diagram_citation};
pub use images::{replace_section_images, resize_section_png};
pub use slides::{allocate_slides, fix_notes_slide_rels, set_active_order, slide_path};
pub use title::set_title_slide;
pub use verses::{set_verses, VerseKind};
pub use zip::{fix_package_ns0, fix_package_ns_prefixes, unzip_pptx, zip_pptx};

#[cfg(test)]
mod tests {
    use super::body::strip_arc_shapes;
    use super::shape::{set_simple_text_block, shape_block, split_teaching_paragraphs};
    use super::verses::{expand_glued_verses, split_verse};
    use super::xml;

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
    fn fix_ns_prefixes_rewrites_ns2_to_a16() {
        let raw = r#"<p:sld xmlns:ns2="http://schemas.microsoft.com/office/drawing/2014/main"><ns2:creationId id="{ABC}"/></p:sld>"#;
        let out = xml::fix_ns_prefixes(raw);
        assert!(out.contains(r#"xmlns:a16="http://schemas.microsoft.com/office/drawing/2014/main""#));
        assert!(out.contains("<a16:creationId"));
        assert!(!out.contains("ns2:"));
    }

    #[test]
    fn split_teaching_paragraphs_at_markers() {
        let text = "Intro.(1) First point.(2) Second.";
        let parts = split_teaching_paragraphs(text);
        assert_eq!(parts.len(), 3);
        assert!(parts[1].starts_with("(1)"));
    }

    #[test]
    fn expand_glued_verses_splits_semicolon() {
        let verses = vec!["2 foo; 3 bar".to_string()];
        let out = expand_glued_verses(&verses);
        assert_eq!(out.len(), 2);
        assert!(out[0].starts_with('2'));
        assert!(out[1].starts_with('3'));
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
        assert!(out.contains(r#"val="000000""#), "outline color must survive: {out}");
        assert!(out.contains(r#"val="FFFFFF""#));
        assert!(!out.contains(r#"val="C32D2E""#));
    }
}
