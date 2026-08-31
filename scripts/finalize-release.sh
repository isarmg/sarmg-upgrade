#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: finalize-release.sh PACKAGE_DIRECTORY ED25519_PRIVATE_KEY OUTPUT_DIRECTORY" >&2
  exit 64
fi
package=$1
private_key=$2
output=$3

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
readarray -t release_identity < <(python3 - "$release_metadata" <<'PY'
import json, pathlib, sys

value = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
if value.get("product") != "sarmg-upgrade":
    raise SystemExit("release metadata names the wrong product")
if value.get("target") != "x86_64-unknown-linux-gnu":
    raise SystemExit("release metadata does not name the sole supported release target")
version = value.get("version")
if not isinstance(version, str) or not version:
    raise SystemExit("release metadata version is invalid")
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
sha256sum "$output/$archive" >"$output/$archive.sha256"

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
