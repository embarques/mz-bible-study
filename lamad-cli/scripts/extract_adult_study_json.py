#!/usr/bin/env python3
"""Extract structured study JSON from a gold adult Bible-study PPTX.

Reads slide text and SmartArt diagram data from the adult template deck and
writes JSON matching the adult study schema (lectura antifonal, objetivos,
temas with A/B blocks, etc.).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path

A = "{http://schemas.openxmlformats.org/drawingml/2006/main}"
P = "{http://schemas.openxmlformats.org/presentationml/2006/main}"
DGM = "{http://schemas.openxmlformats.org/drawingml/2006/diagram}"

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_PPTX = REPO_ROOT / "lamad-cli/template/adult/24 EL VALOR DE LA MODESTIA.pptx"
DEFAULT_OUT_FIXTURE = REPO_ROOT / "lamad-cli/tests/fixtures/adult/24.json"
DEFAULT_OUT_STUDY = REPO_ROOT / "studies/adult/24.json"

BOOK_CITATION_RE = re.compile(
    r"^(?:\d\s+)?[A-Za-zÁÉÍÓÚÁÉÍÓÚñÑ][\w\s]*?\d+:\d+(?:[,\-–]\d+)*"
)
VERSE_SPLIT_RE = re.compile(
    r"(?<=[.;])\s*(?=\d+\s+[A-ZÁÉÍÓÚ\"])"
    r"|(?<=[a-záéíóúñ])\s*(?=\d+\s+[A-ZÁÉÍÓÚ])"
    r"|(?<=[a-záéíóúñ])(?=\d+\s+[A-ZÁÉÍÓÚ])"
)


def text_from_shape(sp: ET.Element) -> str:
    parts: list[str] = []
    for node in sp.iter(A + "t"):
        if node.text:
            parts.append(node.text)
        if node.tail:
            parts.append(node.tail)
    return "".join(parts).strip()


def shapes_from_slide(xml_bytes: bytes) -> list[str]:
    root = ET.fromstring(xml_bytes)
    texts: list[str] = []
    for sp in root.iter(P + "sp"):
        txt = text_from_shape(sp)
        if txt:
            texts.append(txt)
    return texts


def diagram_texts(zf: zipfile.ZipFile, name: str) -> list[str]:
    root = ET.fromstring(zf.read(f"ppt/diagrams/{name}"))
    out: list[str] = []
    for pt in root.iter(DGM + "pt"):
        t = pt.find(DGM + "t")
        if t is None:
            continue
        text = "".join(t.itertext()).strip()
        if text:
            out.append(text)
    return out


def ordered_slide_files(zf: zipfile.ZipFile) -> list[str]:
    pres = zf.read("ppt/presentation.xml").decode()
    rels = zf.read("ppt/_rels/presentation.xml.rels").decode()
    slide_ids = re.findall(r'r:id="(rId\d+)"', pres)
    rid_to_target = dict(re.findall(r'Id="(rId\d+)"[^>]*Target="([^"]+)"', rels))
    slides: list[str] = []
    for rid in slide_ids:
        target = rid_to_target.get(rid, "")
        if "slides/slide" in target:
            slides.append(target.split("/")[-1])
    return slides


def normalize_ws(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def parse_base_biblica(raw: str) -> list[str]:
    raw = re.sub(r"^Base Bíblica:\s*", "", raw, flags=re.I).strip()
    parts = [p.strip() for p in raw.split(";")]
    out: list[str] = []
    for i, part in enumerate(parts):
        part = part.strip()
        if not part:
            continue
        if i < len(parts) - 1 and not part.endswith(";"):
            part += ";"
        out.append(part)
    return out


def split_glued_citation(raw: str) -> tuple[str, str] | None:
    """Split citations glued to the first verse number."""
    patterns = [
        # 1 Pedro 5:2-72 Apacentad… / 1 Pedro 5:2-75 Igualmente…
        r"^((?:\d\s+)?[A-Za-zÁÉÍÓÚÁÉÍÓÚñÑ][\w\s]*?\d+:\d+(?:-\d+)?)(\d+\s+[A-ZÁÉÍÓÚ].*)$",
        # MATEO 6:3,43 Mas… (comma verse list glued to first verse)
        r"^([A-Za-zÁÉÍÓÚÁÉÍÓÚñÑ][\w\s]*?\d+:\d+(?:,\d)?)(\d+\s+.*)$",
        # General chapter:verse tail glued to verse number
        r"^((?:\d\s+)?[A-Za-zÁÉÍÓÚÁÉÍÓÚñÑ][\w\s]*?\d+:\d+(?:[,\-–]\d+)*)(\d+\s+.*)$",
    ]
    for pattern in patterns:
        m = re.match(pattern, raw, re.S)
        if m:
            return m.group(1).strip(), m.group(2).strip()
    return None


def split_citation_and_verses(raw: str) -> tuple[str, list[str]]:
    raw = raw.strip()
    # Parenthetical citation: "(MATEO 6:1,2)1 Guardaos…"
    m = re.match(r"^\(([^)]+)\)(.*)$", raw, re.S)
    if m:
        return m.group(1).strip(), split_verses(m.group(2).strip())
    # Lectura antifonal: "Mateo 6:1-4; 1 Guardaos…" — only when the tail starts with a verse.
    if "; " in raw:
        left, _, right = raw.partition("; ")
        if re.match(r"^\d+\s+[A-ZÁÉÍÓÚ]", right.strip()):
            return left.strip(), split_verses(right)
    glued = split_glued_citation(raw)
    if glued:
        cita, rest = glued
        return cita, split_verses(rest)
    m = BOOK_CITATION_RE.match(raw)
    if m and m.end() < len(raw):
        return raw[: m.end()].strip(), split_verses(raw[m.end() :].strip())
    return "", split_verses(raw)


def split_verses(text: str) -> list[str]:
    text = text.strip()
    if not text:
        return []
    chunks = VERSE_SPLIT_RE.split(text)
    verses: list[str] = []
    for chunk in chunks:
        chunk = chunk.strip()
        if not chunk:
            continue
        m = re.match(r"^(\d+)\s*(.*)$", chunk, re.S)
        if m:
            verses.append(f"{m.group(1)} {m.group(2).strip()}")
        else:
            verses.append(chunk)
    return verses


def parse_definiciones(text: str) -> list[dict[str, str]]:
    text = text.strip()
    if not text:
        return []
    # Terms are glued like "limosna.Hipócritas." — only split on lowercase→Capital boundaries.
    parts = re.split(r"(?<=[a-záéíóúñ])\.(?=[A-ZÁÉÍÓÚ])", text)
    out: list[dict[str, str]] = []
    for part in parts:
        part = part.strip()
        if not part:
            continue
        if ". " in part:
            term, texto = part.split(". ", 1)
        else:
            term, _, texto = part.partition(".")
        out.append({"termino": term.strip() + ".", "texto": texto.strip()})
    return out


def parse_texto_aureo(text: str) -> dict[str, str | None]:
    text = text.strip()
    m = re.search(r"\s*\(([^)]+)\)\.?\s*$", text)
    if m:
        cita = m.group(1).strip()
        texto = text[: m.start()].strip()
        return {"texto": texto, "cita": cita}
    return {"texto": text, "cita": None}


def parse_tema_header(shapes: list[str]) -> tuple[str, str]:
    if len(shapes) >= 2 and re.match(r"^TEMA\s", shapes[0], re.I):
        titulo = normalize_ws(re.sub(r"^TEMA\s+[IVXLC]+\s*", "", shapes[0], flags=re.I))
        rango = shapes[1].strip()
        return titulo, rango
    joined = shapes[0]
    m = re.search(r"\(([^)]+)\)\s*$", joined)
    if m:
        rango = m.group(1).strip()
        titulo = normalize_ws(re.sub(r"^TEMA\s+[IVXLC]+\s*", "", joined[: m.start()], flags=re.I))
        return titulo, rango
    m = re.search(
        r"\s+((?:\d\s+)?[1-3]?\s*[A-Za-zÁÉÍÓÚ][\w\s.:,;\-–]+)\s*$",
        joined,
    )
    if m and re.search(r"\d", m.group(1)):
        rango = m.group(1).strip()
        titulo = normalize_ws(re.sub(r"^TEMA\s+[IVXLC]+\s*", "", joined[: m.start()], flags=re.I))
        return titulo, rango
    return normalize_ws(re.sub(r"^TEMA\s+[IVXLC]+\s*", "", joined, flags=re.I)), ""


def is_citation_only(text: str) -> bool:
    t = text.strip()
    if not t or re.match(r"^\d+\.[AB]", t, re.I):
        return False
    return bool(re.match(r"^[\(\[]?.+[\)\]]?$", t) and re.search(r"\d", t))


def parse_ab_title(shapes: list[str]) -> str:
    if len(shapes) == 1:
        return normalize_ws(re.sub(r"^\d+\.[AB]\s*-?\s*", "", shapes[0], flags=re.I))
    if re.match(r"^\d+\.[AB]", shapes[0], re.I):
        return normalize_ws(re.sub(r"^\d+\.[AB]\s*", "", shapes[0], flags=re.I))
    if re.match(r"^\d+\.[AB]", shapes[1], re.I):
        return normalize_ws(re.sub(r"^\d+\.[AB]\s*", "", shapes[1], flags=re.I))
    return normalize_ws(" ".join(shapes))


def classify_slide(shapes: list[str]) -> str:
    if not shapes:
        return "unknown"
    s0 = shapes[0]
    u0 = s0.upper()
    if u0 == "LECTURA ANTIFONAL":
        return "lectura_antifonal"
    if u0.startswith("OBJETIVOS"):
        return "objetivos_shell"
    if u0.startswith("PENSAMIENTO CENTRAL"):
        return "pensamiento_shell"
    if u0 == "ENSEÑANZA" or (len(shapes) > 1 and shapes[1].upper() == "ENSEÑANZA"):
        return "ensenanza"
    if u0 == "INTRODUCCIÓN" and len(shapes) == 1:
        return "intro_header"
    if s0.lower().startswith("introducción") and len(shapes) > 1:
        return "intro_body"
    if u0.startswith("TEMA "):
        return "tema_header"
    if u0.startswith("DEFINICIÓN"):
        return "definicion"
    if u0 in ("TEXTO BÍBLICO", "TEXTO BIBLICO") or s0 == "Texto Bíblico":
        return "texto_biblico"
    if re.match(r"^\d+\.A\s*-", s0, re.I):
        return "a_body"
    if re.match(r"^\d+\.B\s*-", s0, re.I):
        return "b_body"
    if re.match(r"^\d+\.A", s0, re.I) or (
        len(shapes) > 1 and re.match(r"^\d+\.A", shapes[1], re.I)
    ):
        return "a_title"
    if re.match(r"^\d+\.B", s0, re.I) or (
        len(shapes) > 1 and re.match(r"^\d+\.B", shapes[1], re.I)
    ):
        return "b_title"
    if is_citation_only(s0) and len(shapes) > 1 and re.match(r"^\d+\.B", shapes[1], re.I):
        return "b_title"
    if "PRÓXIMA ESCUELA BIBLICA" in " ".join(shapes).upper():
        return "proximo"
    if u0.startswith("ESTUDIO") and any("BASE BÍBLICA" in s.upper() for s in shapes):
        return "title"
    return "unknown"


def extract_title(shapes: list[str]) -> tuple[int, str, list[str]]:
    numero = 0
    titulo = ""
    base: list[str] = []
    for s in shapes:
        if s.isdigit():
            numero = int(s)
        elif s.upper() in ("ESTUDIOBÍBLICO", "ESTUDIO BÍBLICO", "ESCUELA BIBLICA"):
            continue
        elif s.upper().startswith("BASE BÍBLICA"):
            base = parse_base_biblica(s)
        elif not titulo and s.upper() not in ("ESTUDIOBÍBLICO",):
            titulo = s.strip()
    return numero, titulo, base


def extract_proximo(shapes: list[str]) -> dict:
    numero = 0
    titulo = ""
    base: list[str] = []
    for s in shapes:
        if s.isdigit():
            numero = int(s)
        elif "PRÓXIMA" in s.upper() or s.upper() in ("ESTUDIOBÍBLICO", "ESTUDIO BÍBLICO"):
            continue
        elif s.upper().startswith("BASE BÍBLICA"):
            base = parse_base_biblica(s)
        elif not titulo:
            titulo = s.strip()
    return {"numero": numero, "titulo": titulo, "base_biblica": base}


def extract_datos_generales(shapes: list[str]) -> dict[str, str]:
    datos = {"autor": "", "personajes": "", "fecha": "", "lugar": ""}
    label_map = {
        "AUTOR": "autor",
        "PERSONAJES": "personajes",
        "FECHA": "fecha",
        "LUGAR": "lugar",
    }
    i = 0
    while i < len(shapes):
        s = shapes[i].strip().upper()
        if s in label_map:
            key = label_map[s]
            if i + 1 < len(shapes):
                datos[key] = shapes[i + 1].strip()
                i += 2
                continue
        i += 1
    return datos


def extract_study(pptx_path: Path) -> dict:
    issues: list[str] = []
    study: dict = {"audience": "adult"}
    temas: list[dict] = []
    current_tema: dict | None = None
    current_side: str | None = None  # "A" or "B"

    with zipfile.ZipFile(pptx_path) as zf:
        slide_files = ordered_slide_files(zf)
        slides = [shapes_from_slide(zf.read(f"ppt/slides/{sf}")) for sf in slide_files]

        objetivos = diagram_texts(zf, "data1.xml")
        data2 = diagram_texts(zf, "data2.xml")

    if len(slides) != 62:
        issues.append(f"expected 62 slides, found {len(slides)}")

    # Title + proximo handled in loop; collect other fields
    study["lectura_antifonal"] = []
    study["introduccion_slides"] = []

    for idx, shapes in enumerate(slides, start=1):
        kind = classify_slide(shapes)
        if kind == "unknown":
            issues.append(f"slide {idx}: unclassified ({shapes[0][:60]!r})")

        if kind == "title":
            numero, titulo, base = extract_title(shapes)
            study["numero"] = numero
            study["titulo"] = titulo
            study["base_biblica"] = base
        elif kind == "lectura_antifonal":
            body = shapes[1] if len(shapes) > 1 else ""
            cita, versiculos = split_citation_and_verses(body)
            study["lectura_antifonal"].append({"cita": cita, "versiculos": versiculos})
        elif kind == "ensenanza":
            ensenanza = ""
            for s in shapes:
                if s.upper() == "ENSEÑANZA":
                    continue
                if s.upper().startswith("DATOS GENERALES"):
                    continue
                if s.isdigit():
                    continue
                if s.upper() in ("AUTOR", "PERSONAJES", "FECHA", "LUGAR"):
                    continue
                if not ensenanza and len(s) > 40:
                    ensenanza = s.strip()
            study["ensenanza"] = ensenanza
            study["datos_generales"] = extract_datos_generales(shapes)
        elif kind == "intro_body":
            body = shapes[1] if len(shapes) > 1 else shapes[0]
            body = re.sub(r"^Introducción\s*", "", body, flags=re.I).strip()
            study["introduccion_slides"].append(body)
        elif kind == "tema_header":
            titulo, rango = parse_tema_header(shapes)
            current_tema = {
                "titulo": titulo,
                "rango": rango,
                "definiciones": [],
                "A": {"titulo": "", "texto_slides": [], "texto_biblico": {"cita": "", "versiculos": []}},
                "B": {"titulo": "", "texto_slides": [], "texto_biblico": {"cita": "", "versiculos": []}},
            }
            temas.append(current_tema)
            current_side = None
        elif kind == "definicion" and current_tema is not None:
            text = shapes[1] if len(shapes) > 1 else ""
            current_tema["definiciones"] = parse_definiciones(text)
        elif kind == "a_title" and current_tema is not None:
            current_tema["A"]["titulo"] = parse_ab_title(shapes)
            current_side = "A"
        elif kind == "b_title" and current_tema is not None:
            current_tema["B"]["titulo"] = parse_ab_title(shapes)
            current_side = "B"
        elif kind == "texto_biblico" and current_tema is not None:
            body = shapes[1] if len(shapes) > 1 else ""
            cita, versiculos = split_citation_and_verses(body)
            block = current_tema[current_side or "A"]
            block["texto_biblico"] = {"cita": cita, "versiculos": versiculos}
        elif kind == "a_body" and current_tema is not None:
            body = shapes[1] if len(shapes) > 1 else ""
            current_tema["A"]["texto_slides"].append(body)
            current_side = "A"
        elif kind == "b_body" and current_tema is not None:
            body = shapes[1] if len(shapes) > 1 else ""
            current_tema["B"]["texto_slides"].append(body)
            current_side = "B"
        elif kind == "proximo":
            study["proximo"] = extract_proximo(shapes)

    study["objetivos"] = objetivos if len(objetivos) == 3 else objetivos[:3]
    if len(objetivos) != 3:
        issues.append(f"expected 3 objetivos in data1.xml, found {len(objetivos)}")

    if len(data2) >= 2:
        study["texto_aureo"] = parse_texto_aureo(data2[0])
        study["pensamiento_central"] = data2[1]
    elif len(data2) == 1:
        parsed = parse_texto_aureo(data2[0])
        if parsed.get("cita"):
            study["texto_aureo"] = parsed
            study["pensamiento_central"] = ""
            issues.append("data2.xml has only one text node; pensamiento_central missing")
        else:
            study["pensamiento_central"] = data2[0]
            study["texto_aureo"] = {"texto": "", "cita": None}
            issues.append("data2.xml has only one text node; texto_aureo missing")
    else:
        study["texto_aureo"] = {"texto": "", "cita": None}
        study["pensamiento_central"] = ""
        issues.append("data2.xml has no text nodes")

    study["temas"] = temas
    if len(temas) != 3:
        issues.append(f"expected 3 temas, found {len(temas)}")

    return study, issues


def write_json(data: dict, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--pptx",
        type=Path,
        default=DEFAULT_PPTX,
        help=f"Source adult PPTX (default: {DEFAULT_PPTX})",
    )
    parser.add_argument(
        "--out-fixture",
        type=Path,
        default=DEFAULT_OUT_FIXTURE,
        help=f"Fixture JSON path (default: {DEFAULT_OUT_FIXTURE})",
    )
    parser.add_argument(
        "--out-study",
        type=Path,
        default=DEFAULT_OUT_STUDY,
        help=f"Study JSON path (default: {DEFAULT_OUT_STUDY})",
    )
    args = parser.parse_args(argv)

    if not args.pptx.is_file():
        print(f"error: PPTX not found: {args.pptx}", file=sys.stderr)
        return 1

    study, issues = extract_study(args.pptx)
    write_json(study, args.out_fixture)
    write_json(study, args.out_study)

    print(args.out_fixture)
    print(args.out_study)
    if issues:
        print("issues:", file=sys.stderr)
        for issue in issues:
            print(f"  - {issue}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
