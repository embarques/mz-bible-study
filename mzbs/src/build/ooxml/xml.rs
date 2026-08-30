//! Tiny non-nesting XML scanner for shape/run/paragraph surgery.
//!
//! Tags we touch (`a:r`, `a:rPr`, `a:t`, `a:p`, …) do not nest a same-named
//! descendant in this template, so a linear scan is enough and avoids a DOM
//! that could emit `nsN:` prefixes.

use regex::Regex;

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
        .find([' ', '/', '>'])
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
