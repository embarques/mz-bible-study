# Adult template

Place `master-template.pptx` here (or under `lamad-cli/template/adult/`) for adult builds.

```bash
lamad build studies/adult/24.json \
  -o "bible-studies/24 - TITLE.pptx" \
  --audience adult --export-pdf
```

**Layout HARD rules** (citations, title chrome, image slides, Texto Áureo):  
[`lamad-cli/src/build/adult/LAYOUT_GUIDE.md`](../../lamad-cli/src/build/adult/LAYOUT_GUIDE.md)

Youth continues with `--audience youth` (default) and `template/youth/master-template.pptx`.
