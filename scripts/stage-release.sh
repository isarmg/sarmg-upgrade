#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: stage-release.sh NEW_OUTPUT_DIRECTORY" >&2
  exit 64
fi

output=$1
version=$(python3 - <<'PY'
import pathlib, tomllib

package = tomllib.loads(pathlib.Path("Cargo.toml").read_text(encoding="utf-8"))["package"]
if package["name"] != "sarmg-upgrade":
    raise SystemExit("Cargo.toml does not describe sarmg-upgrade")
print(package["version"])
PY
)
target=x86_64-unknown-linux-gnu
revision=$(git rev-parse HEAD)
if [[ -n $(git status --porcelain --untracked-files=normal) ]]; then
  echo "formal release staging requires a clean source tree" >&2
  exit 1
fi
if [[ $(git describe --exact-match --match "v${version}" --tags HEAD 2>/dev/null || true) != "v${version}" ]]; then
  echo "formal release staging requires immutable tag v${version}" >&2
  exit 1
fi
if [[ $(git cat-file -t "v${version}") != tag ]]; then
  echo "formal release tag must be annotated" >&2
  exit 1
fi
if [[ -e $output ]]; then
  echo "release staging output already exists" >&2
  exit 1
fi

umask 077
mkdir -p "$output/package/bin" "$output/package/LICENSES" "$output/release-tools"
cargo build --locked --release --target "$target"
install -m 0755 "target/${target}/release/sarmg-upgrade" "$output/package/bin/sarmg-upgrade"
install -m 0644 docs/operations.md "$output/package/README.md"
install -m 0644 LICENSE-APACHE "$output/package/LICENSES/Apache-2.0.txt"
install -m 0644 release/sarmg-upgrade-release-signing-public.pem \
  "$output/package/RELEASE-SIGNING-PUBLIC.pem"
install -m 0644 scripts/finalize-release.sh "$output/release-tools/"
"$output/package/bin/sarmg-upgrade" support --json >"$output/package/adapter-catalog.json"
python3 scripts/write-sbom.py "$output/package/SBOM.cdx.json"
rustc --version --verbose >"$output/package/BUILD-ENVIRONMENT.txt"
binary_sha=$(sha256sum "$output/package/bin/sarmg-upgrade" | awk '{print $1}')
catalog_sha=$(sha256sum "$output/package/adapter-catalog.json" | awk '{print $1}')
release_signing_public_key_sha=$(
  openssl pkey -pubin \
    -in "$output/package/RELEASE-SIGNING-PUBLIC.pem" \
    -outform DER \
    | sha256sum \
    | awk '{print $1}'
)
capabilities=$(python3 - "$output/package/adapter-catalog.json" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as source:
    catalog = json.load(source)
if catalog.get("formal_release_target") != "x86_64-unknown-linux-gnu":
    raise SystemExit("binary support catalog does not name the sole formal release target")
print(json.dumps(catalog["supported_capabilities"], separators=(",", ":")))
PY
)
cat >"$output/package/release.json" <<EOF
{
  "product": "sarmg-upgrade",
  "version": "${version}",
  "source_revision": "${revision}",
  "target": "${target}",
  "rust_version": "1.98.0",
  "binary_sha256": "${binary_sha}",
  "catalog_sha256": "${catalog_sha}",
  "release_signing_public_key_sha256": "${release_signing_public_key_sha}",
  "manifest_versions": [2],
  "supported_capabilities": ${capabilities}
}
EOF
cat >"$output/package/provenance.json" <<EOF
{
  "builder": "github-actions/ubuntu-24.04",
  "source_revision": "${revision}",
  "subject_sha256": "${binary_sha}",
  "build_type": "sarmg-upgrade/formal-release-v1"
}
EOF
find "$output/package" -type d -exec chmod 0755 {} +
find "$output/package" -type f -exec chmod 0644 {} +
chmod 0755 "$output/package/bin/sarmg-upgrade" "$output/release-tools/finalize-release.sh"
