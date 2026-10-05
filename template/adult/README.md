# Adult template

**Prototype package:** [`master-template.pptx`](master-template.pptx) — OOXML skeleton for `lamad build … --audience adult`.

**Visual / layout authority:** Estudio 4 QA rules enforced in Rust + [`lamad-cli/src/build/adult/LAYOUT_GUIDE.md`](../../lamad-cli/src/build/adult/LAYOUT_GUIDE.md). Do **not** replace this PPTX by remapping a finished built deck — that caused PowerPoint Repair.

Also mirrored at `lamad-cli/template/adult/master-template.pptx` (fallback if the repo-root copy is missing).

```bash
lamad build studies/adult/4.json \
  -o "bible-studies/4 - TITLE.pptx" \
  --audience adult --export-pdf
```

**Layout HARD rules** (citations, title chrome, image slides, Texto Áureo, unified A/B panels):  
[`lamad-cli/src/build/adult/LAYOUT_GUIDE.md`](../../lamad-cli/src/build/adult/LAYOUT_GUIDE.md)

Youth continues with `--audience youth` (default) and `template/youth/master-template.pptx`.
