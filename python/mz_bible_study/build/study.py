"""Build a Mount Zion Church Bible-study PPTX from structured JSON + section images.

Clone master-template.pptx, pack text into as many slides as needed (duplicate
prototypes; omit unused), apply JSON + section images, enforce Propósitos 36pt /
Conclusión Times New Roman 42pt, validate, optionally export PDF.
"""
from __future__ import annotations

import copy
import json
import re
import shutil
import sys
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

from mz_bible_study.audience import normalize_audience
from mz_bible_study.build.base import AdultBuilderNotImplemented, get_builder
from mz_bible_study.build.youth.proto import PROTO
from mz_bible_study.paths import master_template, project_root

P = "{http://schemas.openxmlformats.org/presentationml/2006/main}"
A = "{http://schemas.openxmlformats.org/drawingml/2006/main}"
R = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"
REL = "{http://schemas.openxmlformats.org/package/2006/relationships}"
CT = "{http://schemas.openxmlformats.org/package/2006/content-types}"
DGM = "{http://schemas.openxmlformats.org/drawingml/2006/diagram}"
DSP = "{http://schemas.microsoft.com/office/drawing/2008/diagram}"
XML = "{http://www.w3.org/XML/1998/namespace}"

for prefix, uri in [
    ("p", P), ("a", A), ("r", R), ("rel", REL), ("ct", CT), ("dgm", DGM), ("dsp", DSP),
]:
    ET.register_namespace(prefix, uri)

def slide_path(build: Path, num: int) -> Path:
    return build / "ppt" / "slides" / f"slide{num}.xml"


def list_slide_nums(build: Path) -> list[int]:
    return sorted(
        int(p.stem[5:])
        for p in (build / "ppt" / "slides").glob("slide*.xml")
        if p.stem[5:].isdigit()
    )


def fix_package_ns0(build: Path) -> None:
    """Rewrite ns0:Relationships corruption (seen on some master-template clones)."""
    rels_path = build / "ppt" / "_rels" / "presentation.xml.rels"
    text = rels_path.read_text(encoding="utf-8")
    if "ns0:" in text or "xmlns:ns0=" in text:
        text = text.replace("ns0:", "")
        text = text.replace(
            'xmlns:ns0="http://schemas.openxmlformats.org/package/2006/relationships"',
            'xmlns="http://schemas.openxmlformats.org/package/2006/relationships"',
        )
        # If both xmlns= and leftover appear, leave the default xmlns
        if 'xmlns="http://schemas.openxmlformats.org/package/2006/relationships"' not in text:
            text = text.replace(
                "<Relationships>",
                '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">',
                1,
            )
        rels_path.write_text(text, encoding="utf-8")
    ct_path = build / "[Content_Types].xml"
    ct = ct_path.read_text(encoding="utf-8")
    if "ns0:" in ct:
        ct = ct.replace("ns0:", "")
        ct = ct.replace(
            'xmlns:ns0="http://schemas.openxmlformats.org/package/2006/content-types"',
            'xmlns="http://schemas.openxmlformats.org/package/2006/content-types"',
        )
        ct_path.write_text(ct, encoding="utf-8")


def duplicate_slide(build: Path, proto: int) -> int:
    """Copy slide XML + rels to a new slideN. Strip notes rels (never leave notesSlide{old})."""
    nums = list_slide_nums(build)
    dst = max(nums) + 1 if nums else 1
    src_xml = slide_path(build, proto)
    dst_xml = slide_path(build, dst)
    if not src_xml.exists():
        die(f"prototype slide missing: {src_xml}")
    shutil.copy2(src_xml, dst_xml)

    rels_dir = build / "ppt" / "slides" / "_rels"
    src_rels = rels_dir / f"slide{proto}.xml.rels"
    dst_rels = rels_dir / f"slide{dst}.xml.rels"
    if src_rels.exists():
        rels = src_rels.read_text(encoding="utf-8")
        rels = re.sub(
            r'<Relationship\b[^>]*Type="[^"]*notesSlide"[^>]*/>',
            "",
            rels,
        )
        dst_rels.write_text(rels, encoding="utf-8")

    ct_path = build / "[Content_Types].xml"
    ct = ct_path.read_text(encoding="utf-8")
    part = f"/ppt/slides/slide{dst}.xml"
    if part not in ct:
        override = (
            f'<Override PartName="{part}" '
            f'ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>'
        )
        ct = ct.replace("</Types>", override + "</Types>")
        ct_path.write_text(ct, encoding="utf-8")
    return dst


def allocate_slides(build: Path, proto: int, count: int) -> list[int]:
    """Create `count` slides by duplicating prototype (never reuses proto in-place)."""
    if count < 0:
        die(f"negative slide count for proto {proto}")
    return [duplicate_slide(build, proto) for _ in range(count)]


def set_active_order(build: Path, slide_nums: list[int]) -> None:
    """Rebuild presentation slide rels + sldIdLst to exactly this order (string-safe)."""
    if not slide_nums:
        die("active order is empty")
    for n in slide_nums:
        if not slide_path(build, n).exists():
            die(f"active slide missing on disk: slide{n}.xml")

    rels_path = build / "ppt" / "_rels" / "presentation.xml.rels"
    rels = rels_path.read_text(encoding="utf-8")
    # Drop existing slide relationships
    rels = re.sub(
        r'<(?:[\w]+:)?Relationship\b[^>]*Type="[^"]*/relationships/slide"[^>]*/>\s*',
        "",
        rels,
    )
    used = [int(x) for x in re.findall(r'\bId="rId(\d+)"', rels)]
    next_id = (max(used) + 1) if used else 1
    rids: list[str] = []
    chunks: list[str] = []
    for n in slide_nums:
        rid = f"rId{next_id}"
        next_id += 1
        rids.append(rid)
        chunks.append(
            f'<Relationship Id="{rid}" '
            f'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" '
            f'Target="slides/slide{n}.xml"/>'
        )
    if "</Relationships>" not in rels:
        die("presentation.xml.rels missing </Relationships>")
    rels = rels.replace("</Relationships>", "".join(chunks) + "</Relationships>")
    rels_path.write_text(rels, encoding="utf-8")

    pres_path = build / "ppt" / "presentation.xml"
    pres = pres_path.read_text(encoding="utf-8")
    items = "".join(
        f'<p:sldId id="{256 + i}" r:id="{rid}"/>' for i, rid in enumerate(rids)
    )
    new_pres, n = re.subn(
        r"<p:sldIdLst>.*?</p:sldIdLst>",
        f"<p:sldIdLst>{items}</p:sldIdLst>",
        pres,
        count=1,
        flags=re.S,
    )
    if n != 1:
        die("failed to rewrite p:sldIdLst")
    pres_path.write_text(new_pres, encoding="utf-8")


def as_lines(value) -> list[str]:
    if value is None:
        return []
    if isinstance(value, str):
        return [b.strip() for b in value.split(";") if b.strip()]
    return list(value)


REF_RE = re.compile(
    r"(\((?:v\.?\s*\d+|[1-3]?\s*[A-Za-zÁÉÍÓÚáéíóúñÑ][A-Za-zÁÉÍÓÚáéíóúñÑ\s]+\s+\d+[^\)]*)\)|"
    r"(?:[1-3]?\s*[A-Za-zÁÉÍÓÚáéíóúñÑ][A-Za-zÁÉÍÓÚáéíóúñÑ\.]*(?:\s+[A-Za-zÁÉÍÓÚáéíóúñÑ\.]+)*)\s+\d+:\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)"
)


def die(msg: str, code: int = 1) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    raise SystemExit(code)


def parse_xml(path: Path) -> ET.ElementTree:
    return ET.parse(path)


# Preferred OOXML prefixes (ElementTree otherwise emits ns0/ns1 and PowerPoint
# drops white Texto Bíblico on black slides).
NS_MAP = {
    "http://schemas.openxmlformats.org/presentationml/2006/main": "p",
    "http://schemas.openxmlformats.org/drawingml/2006/main": "a",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships": "r",
    "http://schemas.openxmlformats.org/package/2006/relationships": "pr",
    "http://schemas.openxmlformats.org/package/2006/content-types": "ct",
    "http://schemas.openxmlformats.org/drawingml/2006/diagram": "dgm",
    "http://schemas.microsoft.com/office/drawing/2008/diagram": "dsp",
    "http://schemas.microsoft.com/office/powerpoint/2010/main": "p14",
    "http://schemas.microsoft.com/office/drawing/2010/main": "a14",
    "http://schemas.microsoft.com/office/drawing/2014/main": "a16",
    "http://schemas.microsoft.com/office/office/2014/main": "o16",
    "http://schemas.openxmlformats.org/markup-compatibility/2006": "mc",
    "http://www.w3.org/XML/1998/namespace": "xml",
}

for uri, prefix in NS_MAP.items():
    try:
        ET.register_namespace(prefix, uri)
    except ValueError:
        pass


def write_xml(tree: ET.ElementTree, path: Path) -> None:
    """Serialize slide/diagram XML with real OOXML prefixes (never ns0:)."""
    for uri, prefix in NS_MAP.items():
        try:
            ET.register_namespace(prefix, uri)
        except ValueError:
            pass
    # tostring still sometimes emits nsN for unregistered URIs — remap those.
    raw = ET.tostring(tree.getroot(), encoding="utf-8", xml_declaration=True).decode("utf-8")
    raw = _fix_ns_prefixes(raw)
    # Prefer standalone declarations like the originals
    if raw.startswith("<?xml"):
        raw = re.sub(r"<\?xml[^?]+\?>", '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>', raw, count=1)
    path.write_text(raw, encoding="utf-8")


def _fix_ns_prefixes(xml: str) -> str:
    """Rewrite xmlns:nsN / nsN: tags back to canonical p/a/r/... prefixes."""
    decls = dict(re.findall(r'xmlns:ns(\d+)="([^"]+)"', xml))
    if not decls:
        # Also handle default ElementTree form without prior ns map if prefixes already good
        return xml
    # Build nsN -> preferred prefix
    remap: dict[str, str] = {}
    used: set[str] = set()
    for num, uri in decls.items():
        pref = NS_MAP.get(uri)
        if pref is None:
            # keep a stable unknown prefix rather than nsN
            pref = f"x{num}"
        # avoid collisions
        base = pref
        i = 2
        while pref in used:
            pref = f"{base}{i}"
            i += 1
        used.add(pref)
        remap[num] = pref

    # Replace xmlns declarations first
    def repl_decl(m: re.Match) -> str:
        num, uri = m.group(1), m.group(2)
        return f'xmlns:{remap[num]}="{uri}"'

    xml = re.sub(r'xmlns:ns(\d+)="([^"]+)"', repl_decl, xml)
    # Replace prefix usages nsN: → pref:
    for num, pref in sorted(remap.items(), key=lambda x: -len(x[0])):
        xml = xml.replace(f"ns{num}:", f"{pref}:")
    return xml


def unzip_pptx(src: Path, dest: Path) -> None:
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    with zipfile.ZipFile(src) as z:
        z.extractall(dest)


def zip_pptx(src_dir: Path, dest: Path) -> None:
    if dest.exists():
        dest.unlink()
    with zipfile.ZipFile(dest, "w", compression=zipfile.ZIP_DEFLATED) as z:
        for path in sorted(src_dir.rglob("*")):
            if not path.is_file():
                continue
            name = path.name
            if name == ".DS_Store" or name.startswith("._") or name.startswith("~$"):
                continue
            # Never use startswith(".") — that drops _rels/.rels
            z.write(path, path.relative_to(src_dir).as_posix())


def find_shape(root: ET.Element, name: str) -> ET.Element | None:
    for sp in root.iter(P + "sp"):
        nv = sp.find(f"{P}nvSpPr/{P}cNvPr")
        if nv is not None and nv.get("name") == name:
            return sp
    return None


def clear_paragraphs(txBody: ET.Element) -> list[ET.Element]:
    paras = list(txBody.findall(A + "p"))
    for p in paras:
        txBody.remove(p)
    return paras


def clone_run(sample: ET.Element | None, text: str, *, bold: bool | None = None) -> ET.Element:
    if sample is not None:
        r = copy.deepcopy(sample)
    else:
        r = ET.Element(A + "r")
        rPr = ET.SubElement(r, A + "rPr")
        rPr.set("lang", "es-MX")
        ET.SubElement(r, A + "t")
    rPr = r.find(A + "rPr")
    if rPr is None:
        rPr = ET.Element(A + "rPr")
        r.insert(0, rPr)
    if bold is True:
        rPr.set("b", "1")
    elif bold is False:
        if "b" in rPr.attrib:
            del rPr.attrib["b"]
    # preserve otherwise
    t = r.find(A + "t")
    if t is None:
        t = ET.SubElement(r, A + "t")
    if text.startswith(" ") or text.endswith(" ") or (text != text.strip()):
        t.set("{http://www.w3.org/XML/1998/namespace}space", "preserve")
    t.text = text
    return r


def first_run(sp: ET.Element) -> ET.Element | None:
    return next(sp.iter(A + "r"), None)


def set_simple_text(sp: ET.Element, text: str, *, bold: bool | None = None) -> None:
    """Replace shape text with a single run, preserving first-run formatting."""
    txBody = sp.find(P + "txBody")
    if txBody is None:
        txBody = sp.find(A + "txBody")
    if txBody is None:
        die(f"shape has no txBody: {sp.find(f'{P}nvSpPr/{P}cNvPr').get('name')}")
    sample = first_run(sp)
    paras = clear_paragraphs(txBody)
    p = copy.deepcopy(paras[0]) if paras else ET.Element(A + "p")
    # remove old runs/br from paragraph, keep pPr
    for child in list(p):
        if child.tag != A + "pPr":
            p.remove(child)
    p.append(clone_run(sample, text, bold=bold))
    end = ET.SubElement(p, A + "endParaRPr")
    end.set("lang", "es-MX")
    txBody.append(p)


def set_body_with_refs(sp: ET.Element, text: str, *, force_tnr_42: bool = False) -> None:
    """Body text: regular weight; bold inline scripture refs only."""
    txBody = sp.find(P + "txBody")
    if txBody is None:
        die("missing txBody")
    sample = first_run(sp)
    paras = clear_paragraphs(txBody)
    p = copy.deepcopy(paras[0]) if paras else ET.Element(A + "p")
    for child in list(p):
        if child.tag != A + "pPr":
            p.remove(child)

    parts: list[tuple[str, bool]] = []
    pos = 0
    for m in REF_RE.finditer(text):
        if m.start() > pos:
            parts.append((text[pos : m.start()], False))
        parts.append((m.group(0), True))
        pos = m.end()
    if pos < len(text):
        parts.append((text[pos:], False))
    if not parts:
        parts = [(text, False)]

    for chunk, is_ref in parts:
        if not chunk:
            continue
        r = clone_run(sample, chunk, bold=True if is_ref else False)
        if force_tnr_42:
            rPr = r.find(A + "rPr")
            rPr.set("sz", "4200")
            for tag in (A + "latin", A + "ea", A + "cs"):
                el = rPr.find(tag)
                if el is None:
                    el = ET.SubElement(rPr, tag)
                el.set("typeface", "Times New Roman")
        p.append(r)
    end = ET.SubElement(p, A + "endParaRPr")
    end.set("lang", "es-MX")
    if force_tnr_42:
        end.set("sz", "4200")
    txBody.append(p)


def set_title_slide(path: Path, *, numero: int | str, titulo: str, base: list[str], estudio_label: bool = True) -> None:
    tree = parse_xml(path)
    root = tree.getroot()
    sp_num = find_shape(root, "TextBox 4")
    sp_title = find_shape(root, "CuadroTexto 13")
    sp_base = find_shape(root, "TextBox 7")
    if sp_num is None or sp_title is None or sp_base is None:
        die(f"title slide missing shapes: {path}")
    set_simple_text(sp_num, str(numero), bold=True)
    # Title uses Verlag Black — never add b="1" (good decks leave bold unset).
    set_simple_text(sp_title, titulo, bold=False)
    # Base Bíblica: label bold; citation lines regular
    txBody = sp_base.find(P + "txBody")
    sample = first_run(sp_base)
    clear_paragraphs(txBody)
    lines = [f"Base Bíblica:"] + list(base)
    # Single paragraph with soft breaks, matching template style
    p = ET.Element(A + "p")
    pPr = ET.SubElement(p, A + "pPr")
    pPr.set("algn", "ctr")
    p.append(clone_run(sample, "Base Bíblica:", bold=True))
    for i, line in enumerate(base):
        br = ET.SubElement(p, A + "br")
        # citation regular
        suffix = "" if i == len(base) - 1 else ""
        p.append(clone_run(sample, line + suffix, bold=False))
    txBody.append(p)
    write_xml(tree, path)


def split_verse(v: str) -> tuple[str, str]:
    m = re.match(r"^(\d+)\s+(.*)$", v.strip(), re.S)
    if not m:
        die(f"verse must start with number: {v[:60]!r}")
    return m.group(1), " " + m.group(2)


def pack_verses(verses: list[str], *, budget: int = 280) -> list[list[str]]:
    packs: list[list[str]] = []
    cur: list[str] = []
    cur_len = 0
    for v in verses:
        add = len(v) + 1
        if cur and cur_len + add > budget:
            packs.append(cur)
            cur = [v]
            cur_len = len(v)
        else:
            cur.append(v)
            cur_len += add
    if cur:
        packs.append(cur)
    return packs


def run_color(r: ET.Element) -> str | None:
    rPr = r.find(A + "rPr")
    if rPr is None:
        return None
    s = rPr.find(f"{A}solidFill/{A}srgbClr")
    return s.get("val") if s is not None else None


def set_run_color(r: ET.Element, rgb: str | None) -> None:
    """Force run solid fill. rgb=None removes solidFill (inherits / black body).

    OOXML rPr child order matters: solidFill must come BEFORE latin/ea/cs.
    Appending solidFill after latin makes PowerPoint ignore the color — white
    Texto body becomes black-on-black and looks blank.
    """
    rPr = r.find(A + "rPr")
    if rPr is None:
        rPr = ET.Element(A + "rPr")
        r.insert(0, rPr)
    for child in list(rPr):
        if child.tag == A + "solidFill":
            rPr.remove(child)
    if rgb is None:
        return
    fill = ET.Element(A + "solidFill")
    ET.SubElement(fill, A + "srgbClr").set("val", rgb)
    # Insert before first typeface node (latin/ea/cs), else at start
    insert_at = 0
    for i, child in enumerate(list(rPr)):
        local = child.tag.split("}")[-1]
        if local in ("latin", "ea", "cs", "sym"):
            insert_at = i
            break
        insert_at = i + 1
    rPr.insert(insert_at, fill)


def pick_verse_samples(runs: list[ET.Element]) -> tuple[ET.Element, ET.Element, ET.Element]:
    """Pick cite / number / body samples by content — not by fixed index.

    Continuation Lectura/Texto slides start with a number run, so index-based
    picking swaps colors (body inherits red/yellow; numbers inherit black/white).
    """
    sample_num = None
    sample_body = None
    sample_cite = None

    def run_text(r: ET.Element) -> str:
        t = r.find(A + "t")
        return (t.text or "") if t is not None else ""

    for r in runs:
        text = run_text(r)
        stripped = text.strip()
        if not stripped:
            continue
        if stripped.isdigit():
            if sample_num is None:
                sample_num = r
            continue
        # Citation titles are short-ish and usually contain ':' (Josué 10:7-14)
        if sample_cite is None and ":" in stripped and len(stripped) < 40:
            sample_cite = r
            continue
        if sample_body is None and (text.startswith(" ") or len(stripped) > 15):
            sample_body = r

    if sample_num is None:
        sample_num = next((r for r in runs if run_text(r).strip().isdigit()), runs[0])
    if sample_body is None:
        sample_body = next(
            (r for r in runs if not run_text(r).strip().isdigit()),
            runs[-1],
        )
    if sample_cite is None:
        sample_cite = sample_num
    return sample_cite, sample_num, sample_body


def set_verses(path: Path, verses: list[str], *, citation: str | None, kind: str) -> None:
    """kind=lectura (red cite+nums, black body) or texto (yellow cite+nums, white body).

    Match known-good decks: one paragraph per citation / per verse (no a:br soft breaks).
    """
    tree = parse_xml(path)
    root = tree.getroot()
    shape_name = "CuadroTexto 5" if kind == "lectura" else "TextBox 4"
    accent = "FF0000" if kind == "lectura" else "FFFF00"
    body_rgb = None if kind == "lectura" else "FFFFFF"  # lectura body = default black
    sp = find_shape(root, shape_name)
    if sp is None:
        die(f"missing {shape_name} in {path}")
    txBody = sp.find(P + "txBody")
    runs = list(sp.iter(A + "r"))
    if not runs:
        die(f"no sample runs in {path}")
    sample_cite, sample_num, sample_body = pick_verse_samples(runs)
    paras = clear_paragraphs(txBody)

    def new_para() -> ET.Element:
        p = copy.deepcopy(paras[0]) if paras else ET.Element(A + "p")
        for child in list(p):
            if child.tag != A + "pPr":
                p.remove(child)
        return p

    if citation:
        p = new_para()
        r = clone_run(sample_cite, citation, bold=True)
        set_run_color(r, accent)
        p.append(r)
        txBody.append(p)
    for v in verses:
        num, body = split_verse(v)
        p = new_para()
        r_num = clone_run(sample_num, num, bold=True)
        set_run_color(r_num, accent)
        p.append(r_num)
        r_body = clone_run(sample_body, body, bold=False)
        set_run_color(r_body, body_rgb)
        p.append(r_body)
        txBody.append(p)
    write_xml(tree, path)


def set_section_chrome(path: Path, title: str, rango: str, n: int) -> None:
    tree = parse_xml(path)
    root = tree.getroot()
    sp_t = find_shape(root, "CuadroTexto 8")
    sp_v = find_shape(root, "CuadroTexto 3")
    if sp_t is None or sp_v is None:
        die(f"section chrome shapes missing: {path}")
    set_simple_text(sp_t, f"{n}- {title}", bold=True)
    set_simple_text(sp_v, rango, bold=True)
    # Strip retired Arc dashed chrome
    for spTree in root.iter(P + "spTree"):
        for child in list(spTree):
            nv = child.find(f"{P}nvSpPr/{P}cNvPr")
            if nv is not None and (nv.get("name") or "").startswith("Arc"):
                spTree.remove(child)
    write_xml(tree, path)


def set_ab_title(path: Path, title: str) -> None:
    tree = parse_xml(path)
    root = tree.getroot()
    sp = find_shape(root, "Título 1")
    if sp is None:
        die(f"missing Título 1: {path}")
    set_simple_text(sp, title, bold=True)
    write_xml(tree, path)


def set_content_body(path: Path, text: str, *, shape: str = "Marcador de contenido 2", conclusion: bool = False) -> None:
    tree = parse_xml(path)
    root = tree.getroot()
    sp = find_shape(root, shape)
    if sp is None and conclusion:
        sp = find_shape(root, "CuadroTexto 5")
    if sp is None:
        die(f"missing body shape in {path}")
    set_body_with_refs(sp, text, force_tnr_42=conclusion)
    write_xml(tree, path)


def replace_diagram_texts(data_path: Path, drawing_path: Path | None, texts: list[str], *, force_drawing_sz: int | None = None) -> None:
    """Replace longest / purpose-like <a:t> nodes in order with provided texts."""
    tree = parse_xml(data_path)
    root = tree.getroot()
    nodes = [t for t in root.iter(A + "t") if (t.text or "").strip()]
    # Heuristic: replace the N longest non-trivial texts for propósitos/idea
    # Prefer nodes whose text length > 15
    candidates = [t for t in nodes if len((t.text or "").strip()) > 15]
    if len(candidates) < len(texts):
        candidates = nodes[-len(texts) :]
    # Keep document order among candidates long enough
    candidates = [t for t in nodes if len((t.text or "").strip()) > 15][: len(texts)]
    if len(candidates) != len(texts):
        # fallback: last N text nodes
        candidates = [t for t in nodes if (t.text or "").strip()][-len(texts) :]
    if len(candidates) != len(texts):
        die(f"diagram text count mismatch in {data_path}: need {len(texts)} got {len(candidates)}")
    for node, text in zip(candidates, texts):
        node.text = text
    write_xml(tree, data_path)

    if drawing_path and drawing_path.exists():
        dtree = parse_xml(drawing_path)
        droot = dtree.getroot()
        dnodes = [t for t in droot.iter(A + "t") if (t.text or "").strip()]
        dcand = [t for t in dnodes if len((t.text or "").strip()) > 15][: len(texts)]
        if len(dcand) != len(texts):
            dcand = [t for t in dnodes if (t.text or "").strip()][-len(texts) :]
        for node, text in zip(dcand, texts):
            node.text = text
        if force_drawing_sz is not None:
            for rPr in droot.iter(A + "rPr"):
                # Only shrink body runs that were template 56pt / large
                sz = rPr.get("sz")
                if sz and int(sz) >= 4000:
                    # Propósitos bodies should be 3600; leave headers alone if smaller labels
                    parent = None
                if sz and int(sz) >= 5000:
                    rPr.set("sz", str(force_drawing_sz))
        write_xml(dtree, drawing_path)


def force_propositos_36pt(drawing2: Path) -> None:
    tree = parse_xml(drawing2)
    root = tree.getroot()
    for rPr in root.iter(A + "rPr"):
        sz = rPr.get("sz")
        if sz and int(sz) >= 5000:
            rPr.set("sz", "3600")
    write_xml(tree, drawing2)


def replace_section_images(build: Path, images: list[Path]) -> None:
    if len(images) != 3:
        die("exactly 3 section images required")
    for i, img in enumerate(images, start=4):
        dest = build / "ppt" / "media" / f"image{i}.png"
        if not img.exists():
            die(f"image not found: {img}")
        shutil.copy2(img, dest)


def pack_sentences(text: str, budget: int = 380) -> list[str]:
    sentences = re.split(r"(?<=[.!?…])\s+", text.strip())
    sentences = [s for s in sentences if s]
    packs: list[str] = []
    cur = ""
    for s in sentences:
        trial = s if not cur else f"{cur} {s}"
        if cur and len(trial) > budget:
            packs.append(cur)
            cur = s
        else:
            cur = trial
    if cur:
        packs.append(cur)
    return packs


def apply_study(build: Path, study: dict) -> list[int]:
    """Fill content and return the final active slide-number order."""
    order: list[int] = []
    base_lines = as_lines(study.get("base_biblica"))

    # Title (reuse prototype in-place)
    title_n = PROTO["title"]
    order.append(title_n)
    set_title_slide(
        slide_path(build, title_n),
        numero=study["numero"],
        titulo=study["titulo"],
        base=base_lines,
    )

    # Lectura — pack + allocate
    lectura = study["lectura"]
    lectura_packs = study.get("lectura_slides") or pack_verses(lectura["versiculos"], budget=280)
    lectura_nums = allocate_slides(build, PROTO["lectura"], len(lectura_packs))
    order.extend(lectura_nums)
    for i, n in enumerate(lectura_nums):
        set_verses(
            slide_path(build, n),
            lectura_packs[i],
            citation=lectura["cita"] if i == 0 else None,
            kind="lectura",
        )

    # Propósitos / Idea (in-place)
    order.append(PROTO["propositos"])
    order.append(PROTO["idea"])
    replace_diagram_texts(
        build / "ppt/diagrams/data2.xml",
        build / "ppt/diagrams/drawing2.xml",
        study["propositos"],
        force_drawing_sz=3600,
    )
    force_propositos_36pt(build / "ppt/diagrams/drawing2.xml")

    idea = study["idea_principal"]
    memo = study["para_memorizar"]["texto"]
    replace_diagram_texts(
        build / "ppt/diagrams/data3.xml",
        build / "ppt/diagrams/drawing3.xml",
        [idea, memo],
    )
    cite = study["para_memorizar"].get("cita")
    if cite:
        d3 = parse_xml(build / "ppt/diagrams/data3.xml")
        for t in d3.getroot().iter(A + "t"):
            if (t.text or "").strip() and len(t.text.strip()) < 40 and ":" in t.text:
                t.text = cite
                break
        write_xml(d3, build / "ppt/diagrams/data3.xml")

    # Comentario
    comentario = study.get("comentario_slides") or pack_sentences(study.get("comentario", ""))
    com_nums = allocate_slides(build, PROTO["comentario"], len(comentario))
    order.extend(com_nums)
    for n, text in zip(com_nums, comentario):
        set_content_body(slide_path(build, n), text)

    # Intro header + bodies
    order.append(PROTO["intro_header"])
    intro = study.get("introduccion_slides") or pack_sentences(study.get("introduccion", ""))
    intro_nums = allocate_slides(build, PROTO["intro"], len(intro))
    order.extend(intro_nums)
    for n, text in zip(intro_nums, intro):
        set_content_body(slide_path(build, n), text)

    # Points 1–3
    puntos = study["puntos"]
    if len(puntos) != 3:
        die(f"expected 3 puntos, got {len(puntos)}")
    section_protos = PROTO["section"]
    for idx, punto in enumerate(puntos):
        sec_n = section_protos[idx]
        order.append(sec_n)
        set_section_chrome(slide_path(build, sec_n), punto["titulo"], punto["rango"], punto["n"])

        tpacks = punto.get("texto_slides") or pack_verses(punto["texto_biblico"], budget=280)
        texto_nums = allocate_slides(build, PROTO["texto"], len(tpacks))
        order.extend(texto_nums)
        for j, n in enumerate(texto_nums):
            set_verses(
                slide_path(build, n),
                tpacks[j],
                citation=punto["rango"] if j == 0 else None,
                kind="texto",
            )

        for letter in ("A", "B"):
            block = punto[letter]
            ab_title = f"{punto['n']}.{letter}- {block['titulo']}"
            texts = block.get("slides") or pack_sentences(block.get("cuerpo", ""))
            ab_nums = allocate_slides(build, PROTO["ab"], len(texts))
            order.extend(ab_nums)
            for n, text in zip(ab_nums, texts):
                set_ab_title(slide_path(build, n), ab_title)
                set_content_body(slide_path(build, n), text)

    # Conclusión
    conclusion = study.get("conclusion_slides") or pack_sentences(study.get("conclusion", ""))
    conc_nums = allocate_slides(build, PROTO["conclusion"], len(conclusion))
    order.extend(conc_nums)
    for n, text in zip(conc_nums, conclusion):
        set_content_body(slide_path(build, n), text, shape="CuadroTexto 5", conclusion=True)

    # Próximo (omit if study says so)
    prox = study.get("proximo")
    if prox:
        prox_n = PROTO["proximo"]
        order.append(prox_n)
        set_title_slide(
            slide_path(build, prox_n),
            numero=prox["numero"],
            titulo=prox["titulo"],
            base=as_lines(prox.get("base_biblica")),
        )

    images = [
        project_root() / p if not Path(p).is_absolute() else Path(p)
        for p in study.get("section_images", [])
    ]
    if len(images) != 3:
        die("study JSON must include section_images: [3 paths]")
    replace_section_images(build, images)

    print(
        f"Packed {len(order)} active slides "
        f"(lectura={len(lectura_nums)}, comentario={len(com_nums)}, "
        f"intro={len(intro_nums)}, conclusion={len(conc_nums)})"
    )
    return order


def build_study(
    study_path: Path,
    output: Path,
    *,
    base: Path | None = None,
    audience: str | None = None,
    export_pdf: bool = False,
) -> Path:
    """Build a study deck via the audience-specific builder. Returns the output pptx path."""
    study_path = study_path.resolve()
    output = output.resolve()
    study = json.loads(study_path.read_text(encoding="utf-8"))

    try:
        aud = normalize_audience(audience if audience is not None else study.get("audience"))
    except ValueError as exc:
        die(str(exc))

    if base is not None:
        pptx = base.resolve()
    else:
        json_tmpl = study.get("template")
        if json_tmpl:
            p = Path(str(json_tmpl))
            pptx = (project_root() / p if not p.is_absolute() else p).resolve()
        else:
            try:
                pptx = master_template(aud).resolve()
            except FileNotFoundError as exc:
                die(str(exc))

    builder = get_builder(aud)
    try:
        return builder.build(
            study_path,
            output,
            base=pptx,
            export_pdf=export_pdf,
        )
    except AdultBuilderNotImplemented as exc:
        die(str(exc))
