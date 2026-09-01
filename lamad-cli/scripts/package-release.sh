#!/usr/bin/env bash
# Build a volunteer-ready lamad-app folder with binary + bundled pdftoppm + template.
#
# Usage (run on the target OS, or use --os for Windows poppler when cross-packaging):
#   ./scripts/package-release.sh
#   ./scripts/package-release.sh --os macos|linux|windows
#   ./scripts/package-release.sh --skip-build   # reuse target/release/lamad
#
# Output: dist/lamad-app-<os>-<arch>/ and a .zip beside it.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$ROOT/.." && pwd)"
DIST="$ROOT/dist"
SKIP_BUILD=0
OS_OVERRIDE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --os) OS_OVERRIDE="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,12p' "$0"
      exit 0
      ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

detect_os() {
  case "$(uname -s)" in
    Darwin) echo macos ;;
    Linux) echo linux ;;
    MINGW*|MSYS*|CYGWIN*) echo windows ;;
    *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    arm64|aarch64) echo aarch64 ;;
    x86_64|amd64) echo x86_64 ;;
    *) echo "$(uname -m)" ;;
  esac
}

OS="${OS_OVERRIDE:-$(detect_os)}"
ARCH="$(detect_arch)"
APP_NAME="lamad-app-${OS}-${ARCH}"
STAGE="$DIST/$APP_NAME"

echo "==> Packaging $APP_NAME"

# --- build ---
if [[ "$SKIP_BUILD" -eq 0 ]]; then
  if [[ "$OS" == "windows" && "$(detect_os)" != "windows" ]]; then
    echo "Cross-building Windows binary is not automated here."
    echo "Build on Windows (or with a Windows target toolchain), then re-run with --skip-build"
    echo "after placing target/release/lamad.exe in place — or run this script on Windows."
    exit 1
  fi
  echo "==> cargo build --release"
  (cd "$ROOT" && cargo build --release)
fi

BIN_SRC="$ROOT/target/release/lamad"
BIN_NAME="lamad"
PDFTO_PPM_NAME="pdftoppm"
EXPORT_PDF="true"
if [[ "$OS" == "windows" ]]; then
  BIN_NAME="lamad.exe"
  PDFTO_PPM_NAME="pdftoppm.exe"
  EXPORT_PDF="false"
  if [[ -f "$ROOT/target/release/lamad.exe" ]]; then
    BIN_SRC="$ROOT/target/release/lamad.exe"
  elif [[ -f "$ROOT/target/x86_64-pc-windows-msvc/release/lamad.exe" ]]; then
    BIN_SRC="$ROOT/target/x86_64-pc-windows-msvc/release/lamad.exe"
  elif [[ -f "$ROOT/target/x86_64-pc-windows-gnu/release/lamad.exe" ]]; then
    BIN_SRC="$ROOT/target/x86_64-pc-windows-gnu/release/lamad.exe"
  fi
fi

if [[ ! -f "$BIN_SRC" ]]; then
  echo "Missing release binary: $BIN_SRC" >&2
  echo "Run cargo build --release (on the target OS) first." >&2
  exit 1
fi

if [[ "$OS" != "macos" ]]; then
  EXPORT_PDF="false"
fi

# --- stage tree ---
rm -rf "$STAGE"
mkdir -p "$STAGE/tools" "$STAGE/scans/complete" "$STAGE/scans/error" \
  "$STAGE/studies/youth/media" "$STAGE/bible-studies" \
  "$STAGE/template/youth"

cp "$BIN_SRC" "$STAGE/$BIN_NAME"
chmod +x "$STAGE/$BIN_NAME" 2>/dev/null || true

# Template: prefer crate copy, else monorepo
if [[ -f "$ROOT/template/youth/master-template.pptx" ]]; then
  cp "$ROOT/template/youth/master-template.pptx" "$STAGE/template/youth/"
elif [[ -f "$REPO/template/youth/master-template.pptx" ]]; then
  cp "$REPO/template/youth/master-template.pptx" "$STAGE/template/youth/"
else
  echo "ERROR: master-template.pptx not found" >&2
  exit 1
fi

cp "$ROOT/config.example.toml" "$STAGE/config.example.toml"
cp "$ROOT/LEEME.md" "$STAGE/LEEME.md"
cp "$ROOT/README.md" "$STAGE/README.md"
cp "$ROOT/tools/README.md" "$STAGE/tools/README.md"
touch "$STAGE/scans/complete/.gitkeep" "$STAGE/scans/error/.gitkeep"

# --- config.toml for volunteers ---
cat > "$STAGE/config.toml" <<EOF
# Paste your Cursor API key (https://cursor.com/dashboard/api)
cursor_api_key = ""
cursor_model = "composer-2.5"
audience = "youth"
export_pdf = ${EXPORT_PDF}
pdftoppm_path = "tools/${PDFTO_PPM_NAME}"
EOF

# --- bundle pdftoppm ---
bundle_macos() {
  local dest="$1"
  local brew_bin
  brew_bin="$(command -v pdftoppm || true)"
  if [[ -z "$brew_bin" ]]; then
    echo "Install poppler first: brew install poppler" >&2
    exit 1
  fi
  echo "==> Bundling macOS pdftoppm from $brew_bin"
  python3 "$ROOT/scripts/bundle_macos_pdftoppm.py" "$brew_bin" "$dest"
}

bundle_linux() {
  local dest="$1"
  local sys
  sys="$(command -v pdftoppm || true)"
  if [[ -z "$sys" ]]; then
    echo "Install poppler-utils first (e.g. apt install poppler-utils)" >&2
    exit 1
  fi
  echo "==> Bundling Linux pdftoppm from $sys"
  cp "$sys" "$dest/pdftoppm"
  chmod +x "$dest/pdftoppm"
  mkdir -p "$dest/lib"
  if command -v ldd >/dev/null; then
    while read -r line; do
      local lib
      lib="$(echo "$line" | awk '{print $3}')"
      [[ -f "$lib" ]] || continue
      [[ "$lib" == /lib/* || "$lib" == /lib64/* || "$lib" == /usr/lib/* ]] || continue
      # Skip glibc core
      local base
      base="$(basename "$lib")"
      case "$base" in
        libc.so*|libm.so*|libdl.so*|libpthread.so*|ld-linux*) continue ;;
      esac
      cp -n "$lib" "$dest/lib/" 2>/dev/null || cp "$lib" "$dest/lib/"
    done < <(ldd "$sys")
  fi
}

bundle_windows() {
  local dest="$1"
  local ver="${POPPLER_WINDOWS_VERSION:-25.07.0-0}"
  local url="https://github.com/oschwartz10612/poppler-windows/releases/download/v${ver}/Release-${ver}.zip"
  local tmp
  tmp="$(mktemp -d)"
  echo "==> Downloading Windows poppler $ver"
  if command -v curl >/dev/null; then
    curl -fsSL -o "$tmp/poppler.zip" "$url"
  else
    wget -q -O "$tmp/poppler.zip" "$url"
  fi
  mkdir -p "$tmp/extract"
  unzip -q "$tmp/poppler.zip" -d "$tmp/extract"
  local bin_dir
  bin_dir="$(find "$tmp/extract" -type d -name bin | head -1)"
  if [[ -z "$bin_dir" || ! -f "$bin_dir/pdftoppm.exe" ]]; then
    echo "Could not find pdftoppm.exe in poppler zip" >&2
    exit 1
  fi
  # Copy pdftoppm + all DLLs from bin (runtime)
  cp "$bin_dir/pdftoppm.exe" "$dest/"
  find "$bin_dir" -maxdepth 1 -name '*.dll' -exec cp {} "$dest/" \;
  rm -rf "$tmp"
}

case "$OS" in
  macos) bundle_macos "$STAGE/tools" ;;
  linux) bundle_linux "$STAGE/tools" ;;
  windows) bundle_windows "$STAGE/tools" ;;
  *) echo "Unknown --os $OS" >&2; exit 1 ;;
esac

# Smoke: binary --help and bundled pdftoppm
"$STAGE/$BIN_NAME" --help >/dev/null || true
if [[ "$OS" != "windows" && -x "$STAGE/tools/$PDFTO_PPM_NAME" ]]; then
  "$STAGE/tools/$PDFTO_PPM_NAME" -v >/dev/null || {
    echo "ERROR: bundled pdftoppm failed to run" >&2
    exit 1
  }
fi

# Zip
(
  cd "$DIST"
  rm -f "${APP_NAME}.zip"
  if command -v zip >/dev/null; then
    zip -qr "${APP_NAME}.zip" "$APP_NAME"
  else
    tar -czf "${APP_NAME}.tar.gz" "$APP_NAME"
  fi
)

echo ""
echo "OK: $STAGE"
ls -la "$DIST" | sed -n "1,20p"
echo ""
echo "Volunteer steps:"
echo "  1. Unzip and edit config.toml → paste cursor_api_key"
echo "  2. Drop scan PDF into scans/"
echo "  3. ./$BIN_NAME prepare --from N --to M"
echo "  (Set MZBS_ROOT to the unzipped folder if needed)"
