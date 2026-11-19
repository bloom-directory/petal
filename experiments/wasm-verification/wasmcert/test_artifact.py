"""Integration guards for accidental stale or mismatched artifact evidence."""
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import generate_artifact as embedding


class ArtifactBindingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for variant in ("correct", "faulty"):
            source = embedding.ROOT / "out" / variant
            if not (source / "extract/extraction.json").exists():
                self.skipTest("build-and-extract.sh must run first")
            target = self.root / "out" / variant
            (target / "extract").mkdir(parents=True)
            for relative in ("split-even.petal.tar.gz", "extract/extraction.json",
                             "extract/component.wasm", "extract/target-module.wasm"):
                shutil.copyfile(source / relative, target / relative)
        self.addCleanup(patch.stopall)
        patch.object(embedding, "ROOT", self.root).start()
        patch.object(embedding, "OUT", self.root / "out/wasmcert").start()

    def test_changed_module_is_rejected(self):
        module = self.root / "out/correct/extract/target-module.wasm"
        data = bytearray(module.read_bytes())
        data[-1] ^= 1
        module.write_bytes(data)
        with self.assertRaisesRegex(ValueError, "module hash mismatch"):
            embedding.generate()

    def test_stale_body_selection_is_rejected(self):
        path = self.root / "out/correct/extract/extraction.json"
        report = json.loads(path.read_text())
        report["target_function"]["body_offset_in_module"] += 1
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, "body hash mismatch"):
            embedding.generate()

    def test_component_report_cannot_override_packaged_bytes(self):
        component = self.root / "out/correct/extract/component.wasm"
        component.write_bytes(b"a different component")
        path = component.parent / "extraction.json"
        report = json.loads(path.read_text())
        report["component"]["sha256"] = embedding.sha(component.read_bytes())
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, "component bytes differ"):
            embedding.generate()


if __name__ == "__main__":
    unittest.main()
