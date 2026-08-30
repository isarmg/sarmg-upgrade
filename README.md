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

## Commands

```console
isarmg-upgrade catalog --json
isarmg-upgrade inspect-manifest /path/to/backup/manifest.json
isarmg-upgrade backup-sqlite --product host-monitoring \\
  --database /var/lib/isarmg/host-monitoring/app.sqlite3 \\
  --output /srv/backup/host-monitoring-0.7.0
isarmg-upgrade verify-sqlite /srv/backup/host-monitoring-0.7.0
isarmg-upgrade restore-sqlite --product host-monitoring \\
  --expect-version 0.7.0 \\
  --input /srv/backup/host-monitoring-0.7.0 \\
  --database /var/lib/isarmg/host-monitoring/app.sqlite3 \\
  --replace-existing
isarmg-upgrade upgrade-sqlite --product host-monitoring \\
  --from-version 0.6.0 --to-version 0.7.0 \\
  --database /var/lib/isarmg/host-monitoring/app.sqlite3 \\
  --backup-output /srv/backup/host-monitoring-0.6.0-before-upgrade
isarmg-upgrade verify-source-backup --product host-monitoring \\
  --from-version 0.6.0 --to-version 0.7.0 \\
  --input /srv/backup/host-monitoring-0.6.0-before-upgrade
isarmg-upgrade upgrade-sqlite --product sunshine-manager \\
  --from-version 0.6.0 --to-version 0.7.0 \\
  --database /var/lib/isarmg/sunshine-manager/app.sqlite3 \\
  --backup-output /srv/backup/sunshine-manager-0.6.0-before-upgrade
```

Replacing an existing database is never implicit. Restore stages and verifies
the incoming database, preserves the old database plus SQLite sidecars in a
durable adjacent journal, and only then installs it. If a process is interrupted
after mutation, the error prints the recovery directory; resolve it explicitly:

```console
isarmg-upgrade recover-sqlite --recovery /path/from/error --action commit
isarmg-upgrade recover-sqlite --recovery /path/from/error --action rollback
```

The registered adapters support exactly Host Monitoring `0.6.0 -> 0.7.0` and
Sunshine Manager `0.6.0 -> 0.7.0`. They validate their respective five-row and
four-row SQLx ledgers and SHA-384 checksums, clone the source generation without
opening it, publish a verified old-generation backup, create the consolidated
`0.7.0` schema in a same-filesystem staging directory, and copy every table
through explicit column lists. They do not run `ALTER TABLE` against the source.

The generic SQLite-only backup command supports Host Monitoring and Sunshine
Manager current databases. Composite Photo, Sentinel, and Dufs commands are
added only together with their tested product adapters. This avoids advertising
an unsafe generic migration.

## Development

```console
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```
