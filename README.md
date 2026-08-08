# Monte de Sion Bible Study

Spanish Bible study PowerPoints for Monte de Sion.

## What you provide

### Option A — one study (unchanged)

```
1. [3 scans of the current study]
2. [1 scan of the next study’s title page]
```

Or a small PDF with just those pages.

### Option B — whole unit from one PDF (new)

Send **one PDF** that contains **every study in order**.

**Before generating:** the agent inspects the PDF and reports what it sees (page count, how studies split, estudio numbers/titles, which is last / no Próximo). **Wait for your OK** before building any decks.

Then the agent:

1. Extracts all pages
2. Splits into studies (**3 content pages each**, in sequence)
3. Builds **every** deck into `bible-studies/`

**How Próximo works in a batch PDF:**

| Study in the PDF | Próximo estudio |
|------------------|-----------------|
| Not the last | Taken from the **next** study’s title page (first page of the next 3-page block / title layout) |
| **Last** study | Only 3 pages — **no** next study → **omit** the Próximo slide |

Detail:

- **1** (single) — three content pages (any order; numbered). From a PDF, pages are extracted automatically.
- **2** (single) — title page of the *following* study (number, title, base bíblica)
- **Batch PDF** — studies back-to-back; next study starts when the previous one’s 3 pages end; last study ends the document (3 pages only)

If you skip **2** on a *single* study that still needs Próximo, you’ll be asked for number, title, and bible verses.

Optional: one short note if something spans pages oddly, e.g. `2A spans pages 96–98`.

## What you get

A finished deck in **`bible-studies/`**, named like this:

`bible-studies/17 - UN ABRAZO QUE DA VIDA.pptx`

plus a matching PDF:

`bible-studies/17 - UN ABRAZO QUE DA VIDA.pdf`

Open the PowerPoint and check that text doesn’t run into the logo.

## Folders

| Folder | What’s in it |
|--------|----------------|
| [`bible-studies/`](bible-studies/) | Generated presentations |
| [`template/`](template/) | Master template and format examples (don’t edit these unless you mean to change the system) |
# mz-bible-study
# mz-bible-study
