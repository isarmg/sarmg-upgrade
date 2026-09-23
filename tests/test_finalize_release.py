#!/usr/bin/env python3
"""Exercise release identity checks before the signing key is opened."""

import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REVISION = "a" * 40
VERSION = "1.2.3"


class FinalizeReleaseTests(unittest.TestCase):
    def run_finalize(self, changed_file=None, changed_field=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "package"
            (package / "bin").mkdir(parents=True)
            binary = package / "bin" / "sarmg-upgrade"
            catalog = package / "adapter-catalog.json"
            binary.write_bytes(b"verified binary")
            catalog.write_bytes(b"{}\n")
            (package / "RELEASE-SIGNING-PUBLIC.pem").write_text("test key\n")
            metadata = {
                "product": "sarmg-upgrade",
                "target": "x86_64-unknown-linux-gnu",
                "version": VERSION,
                "source_revision": REVISION,
                "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "catalog_sha256": hashlib.sha256(catalog.read_bytes()).hexdigest(),
                "release_signing_public_key_sha256": "b" * 64,
            }
            if changed_field is not None:
                metadata[changed_field] = "wrong"
            (package / "release.json").write_text(json.dumps(metadata))
            if changed_file == "binary":
                binary.write_bytes(b"changed binary")
            elif changed_file == "catalog":
                catalog.write_bytes(b"changed catalog")
            return subprocess.run(
                [
                    "bash",
                    str(ROOT / "scripts" / "finalize-release.sh"),
                    str(package),
                    str(root / "missing-private-key.pem"),
                    str(root / "output"),
                    REVISION,
                    VERSION,
                ],
                capture_output=True,
                text=True,
                check=False,
            )

    def test_valid_metadata_reaches_signing_key_check(self):
        result = self.run_finalize()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release signing key is unavailable", result.stderr)

    def test_rejects_tampered_release_identity_before_signing(self):
        for changed_file, changed_field, expected in (
            ("binary", None, "binary_sha256 does not match"),
            ("catalog", None, "catalog_sha256 does not match"),
            (None, "source_revision", "source revision does not match"),
            (None, "version", "version does not match"),
        ):
            with self.subTest(changed_file=changed_file, changed_field=changed_field):
                result = self.run_finalize(changed_file, changed_field)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected, result.stderr)
                self.assertNotIn("release signing key is unavailable", result.stderr)


if __name__ == "__main__":
    unittest.main()
