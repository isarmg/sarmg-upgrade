#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: finalize-release.sh PACKAGE_DIRECTORY ED25519_PRIVATE_KEY OUTPUT_DIRECTORY" >&2
  exit 64
fi
package=$1
private_key=$2
output=$3
archive=isarmg-upgrade-0.2.0-linux-x86_64.tar.zst

if [[ -e $output ]]; then
  echo "final release output already exists" >&2
  exit 1
fi
if [[ ! -f $private_key ]]; then
  echo "release signing key is unavailable" >&2
  exit 1
fi
umask 077
mkdir "$output"
openssl pkey -in "$private_key" -pubout -out "$package/RELEASE-SIGNING-PUBLIC.pem"
chmod 0644 "$package/RELEASE-SIGNING-PUBLIC.pem"
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

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
zstd -dc "$output/$archive" | tar -xf - -C "$temporary"
(cd "$temporary" && sha256sum --check SHA256SUMS)
openssl pkeyutl -verify -rawin -pubin -inkey "$temporary/RELEASE-SIGNING-PUBLIC.pem" \
  -in "$temporary/SHA256SUMS" -sigfile "$temporary/SHA256SUMS.sig"
cmp "$package/adapter-catalog.json" "$temporary/adapter-catalog.json"
"$temporary/bin/isarmg-upgrade" support --json >"$temporary/actual-support.json"
cmp "$temporary/adapter-catalog.json" "$temporary/actual-support.json"
