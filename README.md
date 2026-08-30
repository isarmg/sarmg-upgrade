# iSarmg Upgrade

`isarmg-upgrade` is the only repository that owns upgrades, consistent backups,
backup verification, and restores for the iSarmg products.

Product repositories deliberately contain no legacy-schema readers, automatic
migrations, compatibility aliases, backup writers, or restore code. A product
binary may create only its exact current schema and must refuse every other
application version or schema identity without modifying it.

## Boundary

- This tool runs offline and is never a runtime dependency of a product.
- Every adapter names one exact source version and one exact target version.
- Missing adapters fail closed; the tool never guesses from a similar schema.
- A backup is immutable, checksummed, non-overwriting, and complete only after
  its manifest has been durably written last.
- Restore targets are staged and verified before an atomic installation. A
  failed install preserves the original generation and recovery evidence.
- Services must be stopped, and the tool must obtain the same maintenance lock
  contract as the corresponding product before changing state.
- Secrets are requirements in a manifest, not contents of a backup, unless a
  future adapter explicitly defines a separately encrypted secret resource.

The catalog includes all six repositories. `isarmg-foundation` is a library and
therefore has source/API upgrade adapters but no runtime backup resources.

## Initial commands

```console
isarmg-upgrade catalog --json
isarmg-upgrade inspect-manifest /path/to/backup/manifest.json
```

Backup, restore, and version-to-version commands are added only together with a
tested product adapter. This avoids advertising an unsafe generic migration.

## Development

```console
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```
