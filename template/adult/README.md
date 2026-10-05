# Adult template

**Gold master:** [`master-template.pptx`](master-template.pptx) — baked from Estudio 4 (*DIOS BUSCA DISCÍPULOS CONFORME A SU CORAZÓN*) after manual layout QA (title, datos, intro body, TEMA/A-B image chrome, próximo).

Also mirrored at `lamad-cli/template/adult/master-template.pptx` (fallback if the repo-root copy is missing).

Regenerate from the finished gold deck:

```bash
python3 lamad-cli/scripts/prepare_adult_master_template.py
./target/debug/lamad validate template/adult/master-template.pptx
```

```bash
lamad build studies/adult/4.json \
  -o "bible-studies/4 - TITLE.pptx" \
  --audience adult --export-pdf
```

**Layout HARD rules** (citations, title chrome, image slides, Texto Áureo, unified A/B panels):  
[`lamad-cli/src/build/adult/LAYOUT_GUIDE.md`](../../lamad-cli/src/build/adult/LAYOUT_GUIDE.md)

Youth continues with `--audience youth` (default) and `template/youth/master-template.pptx`.
