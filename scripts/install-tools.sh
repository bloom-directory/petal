#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec python3 - "$root" "$@" <<'PY'
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

root = Path(sys.argv[1])
parser = argparse.ArgumentParser(prog="install-tools.sh", description="Install the component tools pinned in Cargo.toml.")
parser.add_argument("--wasm-tools-only", action="store_true", help="skip the binding generator")
args = parser.parse_args(sys.argv[2:])


def exact_version(requirement):
    if not isinstance(requirement, str) or not re.fullmatch(r"=\d+\.\d+\.\d+", requirement):
        raise ValueError(f"expected an exact =major.minor.patch tool version, got {requirement!r}")
    return requirement[1:]


try:
    # Let Cargo parse the manifest without downloading or building dependencies.
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--offline", "--no-deps", "--format-version", "1",
        "--manifest-path", str(root / "Cargo.toml"),
    ], cwd=root, text=True))
    tools = [("wasm-tools", "wasm-tools", exact_version(metadata["metadata"]["tools"]["wasm-tools"]))]
    if not args.wasm_tools_only:
        sdk = next(package for package in metadata["packages"] if package["name"] == "bloom-petal-sdk")
        binding = next(dep for dep in sdk["dependencies"] if dep["name"] == "wit-bindgen")
        tools.append(("wit-bindgen-cli", "wit-bindgen", exact_version(binding["req"])))

    # Validate all pins before installing anything.
    for package, executable, version in tools:
        subprocess.run([
            "cargo", "install", package, "--version", f"={version}", "--locked",
        ], cwd=root, check=True)
        reported = subprocess.check_output([executable, "--version"], text=True).strip()
        if reported != f"{package} {version}":
            raise ValueError(
                f"expected {executable} {version} on PATH, got {reported!r}; "
                "put Cargo's installation bin directory first on PATH"
            )
        print(f"Verified {reported}", flush=True)
except (OSError, ValueError, KeyError, TypeError, StopIteration, subprocess.CalledProcessError) as error:
    sys.exit(f"install-tools: {error}")
PY
