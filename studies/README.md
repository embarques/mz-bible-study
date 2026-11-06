# Studies

Section-folder content for `lamad prepare` (without `-g`).

| Path | Role |
|------|------|
| `adult/{N}/` | Adult section tree + `images/` |
| `adult/{N}.json` | Build cache (written from sections) |
| `youth/` | Youth (unchanged JSON/media for now) |

External extract prompt (PDF → zip): [`../GENERATE_STUDY_AGENT.md`](../GENERATE_STUDY_AGENT.md).
Unzip `adult-estudio-{N}.zip` here so you get `adult/{N}/`, then run prepare **without** `-g`.

Finished decks:

```
{output_dir}/{audience}/{N}/powerpoint/{N} - {TITLE}.pptx
{output_dir}/{audience}/{N}/pdf/{N} - {TITLE}.pdf
```
