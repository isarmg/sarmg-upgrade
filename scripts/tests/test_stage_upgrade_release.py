import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'stage-upgrade-release.py'
spec = importlib.util.spec_from_file_location('stage_upgrade_release', SCRIPT)
stage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage)


class ReleaseStagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / 'server'
        shutil.copyfile('/usr/bin/true', self.binary)
        self.binary.chmod(0o700)
        self.private = self.root / 'private.pem'
        self.public = self.root / 'public.pem'
        stage.openssl('genpkey', '-algorithm', 'ED25519', '-out', self.private)
        self.private.chmod(0o600)
        stage.openssl('pkey', '-in', self.private, '-pubout', '-out', self.public)
        self.identity = self.root / 'identity.json'
        self.identity.write_text(json.dumps({
            'product': 'fixture-product', 'version': '2.0.0',
            'source_revision': 'b' * 40, 'target': 'x86_64-unknown-linux-gnu',
            'state_contract_sha256': 'c' * 64,
        }))
        self.schema = {
            'application': 'fixture-product', 'application_version': 'data-v1',
            'schema_revision': 1, 'schema_sha256': 'd' * 64,
        }
        self.definition = self.root / 'definition.json'
        self.write_definition()
        self.args = argparse.Namespace(
            binary=self.binary, identity=self.identity, definition=self.definition,
            private_key=self.private, trusted_public_key=self.public,
            output=self.root / 'package',
        )

    def write_definition(self, unrecognized=None, different=False):
        target = dict(self.schema)
        if different:
            target.update(application_version='data-v2', schema_revision=2, schema_sha256='e' * 64)
        definition = {'source_identity': json.loads(self.identity.read_bytes()), 'source_schema': self.schema, 'target_schema': target,
                      'resources': [{'name': 'data', 'kind': 'directory'}]}
        if unrecognized is not None:
            definition['unrecognized'] = unrecognized
        self.definition.write_text(json.dumps(definition))

    def test_signed_output_binds_artifact_and_rejects_modified_manifest(self):
        result = stage.stage(self.args)
        manifest = Path(result['manifest'])
        data = json.loads(manifest.read_bytes())
        self.assertEqual(data['binary_sha256'], hashlib.sha256(self.binary.read_bytes()).hexdigest())
        stage.openssl('pkeyutl', '-verify', '-pubin', '-inkey', self.public,
                      '-rawin', '-in', manifest, '-sigfile', result['signature'])
        manifest.write_bytes(manifest.read_bytes() + b' ')
        with self.assertRaises(ValueError):
            stage.openssl('pkeyutl', '-verify', '-pubin', '-inkey', self.public,
                          '-rawin', '-in', manifest, '-sigfile', result['signature'])

    def test_changed_contract_and_unrecognized_conversion_fields_are_rejected(self):
        self.write_definition(different=True)
        with self.assertRaises(ValueError): stage.stage(self.args)
        self.assertFalse(self.args.output.exists())
        self.write_definition({'protocol':'unrecognized','id':'undeclared'})
        with self.assertRaises(ValueError): stage.stage(self.args)
        self.assertFalse(self.args.output.exists())

    def test_signed_writer_roles_are_bounded_unique_and_not_command_hooks(self):
        self.write_definition()
        definition = json.loads(self.definition.read_bytes())
        definition['additional_service_roles'] = ['recording-writer']
        self.definition.write_text(json.dumps(definition))
        result = stage.stage(self.args)
        manifest = json.loads(Path(result['manifest']).read_bytes())
        self.assertEqual(manifest['additional_service_roles'], ['recording-writer'])
        for roles in (['writer'] * 2, ['writer'] * 17, ['/bin/sh'], [42]):
            self.args.output = self.root / 'rejected-package'
            definition['additional_service_roles'] = roles
            self.definition.write_text(json.dumps(definition))
            with self.assertRaises(ValueError): stage.stage(self.args)
            self.assertFalse(self.args.output.exists())

    def test_current_identity_is_required_bound_and_for_the_same_product_and_target(self):
        original = json.loads(self.definition.read_bytes())
        rejected = [dict(original, source_identity=None)]
        missing = dict(original)
        del missing['source_identity']
        rejected.append(missing)
        for field, value in [('product', 'other-product'), ('target', 'aarch64-unknown-linux-gnu'),
                             ('source_revision', 'unbound')]:
            identity = dict(original['source_identity'], **{field: value})
            rejected.append(dict(original, source_identity=identity))
        for definition in rejected:
            self.definition.write_text(json.dumps(definition))
            with self.assertRaises(ValueError): stage.stage(self.args)
            self.assertFalse(self.args.output.exists())

    def test_unbound_revision_or_untrusted_key_cannot_publish_a_package(self):
        value = json.loads(self.identity.read_bytes())
        value['source_revision'] = 'unbound'
        self.identity.write_text(json.dumps(value))
        with self.assertRaises(ValueError):
            stage.stage(self.args)
        self.assertFalse(self.args.output.exists())

        value['source_revision'] = 'b' * 40
        self.identity.write_text(json.dumps(value))
        other = self.root / 'other.pem'
        stage.openssl('genpkey', '-algorithm', 'ED25519', '-out', other)
        other.chmod(0o600)
        self.args.private_key = other
        with self.assertRaises(ValueError):
            stage.stage(self.args)
        self.assertFalse(self.args.output.exists())

    def complete_root(self):
        root = self.root / 'opt/isarmg/fixture/releases/2.0.0'
        (root / 'bin').mkdir(parents=True)
        shutil.copyfile(self.binary, root / 'bin/server')
        (root / 'bin/server').chmod(0o755)
        (root / 'web').mkdir()
        (root / 'web/index.html').write_text('original bundled web')
        (root / 'web/index.html').chmod(0o644)
        definition = json.loads(self.definition.read_bytes())
        definition['artifact'] = {'protocol': 'immutable-release-root-v1',
                                  'root_layout': 'opt/isarmg/fixture/releases/2.0.0',
                                  'entrypoint': 'bin/server'}
        self.definition.write_text(json.dumps(definition))
        self.args.binary = root / 'bin/server'
        self.args.release_root = root
        return root

    def test_complete_root_signs_exact_product_modes_and_all_assets(self):
        root = self.complete_root()
        result = stage.stage(self.args)
        artifact = json.loads(Path(result['manifest']).read_bytes())['artifact']
        entries = stage.artifact_inventory(root, 1024 ** 4)
        self.assertEqual(artifact['tree_sha256'], hashlib.sha256(
            b'immutable-release-root-v1\n' + json.dumps(entries, ensure_ascii=False, separators=(',', ':')).encode()).hexdigest())
        self.assertEqual(next(entry['mode'] for entry in entries if entry['path'] == 'web/index.html'), 0o644)
        (root / 'web/index.html').write_text('changed bundled web')
        changed = stage.artifact_contract(json.loads(self.definition.read_bytes()), self.args.binary, root, '2.0.0', 1024 ** 4)
        self.assertNotEqual(artifact['tree_sha256'], changed['tree_sha256'])

    def test_root_diagnostic_environment_is_signed_and_has_no_general_environment_hook(self):
        self.complete_root()
        definition = json.loads(self.definition.read_bytes())
        definition['artifact']['diagnostic_release_root_env'] = 'FIXTURE_RELEASE_ROOT'
        self.definition.write_text(json.dumps(definition))
        manifest = json.loads(Path(stage.stage(self.args)['manifest']).read_bytes())
        self.assertEqual(manifest['artifact']['diagnostic_release_root_env'], 'FIXTURE_RELEASE_ROOT')
        self.args.output = self.root / 'rejected-package'
        for name in ('PATH', 'LD_PRELOAD', 'bad_RELEASE_ROOT', 'X_RELEASE_ROOT=bad', 'A' * 65 + '_RELEASE_ROOT'):
            definition['artifact']['diagnostic_release_root_env'] = name
            self.definition.write_text(json.dumps(definition))
            with self.assertRaises(ValueError): stage.stage(self.args)
            self.assertFalse(self.args.output.exists())

    def test_complete_root_refuses_links_other_writes_unknown_fields_and_missing_root(self):
        root = self.complete_root()
        self.args.release_root = None
        with self.assertRaises(ValueError): stage.stage(self.args)
        self.args.release_root = root
        (root / 'web/link').symlink_to('index.html')
        with self.assertRaises(ValueError): stage.stage(self.args)
        (root / 'web/link').unlink()
        (root / 'web/index.html').chmod(0o664)
        with self.assertRaises(ValueError): stage.stage(self.args)
        (root / 'web/index.html').chmod(0o644)
        definition = json.loads(self.definition.read_bytes())
        definition['artifact']['hook'] = '/bin/sh'
        self.definition.write_text(json.dumps(definition))
        with self.assertRaises(ValueError): stage.stage(self.args)
        self.assertFalse(self.args.output.exists())



if __name__ == '__main__':
    unittest.main()
