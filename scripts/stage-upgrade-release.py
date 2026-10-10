#!/usr/bin/env python3
"""Sign the exact offline upgrade contract from a tool-owned product definition.

This is the shared packaging entry point. It does not infer historical formats,
rename schema identities, generate signing keys or choose a trust anchor.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess


def regular(path, limit=None, private=False):
    path = Path(path)
    if not path.is_absolute() or path.resolve() != path:
        raise ValueError("paths must be absolute and contain no symbolic links")
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
        raise ValueError("expected a single-link regular input file")
    if private and (metadata.st_uid != os.geteuid() or metadata.st_mode & 0o077):
        raise ValueError("signing key must be private and owned by the packaging user")
    if limit is not None and metadata.st_size > limit:
        raise ValueError("input exceeds the contract size limit")
    return path


def openssl(*arguments):
    completed = subprocess.run(
        ["/usr/bin/openssl", *map(str, arguments)],
        stdin=subprocess.DEVNULL, capture_output=True, timeout=30, check=False,
    )
    if completed.returncode:
        raise ValueError("OpenSSL key/signature operation failed")
    return completed.stdout


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def sha256(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
        raise ValueError("invalid SHA-256")


def identifier(value):
    if not isinstance(value, str) or not value or len(value) > 255 or value.strip() != value:
        raise ValueError("invalid contract identifier")


def schema(value):
    if set(value) != {"application", "application_version", "schema_revision", "schema_sha256"}:
        raise ValueError("invalid schema-identity fields")
    identifier(value["application"])
    identifier(value["application_version"])
    revision = value["schema_revision"]
    if type(revision) is not int or not 0 <= revision <= 9_007_199_254_740_991:
        raise ValueError("invalid schema revision")
    sha256(value["schema_sha256"])


def relative(value):
    if not isinstance(value, str) or not value or len(value.encode()) > 4096:
        raise ValueError("invalid immutable release relative path")
    parts = value.split("/")
    if any(part in ("", ".", "..") for part in parts) or Path(value).is_absolute():
        raise ValueError("release paths must be normalized relative paths")
    return Path(value)


def artifact_inventory(root, max_bytes):
    root = Path(root)
    if not root.is_absolute() or root.resolve() != root:
        raise ValueError("release root must be a physical absolute directory")
    owner = root.lstat().st_uid
    if owner not in (0, os.geteuid()):
        raise ValueError("release root belongs to another user")
    entries, total = [], 0

    def visit(path, name, depth):
        nonlocal total
        if depth > 128 or len(entries) >= 2_000_000:
            raise ValueError("release inventory exceeds entry/depth limits")
        before = path.lstat()
        mode = stat.S_IMODE(before.st_mode)
        directory = stat.S_ISDIR(before.st_mode)
        if before.st_uid != owner or mode & 0o7022 or not (directory or stat.S_ISREG(before.st_mode)):
            raise ValueError("release must contain one owner's protected regular files and directories")
        if not directory and before.st_nlink != 1:
            raise ValueError("release file has multiple links")
        total += 0 if directory else before.st_size
        if total > max_bytes:
            raise ValueError("release inventory exceeds byte limit")
        entries.append({"path": name, "directory": directory, "mode": mode,
                        "bytes": 0 if directory else before.st_size,
                        "sha256": "" if directory else digest(regular(path))})
        if directory:
            children = []
            with os.scandir(path) as iterator:
                for child in iterator:
                    if len(entries) + len(children) >= 2_000_000:
                        raise ValueError("release inventory exceeds entry limit")
                    children.append(child.name)
            for child in sorted(children):
                visit(path / child, child if not name else name + "/" + child, depth + 1)
        after = path.lstat()
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError("release changed during verification")

    visit(root, "", 0)
    return entries


def artifact_contract(definition, binary, root, version, max_bytes):
    artifact = definition.get("artifact")
    if artifact is None:
        if root is not None:
            raise ValueError("release root requires an explicit product artifact definition")
        return None
    required = {"protocol", "root_layout", "entrypoint"}
    if not required <= set(artifact) or not set(artifact) <= required | {"tree_sha256", "diagnostic_release_root_env"} or artifact["protocol"] != "immutable-release-root-v1":
        raise ValueError("unsupported complete release definition")
    if "diagnostic_release_root_env" in artifact:
        name = artifact["diagnostic_release_root_env"]
        if not isinstance(name, str) or not re.fullmatch(r'[A-Z][A-Z0-9_]*_RELEASE_ROOT', name) or len(name) > 64:
            raise ValueError("unsupported release-root diagnostic environment name")
    if root is None:
        raise ValueError("complete release definition requires --release-root")
    layout = relative(artifact["root_layout"])
    entrypoint = relative(artifact["entrypoint"])
    if layout.name != version or not Path(root).as_posix().endswith("/" + layout.as_posix()) or Path(root) / entrypoint != binary:
        raise ValueError("binary and complete root must preserve the product's exact versioned layout")
    entries = artifact_inventory(root, max_bytes)
    checksum = hashlib.sha256(b"immutable-release-root-v1\n" + json.dumps(entries, ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()
    if "tree_sha256" in artifact and artifact["tree_sha256"] != checksum:
        raise ValueError("declared complete release checksum differs")
    return {**artifact, "tree_sha256": checksum}


def release_identity(value):
    if not isinstance(value, dict) or set(value) != {"product", "version", "source_revision", "target", "state_contract_sha256"}:
        raise ValueError("release identity must use the supported contract")
    for field in ("product", "version", "target"):
        identifier(value[field])
    if value["target"] != "x86_64-unknown-linux-gnu":
        raise ValueError("unsupported release target")
    if not isinstance(value["source_revision"], str) or not re.fullmatch(r"[0-9a-f]{40}", value["source_revision"]):
        raise ValueError("release source revision must identify one controlled commit")
    sha256(value["state_contract_sha256"])


def stage(args):
    binary = regular(args.binary)
    with binary.open("rb") as stream:
        header = stream.read(20)
    if len(header) != 20 or header[:6] != b"\x7fELF\x02\x01" or header[18:20] != b"\x3e\x00":
        raise ValueError("binary must be Linux AMD64 ELF")
    identity = json.loads(regular(args.identity, 1024 * 1024).read_bytes())
    release_identity(identity)
    definition = json.loads(regular(args.definition, 1024 * 1024).read_bytes())
    if not {"source_identity", "source_schema", "target_schema", "resources"} <= set(definition) or not set(definition) <= {"source_identity", "source_schema", "target_schema", "resources", "artifact", "additional_service_roles"}:
        raise ValueError("invalid tool-owned upgrade-definition fields")
    release_identity(definition["source_identity"])
    if any(definition["source_identity"][field] != identity[field] for field in ("product", "target")):
        raise ValueError("signed source and target release product/target differ")
    for field in ("source_schema", "target_schema"):
        schema(definition[field])
        if definition[field]["application"] != identity["product"]:
            raise ValueError("product and schema identity disagree")
    if definition["source_schema"] != definition["target_schema"]:
        raise ValueError("a future schema change requires that version's explicit implementation")
    artifact = artifact_contract(definition, binary, getattr(args, "release_root", None), identity["version"], getattr(args, "max_artifact_bytes", 1024 ** 4))
    if artifact is not None:
        definition["artifact"] = artifact
    resources = definition["resources"]
    if not isinstance(resources, list) or not 1 <= len(resources) <= 128:
        raise ValueError("invalid persistent resource inventory")
    names = set()
    for resource in resources:
        if set(resource) != {"name", "kind"} or resource["kind"] not in ("file", "directory"):
            raise ValueError("invalid persistent resource definition")
        if not re.fullmatch(r"[A-Za-z0-9_-]{1,100}", resource["name"]) or resource["name"] in names:
            raise ValueError("persistent resource names must be unique")
        names.add(resource["name"])
    roles = definition.get("additional_service_roles", [])
    if not isinstance(roles, list) or len(roles) > 16 or any(not isinstance(role, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", role) for role in roles) or len(set(roles)) != len(roles):
        raise ValueError("invalid additional writer roles")
    private = regular(args.private_key, 8192, private=True)
    public = regular(args.trusted_public_key, 8192)
    derived = openssl("pkey", "-in", private, "-pubout", "-outform", "DER")
    trusted = openssl("pkey", "-pubin", "-in", public, "-outform", "DER")
    if derived != trusted or derived[:12] != bytes.fromhex("302a300506032b6570032100") or len(derived) != 44:
        raise ValueError("private key must match the independently source-bound Ed25519 public key")
    output = Path(args.output)
    if not output.is_absolute() or output.parent.resolve() != output.parent:
        raise ValueError("output must be an absolute path with a trusted parent")
    output.mkdir(mode=0o700)
    manifest = {
        "manifest_version": 1, "identity": identity, "binary_sha256": digest(binary),
        **definition,
    }
    manifest_path = output / "upgrade-release.json"
    with manifest_path.open("xb") as stream:
        stream.write((json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode())
        stream.flush()
        os.fsync(stream.fileno())
    signature_path = output / "upgrade-release.sig"
    openssl("pkeyutl", "-sign", "-inkey", private, "-rawin", "-in", manifest_path, "-out", signature_path)
    openssl("pkeyutl", "-verify", "-pubin", "-inkey", public, "-rawin", "-in", manifest_path, "-sigfile", signature_path)
    with signature_path.open("rb") as stream:
        os.fsync(stream.fileno())
    descriptor = os.open(output, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    return {"manifest": str(manifest_path), "signature": str(signature_path), "trusted_public_key_sha256": hashlib.sha256(trusted).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("binary", "identity", "definition", "private-key", "trusted-public-key", "output"):
        parser.add_argument("--" + option, required=True, type=Path)
    parser.add_argument("--release-root", type=Path)
    parser.add_argument("--max-artifact-bytes", type=int, default=1024 ** 4)
    args = parser.parse_args()
    try:
        result = stage(args)
    except (ValueError, OSError, subprocess.SubprocessError, KeyError, TypeError) as error:
        parser.exit(1, "upgrade release staging failed: " + str(error) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
