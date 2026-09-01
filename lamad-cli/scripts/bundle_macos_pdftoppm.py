#!/usr/bin/env python3
"""Copy pdftoppm + Homebrew dylibs and rewrite install names so the zip is relocatable.

Usage: bundle_macos_pdftoppm.py /path/to/pdftoppm /path/to/tools
Produces: tools/pdftoppm + tools/lib/*.dylib
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

SYSTEM_PREFIXES = ("/usr/lib/", "/System/", "/Library/Apple/")


def run(cmd: list[str]) -> str:
    return subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL)


def otool_libs(path: Path) -> list[str]:
    out = run(["otool", "-L", str(path)])
    libs: list[str] = []
    for line in out.splitlines()[1:]:
        line = line.strip()
        if not line:
            continue
        name = line.split(" (compatibility")[0].strip()
        libs.append(name)
    return libs


def otool_rpaths(path: Path) -> list[str]:
    try:
        out = run(["otool", "-l", str(path)])
    except subprocess.CalledProcessError:
        return []
    rpaths: list[str] = []
    lines = out.splitlines()
    for i, line in enumerate(lines):
        if "cmd LC_RPATH" in line:
            for j in range(i, min(i + 6, len(lines))):
                if "path " in lines[j]:
                    # "         path @loader_path/../lib (offset 12)"
                    p = lines[j].strip().split("path ", 1)[1]
                    p = p.split(" (offset")[0].strip()
                    rpaths.append(p)
                    break
    return rpaths


def is_system(lib: str) -> bool:
    if lib.startswith(SYSTEM_PREFIXES):
        return True
    if lib.startswith("/usr/lib/libSystem") or lib.startswith("/usr/lib/libc++"):
        return True
    return False


def resolve(lib: str, loader: Path) -> Path | None:
    if lib.startswith("@rpath/"):
        rest = lib[len("@rpath/") :]
        for rp in otool_rpaths(loader):
            if rp.startswith("@loader_path"):
                base = loader.parent / rp.replace("@loader_path", ".", 1)
            elif rp.startswith("@executable_path"):
                base = loader.parent / rp.replace("@executable_path", ".", 1)
            else:
                base = Path(rp)
            cand = (base / rest).resolve()
            if cand.is_file():
                return cand
        # brew layout: binary in bin/, libs in ../lib
        cand = (loader.parent / ".." / "lib" / rest).resolve()
        if cand.is_file():
            return cand
        return None
    if lib.startswith("@loader_path/"):
        cand = (loader.parent / lib[len("@loader_path/") :]).resolve()
        return cand if cand.is_file() else None
    if lib.startswith("@executable_path/"):
        cand = (loader.parent / lib[len("@executable_path/") :]).resolve()
        return cand if cand.is_file() else None
    p = Path(lib)
    if p.is_file():
        return p.resolve()
    return None


def install_name_change(binary: Path, old: str, new: str) -> None:
    subprocess.run(
        ["install_name_tool", "-change", old, new, str(binary)],
        check=False,
        capture_output=True,
    )


def install_name_id(binary: Path, new_id: str) -> None:
    subprocess.run(
        ["install_name_tool", "-id", new_id, str(binary)],
        check=False,
        capture_output=True,
    )


def collect(exe: Path) -> dict[str, Path]:
    """Map basename -> real path of every non-system dylib needed."""
    needed: dict[str, Path] = {}
    queue = [exe]
    seen_files: set[Path] = set()
    while queue:
        cur = queue.pop()
        if cur in seen_files:
            continue
        seen_files.add(cur)
        for lib in otool_libs(cur):
            if is_system(lib):
                continue
            resolved = resolve(lib, cur)
            if resolved is None:
                print(f"warn: could not resolve {lib} from {cur}", file=sys.stderr)
                continue
            name = resolved.name
            # Prefer the versioned real file
            if resolved.is_symlink():
                resolved = resolved.resolve()
            if name not in needed:
                needed[name] = resolved
                queue.append(resolved)
            # also keep the symlink-style name used in otool (libpoppler.144.dylib)
            soname = Path(lib).name
            if soname.startswith("@") or "/" in soname:
                continue
            if soname not in needed:
                # copy under the name pdftoppm actually loads
                needed[soname] = resolved
    return needed


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: bundle_macos_pdftoppm.py SRC_PDFTO_PPM DEST_TOOLS_DIR", file=sys.stderr)
        return 2
    src = Path(sys.argv[1]).resolve()
    dest = Path(sys.argv[2]).resolve()
    dest.mkdir(parents=True, exist_ok=True)
    libdir = dest / "lib"
    libdir.mkdir(exist_ok=True)

    shutil.copy2(src, dest / "pdftoppm")
    os.chmod(dest / "pdftoppm", 0o755)

    needed = collect(src)
    for name, real in needed.items():
        target = libdir / name
        if not target.exists():
            shutil.copy2(real, target)
            print(f"  copied {name}")

    exe = dest / "pdftoppm"
    # Rewrite pdftoppm deps → @loader_path/lib/<name>
    for lib in otool_libs(exe):
        if is_system(lib):
            continue
        base = Path(lib).name
        if (libdir / base).exists() or any(p.name == base for p in libdir.iterdir()):
            install_name_change(exe, lib, f"@loader_path/lib/{base}")
        else:
            resolved = resolve(lib, src)
            if resolved:
                install_name_change(exe, lib, f"@loader_path/lib/{resolved.name}")

    # Rewrite each bundled dylib to look next to itself
    for dylib in libdir.glob("*.dylib"):
        install_name_id(dylib, f"@loader_path/{dylib.name}")
        for lib in otool_libs(dylib):
            if is_system(lib):
                continue
            base = Path(lib).name
            if (libdir / base).exists():
                install_name_change(dylib, lib, f"@loader_path/{base}")
            else:
                resolved = resolve(lib, src) or resolve(lib, dylib)
                if resolved and (libdir / resolved.name).exists():
                    install_name_change(dylib, lib, f"@loader_path/{resolved.name}")

    # Drop stale rpath that pointed at Homebrew ../lib
    subprocess.run(
        ["install_name_tool", "-delete_rpath", "@loader_path/../lib", str(exe)],
        check=False,
        capture_output=True,
    )
    subprocess.run(
        ["install_name_tool", "-add_rpath", "@loader_path/lib", str(exe)],
        check=False,
        capture_output=True,
    )

    # Smoke test
    r = subprocess.run([str(exe), "-v"], capture_output=True, text=True)
    if r.returncode != 0 and "poppler" not in (r.stderr + r.stdout).lower():
        # pdftoppm -v often prints to stderr and may exit 0 or 99; if dyld failed, abort
        if "Library not loaded" in (r.stderr + r.stdout) or "dyld" in (r.stderr + r.stdout):
            print(r.stderr or r.stdout, file=sys.stderr)
            return 1
    print("pdftoppm bundled OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
