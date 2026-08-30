# Prepare inputs (by audience)

| Folder | CLI flag | Contents |
|--------|----------|----------|
| `studies/youth/` | `--audience youth` (default) | `{N}.json`, `media/{N}-section*.png`, `REVIEW.md` |
| `studies/adult/` | `--audience adult` | Same layout when adult builder is ready |

Built decks still go in `bible-studies/` (flat, both audiences).

```bash
# One study
mzbs prepare --pdf "…" -n 22 --pages 7-9 --last --audience youth

# Batch
mzbs prepare --pdf "…" --from 23 --to 26 --audience youth

mzbs build studies/youth/22.json -o "bible-studies/22 - TITLE.pptx" --audience youth
```
