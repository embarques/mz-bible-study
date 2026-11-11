# Adult Bible study preparation prompt

Copy the prompt below into Codex and attach the adult study scan.

Prepare the adult studies from the attached scan and verify the files yourself.
For this run, proceed without waiting for batch confirmation or my review.

1. Identify each study’s printed page boundaries, from its title page through
   the page marked `RESUMEN GENERAL`. Use the next title page for Próximo when
   available. Report the boundaries in your handoff.
2. Transcribe the required Spanish content verbatim into the standard
   `studies/adult/{N}/` section files. Read the columns in order, join text
   across columns and pages, preserve chapter and verse markers, and prepare
   the required scenic PNGs. Use imagery suited to Mount Zion's Pentecostal
   congregation. Choose scenes and objects that illustrate the lesson. Avoid
   stained-glass depictions, Catholic devotional imagery, cathedral interiors,
   and ornamental carved reliefs or mosaics. Give each study a distinct visual
   design and vary viewpoint and composition within the study. Choose dark
   navy or near-white slide text to contrast with the actual image behind it.
   Inspect every rendered scenic title and citation before delivery. On
   Definición y etimología slides, use the locked card design, illustrate each
   printed term, and omit unused rows entirely.
3. In a separate pass, compare the section files against the original scan.
   Check for omissions, altered words, incorrect citations, and broken joins.
   Re-read unclear areas at higher zoom and fix every discrepancy you can
   resolve from the scan.
4. Run `lamad validate-content -n N --audience adult` for each study. Fix any
   structural or artwork errors and rerun validation. If wording still cannot
   be verified, report the printed page and section and mark that study
   incomplete. Continue preparing the other studies.

Stop after preparing and validating the files. Leave PowerPoint and PDF
generation for a separate build.
