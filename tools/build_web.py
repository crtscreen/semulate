#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]
SUPPORTED = {0, 1, 2, 3, 4, 7, 218}

def catalogue(rom_dir, destination):
    metadata_path = rom_dir / "catalogue.json"
    metadata = json.loads(metadata_path.read_text()) if metadata_path.exists() else {}
    items = []
    destination.mkdir(parents=True, exist_ok=True)
    for path in sorted(rom_dir.glob("*"), key=lambda p: p.name.casefold()):
        if path.suffix.lower() != ".nes" or not path.is_file() or path.is_symlink():
            continue
        if path.stat().st_size > 16 * 1024 * 1024:
            raise ValueError(f"{path.name}: exceeds the 16 MiB ROM limit")
        data = path.read_bytes()
        if len(data) < 16 or data[:4] != b"NES\x1a":
            raise ValueError(f"{path.name}: not an iNES ROM")
        mapper = (data[6] >> 4) | (data[7] & 0xF0)
        if data[7] & 12 == 8:
            mapper |= (data[8] & 15) << 8
        info = metadata.get(path.name, {})
        items.append({
            "file": path.name,
            "url": "roms/" + quote(path.name, safe=""),
            "title": info.get("title", path.stem.replace("_", " ")),
            "description": info.get("description", "Homebrew cartridge"),
            "image": info.get("image"),
            "mapper": mapper,
            "supported": mapper in SUPPORTED,
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        })
        shutil.copy2(path, destination / path.name)
    return items

def build(skip_build=False):
    if not skip_build:
        cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
        subprocess.run([
            cargo, "build", "--locked", "--offline", "--release", "--lib",
            "--no-default-features", "--features", "web", "--target", "wasm32-unknown-unknown",
            "--target-dir", str(ROOT / "target"),
        ], cwd=ROOT, check=True)
    wasm = ROOT / "target/wasm32-unknown-unknown/release/semulate.wasm"
    if not wasm.is_file():
        raise FileNotFoundError("Missing WebAssembly build. Run rustup target add wasm32-unknown-unknown, then build again.")
    # stage before replacing a previous working dist
    with tempfile.TemporaryDirectory(prefix="semulate-web-") as temp:
        stage = Path(temp) / "dist"
        shutil.copytree(ROOT / "web", stage)
        items = catalogue(ROOT / "roms", stage / "roms")
        (stage / "roms.json").write_text(json.dumps(items, indent=2, ensure_ascii=False) + "\n")
        shutil.copy2(wasm, stage / "semulate.wasm")
        out = ROOT / "dist"
        if out.exists():
            shutil.rmtree(out)
        shutil.copytree(stage, out)
    print(f"Built dist/ with {len(items)} ROM(s), WebAssembly and all local assets.")
    print("Preview: python3 -m http.server 8080 --directory dist")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true", help="Reuse an already compiled WebAssembly core")
    args = parser.parse_args()
    build(args.skip_build)