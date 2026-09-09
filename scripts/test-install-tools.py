"""Exercise installer pin selection and verification without downloading tools."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "pinned tooling"
        (self.root / "scripts").mkdir(parents=True)
        (self.root / "src").mkdir()
        (self.root / "src/lib.rs").touch()
        shutil.copy2(Path(__file__).with_name("install-tools.sh"), self.root / "scripts/install-tools.sh")
        self.manifest = self.root / "Cargo.toml"
        self.manifest.write_text('''[package]
name = "bloom-petal-sdk"
version = "0.1.0"
edition = "2021"
[workspace.metadata.tools]
wasm-tools = "=1.2.3"
[workspace.dependencies]
wit-bindgen = "=0.4.5"
[dependencies]
wit-bindgen.workspace = true
''')
        self.bin = Path(self.temp.name) / "bin"
        self.bin.mkdir()
        self.log = Path(self.temp.name) / "install.jsonl"
        self.env = dict(os.environ, REAL_CARGO=shutil.which("cargo"), INSTALL_LOG=str(self.log))
        self.env["PATH"] = str(self.bin) + os.pathsep + self.env["PATH"]
        cargo = self.bin / "cargo"
        cargo.write_text(f"#!{sys.executable}\n" + '''import json, os, pathlib, sys
if sys.argv[1] == "metadata":
    os.execv(os.environ["REAL_CARGO"], [os.environ["REAL_CARGO"], *sys.argv[1:]])
with open(os.environ["INSTALL_LOG"], "a") as log:
    log.write(json.dumps(sys.argv[1:]) + "\\n")
package = sys.argv[2]
version = os.environ.get("WRONG_VERSION", sys.argv[4].removeprefix("="))
name = "wit-bindgen" if package == "wit-bindgen-cli" else package
binary = pathlib.Path(__file__).with_name(name)
binary.write_text("#!/bin/sh\\nprintf '%s\\\\n' '" + package + " " + version + "'\\n")
binary.chmod(0o755)
''')
        cargo.chmod(0o755)
        # A caller's manifest must never determine the pinned tooling versions.
        self.caller = Path(self.temp.name) / "caller"
        self.caller.mkdir()
        (self.caller / "Cargo.toml").write_text("not valid TOML")

    def run_installer(self, *args):
        return subprocess.run([str(self.root / "scripts/install-tools.sh"), *args],
                              cwd=self.caller, env=self.env, text=True, capture_output=True)

    def installs(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_both_tools_use_the_installers_manifest(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.installs(), [
            ["install", "wasm-tools", "--version", "=1.2.3", "--locked"],
            ["install", "wit-bindgen-cli", "--version", "=0.4.5", "--locked"],
        ])

    def test_wasm_only_does_not_require_binding_dependency(self):
        self.manifest.write_text(self.manifest.read_text().replace("wit-bindgen.workspace = true", ""))
        result = self.run_installer("--wasm-tools-only")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.installs()), 1)
        self.assertEqual(self.installs()[0][1], "wasm-tools")

    def test_unpinned_versions_fail_before_installing(self):
        for version in ["=1.2.3", "=0.4.5"]:
            with self.subTest(version=version):
                original = self.manifest.read_text()
                self.manifest.write_text(original.replace(version, version[1:]))
                result = self.run_installer()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("expected an exact", result.stderr)
                self.assertFalse(self.log.exists())
                self.manifest.write_text(original)

    def test_wrong_executable_version_fails(self):
        self.env["WRONG_VERSION"] = "9.9.9"
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("on PATH", result.stderr)


if __name__ == "__main__":
    unittest.main()
