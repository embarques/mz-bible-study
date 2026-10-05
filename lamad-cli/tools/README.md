# Bundled `pdftoppm` (poppler)

Volunteer zips ship a working `pdftoppm` here so **prepare** does not need a
system poppler install.

## Layout (after `package-release`)

```
tools/
  pdftoppm          # or pdftoppm.exe on Windows
  *.dll             # Windows: poppler runtime DLLs (same folder)
  lib/              # macOS/Linux: shared libraries when needed
```

Point `config.toml` at it (default in `config.example.toml`):

```toml
pdftoppm_path = "tools/pdftoppm"
# Windows:
# pdftoppm_path = "tools/pdftoppm.exe"
```

Relative paths resolve against the app folder (`MZBS_ROOT` / directory of the
`lamad` binary). Change the value if you relocate this folder.

## Do not commit binaries

Poppler builds are large and OS-specific. This directory is filled by:

```bash
cd lamad-cli
./scripts/package-release.sh
```

Run that on (or for) each target OS to produce `dist/lamad-app-<os>-<arch>/`.

## Developer fallback

If `tools/` is empty, `lamad` still looks on `PATH` (e.g. `brew install poppler`).
