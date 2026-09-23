#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 5 ]]; then
  echo "usage: finalize-release.sh PACKAGE_DIRECTORY ED25519_PRIVATE_KEY OUTPUT_DIRECTORY EXPECTED_REVISION EXPECTED_VERSION" >&2
  exit 64
fi
package=$1
private_key=$2
output=$3
expected_revision=$4
expected_version=$5

release_metadata=$package/release.json
if [[ ! -f $release_metadata ]]; then
  echo "staged release metadata is unavailable" >&2
  exit 1
fi
expected_public_key=$package/RELEASE-SIGNING-PUBLIC.pem
if [[ ! -f $expected_public_key ]]; then
  echo "source-bound release signing public key is unavailable" >&2
  exit 1
fi
release_identity_text=$(python3 - "$release_metadata" "$expected_revision" "$expected_version" <<'PY'
import hashlib, json, pathlib, sys

metadata_path = pathlib.Path(sys.argv[1])
value = json.loads(metadata_path.read_text(encoding="utf-8"))
if value.get("product") != "sarmg-upgrade":
    raise SystemExit("release metadata names the wrong product")
if value.get("target") != "x86_64-unknown-linux-gnu":
    raise SystemExit("release metadata does not name the sole supported release target")
version = value.get("version")
if version != sys.argv[3]:
    raise SystemExit("release metadata version does not match the release tag")
if value.get("source_revision") != sys.argv[2]:
    raise SystemExit("release metadata source revision does not match the event commit")
for field, relative_path in (
    ("binary_sha256", "bin/sarmg-upgrade"),
    ("catalog_sha256", "adapter-catalog.json"),
):
    expected = value.get(field)
    if not isinstance(expected, str) or len(expected) != 64 or any(
        character not in "0123456789abcdef" for character in expected
    ):
        raise SystemExit(f"release metadata {field} is invalid")
    path = metadata_path.parent / relative_path
    if path.is_symlink() or not path.is_file():
        raise SystemExit(f"release package {relative_path} is not a regular file")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    if digest.hexdigest() != expected:
        raise SystemExit(f"release metadata {field} does not match {relative_path}")
print(version)
print(value["target"])
fingerprint = value.get("release_signing_public_key_sha256")
if not isinstance(fingerprint, str) or len(fingerprint) != 64 or any(
    character not in "0123456789abcdef" for character in fingerprint
):
    raise SystemExit("release signing public key fingerprint is invalid")
print(fingerprint)
PY
)
readarray -t release_identity <<<"$release_identity_text"
version=${release_identity[0]}
target=${release_identity[1]}
expected_public_key_sha=${release_identity[2]}
archive="sarmg-upgrade-${version}-linux-x86_64.tar.zst"

if [[ -e $output ]]; then
  echo "final release output already exists" >&2
  exit 1
fi
if [[ ! -f $private_key ]]; then
  echo "release signing key is unavailable" >&2
  exit 1
fi
umask 077
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
derived_public_key=$temporary/derived-release-signing-public.pem
openssl pkey -in "$private_key" -pubout -out "$derived_public_key"
if ! cmp -s "$expected_public_key" "$derived_public_key"; then
  echo "release signing private key does not match the source-bound public key" >&2
  exit 1
fi
actual_public_key_sha=$(
  openssl pkey -pubin -in "$expected_public_key" -outform DER \
    | sha256sum \
    | awk '{print $1}'
)
if [[ $actual_public_key_sha != "$expected_public_key_sha" ]]; then
  echo "release signing public key does not match release metadata" >&2
  exit 1
fi
mkdir "$output"
(
  cd "$package"
  find . -type f ! -name SHA256SUMS ! -name SHA256SUMS.sig -print0 \
    | sort -z \
    | xargs -0 sha256sum >SHA256SUMS
)
openssl pkeyutl -sign -rawin -inkey "$private_key" \
  -in "$package/SHA256SUMS" -out "$package/SHA256SUMS.sig"
openssl pkeyutl -verify -rawin -pubin -inkey "$package/RELEASE-SIGNING-PUBLIC.pem" \
  -in "$package/SHA256SUMS" -sigfile "$package/SHA256SUMS.sig"
tar --sort=name --mtime='UTC 2020-01-01' --owner=0 --group=0 --numeric-owner \
  --mode='u+rwX,go+rX,go-w' -C "$package" -cf - . | zstd -19 -T0 -o "$output/$archive"
(cd "$output" && sha256sum "$archive" >"$archive.sha256")

extracted=$temporary/extracted
mkdir "$extracted"
zstd -dc "$output/$archive" | tar -xf - -C "$extracted"
(cd "$extracted" && sha256sum --check SHA256SUMS)
cmp "$expected_public_key" "$extracted/RELEASE-SIGNING-PUBLIC.pem"
openssl pkeyutl -verify -rawin -pubin -inkey "$expected_public_key" \
  -in "$extracted/SHA256SUMS" -sigfile "$extracted/SHA256SUMS.sig"
cmp "$package/adapter-catalog.json" "$extracted/adapter-catalog.json"
"$extracted/bin/sarmg-upgrade" support --json >"$temporary/actual-support.json"
cmp "$extracted/adapter-catalog.json" "$temporary/actual-support.json"
test "$target" = x86_64-unknown-linux-gnu
