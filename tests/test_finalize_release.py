#!/usr/bin/env python3
"""Exercise release identity checks before the signing key is opened."""

import hashlib
import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REVISION = "a" * 40
VERSION = "1.2.3"


spec = importlib.util.spec_from_file_location("stage_release_docs", ROOT / "scripts/stage-release-docs.py")
documents = importlib.util.module_from_spec(spec)
spec.loader.exec_module(documents)


class FinalizeReleaseTests(unittest.TestCase):
    def run_finalize(self, changed_file=None, changed_field=None, changed_support=None, changed_provenance=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "package"
            (package / "bin").mkdir(parents=True)
            documents.stage(ROOT, package, VERSION)
            binary = package / "bin" / "xssc"
            catalog = package / "adapter-catalog.json"
            support = {"tool_version": VERSION, "source_revision": REVISION, "compiled_target": "x86_64-unknown-linux-gnu", "formal_release_target": "x86_64-unknown-linux-gnu"}
            if changed_support is not None: support[changed_support] = "wrong"
            # An explicit reporting stand-in tests publisher checks; the real
            # staged ELF and source binding are verified separately.
            binary.write_text("#!/usr/bin/python3\nimport sys\nprint(" + repr(json.dumps(support)) + " if sys.argv[1]=='support' else 'xssc " + VERSION + "')\n")
            binary.chmod(0o700)
            catalog.write_text(json.dumps(support))
            (package / "RELEASE-SIGNING-PUBLIC.pem").write_text("test key\n")
            metadata = {
                "product": "xssc",
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
            provenance = {"source_revision": REVISION, "subject_sha256": metadata["binary_sha256"], "target": metadata["target"]}
            if changed_provenance is not None: provenance[changed_provenance] = "wrong"
            (package / "provenance.json").write_text(json.dumps(provenance))
            if changed_file == "document":
                (package / "docs/xscs-protocol-preparation.md").unlink()
            elif changed_file == "binary":
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

    def test_missing_offline_preparation_document_is_rejected_before_signing(self):
        result = self.run_finalize(changed_file="document")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release documentation validation failed", result.stderr)
        self.assertNotIn("release signing key is unavailable", result.stderr)

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

    def test_rejects_unbound_or_wrong_compiled_source_target_and_provenance_before_signing(self):
        for field in ("source_revision", "compiled_target"):
            result = self.run_finalize(changed_support=field)
            self.assertIn(f"compiled binary {field} does not match", result.stderr)
            self.assertNotIn("release signing key is unavailable", result.stderr)
        for field in ("source_revision", "target", "subject_sha256"):
            result = self.run_finalize(changed_provenance=field)
            self.assertIn(f"provenance {field} does not match", result.stderr)
            self.assertNotIn("release signing key is unavailable", result.stderr)


if __name__ == "__main__":
    unittest.main()
