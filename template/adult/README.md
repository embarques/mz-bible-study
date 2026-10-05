# Adult template

**Gold master:** [`master-template.pptx`](master-template.pptx) — baked from Estudio 24 (*EL VALOR DE LA MODESTIA*) after layout QA.

Also mirrored at `lamad-cli/template/adult/master-template.pptx` (fallback if the repo-root copy is missing).

```bash
lamad build studies/adult/24.json \
  -o "bible-studies/24 - TITLE.pptx" \
  --audience adult --export-pdf
```

**Layout HARD rules** (citations, title chrome, image slides, Texto Áureo, unified A/B panels):  
[`lamad-cli/src/build/adult/LAYOUT_GUIDE.md`](../../lamad-cli/src/build/adult/LAYOUT_GUIDE.md)

Youth continues with `--audience youth` (default) and `template/youth/master-template.pptx`.
