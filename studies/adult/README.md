# Adult prepare inputs

`lamad prepare … --audience adult` writes:

| Path | Role |
|------|------|
| `studies/adult/{N}.json` | Adult study JSON (not youth schema) |
| `studies/adult/{N}/intro-header.png` | Intro full-bleed |
| `studies/adult/{N}/tema-{1,2,3}.png` | Tema headers |
| `studies/adult/{N}/ab-{1A…3B}.png` | A/B title scenics |
| `studies/adult/{N}/definicion-{1,2,3}.png` | Definición cards |

Then builds `bible-studies/{N} - {TITLE}.pptx` from `template/adult/master-template.pptx`.
