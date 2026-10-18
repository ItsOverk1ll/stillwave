#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Build a plugin crate to WASM and produce a signed package directory.

usage: package_plugin.py <plugin-crate-dir> <out-dir> <secret-key-file> [<public-key-file>]

Needs the wasm32-unknown-unknown Rust target. The crate must be a cdylib with a
manifest.toml next to its Cargo.toml. The public key, when given, is used to
verify the result. Keys come from `stillwave-plugin-tool keygen` (local dev) or a
CI-only secret; they are never committed.
"""
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def run(*args, cwd=None):
    subprocess.run([str(a) for a in args], check=True, cwd=cwd)


def main():
    if len(sys.argv) not in (4, 5):
        sys.exit(__doc__)
    crate, out, secret = (Path(a).resolve() for a in sys.argv[1:4])
    public = Path(sys.argv[4]).resolve() if len(sys.argv) == 5 else None
    sdk = ROOT / "plugin-sdk"
    pkg = tomllib.loads((crate / "Cargo.toml").read_text(encoding="utf-8"))["package"]["name"]
    name = pkg.replace("-", "_")
    run("cargo", "build", "--release", "--target", "wasm32-unknown-unknown",
        "-p", pkg, cwd=sdk)
    wasm = sdk / "target" / "wasm32-unknown-unknown" / "release" / f"{name}.wasm"
    out.mkdir(parents=True, exist_ok=True)
    shutil.copy(wasm, out / "plugin.wasm")
    shutil.copy(crate / "manifest.toml", out / "manifest.toml")
    tool = ["cargo", "run", "-q", "-p", "stillwave-plugins", "--bin", "stillwave-plugin-tool", "--"]
    run(*tool, "sign", out, secret, cwd=ROOT)
    if public:
        run(*tool, "verify", out, public, cwd=ROOT)
    print(f"package ready: {out}")


if __name__ == "__main__":
    main()
