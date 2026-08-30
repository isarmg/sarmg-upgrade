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
isarmg-upgrade verify-sqlite --product host-monitoring \\
  --input /srv/backup/host-monitoring-0.7.0
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
isarmg-upgrade upgrade-sentinel --product sentinel-monitor \\
  --from-version 0.1.0 --to-version 0.2.0 \\
  --database /var/lib/isarmg/sentinel-monitor/app.sqlite3 \\
  --runtime-directory /run/isarmg/sentinel-monitor \\
  --mediamtx-config /etc/isarmg/sentinel-monitor/mediamtx.yml \\
  --mediamtx-contract /var/lib/isarmg/sentinel-monitor/mediamtx.lock \\
  --recordings-directory /var/lib/isarmg/sentinel-monitor/recordings \\
  --credentials-key-file /run/credentials/sentinel.key \\
  --backup-output /srv/backup/sentinel-monitor-0.1.0-before-upgrade
isarmg-upgrade verify-sentinel-source-backup --product sentinel-monitor \\
  --from-version 0.1.0 --to-version 0.2.0 \\
  --input /srv/backup/sentinel-monitor-0.1.0-before-upgrade \\
  --credentials-key-file /run/credentials/sentinel.key
```

Replacing an existing database is never implicit. Restore stages and verifies
the incoming database, preserves the old database plus SQLite sidecars in a
durable adjacent journal, and only then installs it. If a process is interrupted
after mutation, the error prints the recovery directory; resolve it explicitly:

```console
isarmg-upgrade recover-sqlite --product host-monitoring \\
  --expect-version 0.7.0 --recovery /path/from/error --action commit
isarmg-upgrade recover-sqlite --product host-monitoring \\
  --expect-version 0.7.0 --recovery /path/from/error --action rollback
```

The registered adapters support exactly Host Monitoring `0.6.0 -> 0.7.0` and
Sunshine Manager `0.6.0 -> 0.7.0`, plus the composite Sentinel Monitor
`0.1.0 -> 0.2.0` adapter. They validate their exact SQLx ledgers and SHA-384
checksums, clone the source generation without opening it, publish a verified
old-generation backup, create the current schema in a same-filesystem staging
directory, and copy every table through explicit column lists. They do not run
`ALTER TABLE` against the source.

The Sentinel adapter obtains the database maintenance, Sentinel runtime, and
MediaMTX locks in that order. Its immutable backup contains the old SQLite
database, exact MediaMTX config and companion contract, and every recording
file and empty directory with a checksummed inventory. The credentials key is
required only to prove that every encrypted camera value is decryptable; its
file must grant no group or other access, and the key is never copied into the
backup, journal, manifest, or command output. The manifest stores only its
non-secret SHA-256 identifier so verification and future restore can require
the exact external key.
MediaMTX resources do not change between these two versions, so the journaled
switch mutates only the database after confirming that config, contract, and
recordings remain byte-identical. Resolve an interrupted switch while both
services remain stopped:

```console
isarmg-upgrade recover-sentinel-upgrade --product sentinel-monitor \\
  --from-version 0.1.0 --to-version 0.2.0 \\
  --database /var/lib/isarmg/sentinel-monitor/app.sqlite3 \\
  --runtime-directory /run/isarmg/sentinel-monitor \\
  --recovery /path/from/error --action commit
```

The generic SQLite-only commands have a code-owned allowlist containing only
Host Monitoring `0.7.0`, revision `1`, schema SHA-256
`2f63778e94b345d100c10f8b45b98f06e39590547f6b1d65f9b5b0e7f6989328`, and
Sunshine Manager `0.7.0`, revision `1`, schema SHA-256
`1e55653f9b9b4805873164e52b79d399aec4fe327a8648218d4cbcb16b561b98`.
Database metadata, the actual schema, backup manifest, explicit product, and
restore/recovery journal must all agree with that allowlist. A different but
self-consistent identity is rejected. Sentinel has only the exact composite
command above; Photo and Dufs commands are added only together with tested
product adapters.

## Development

```console
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 check --locked --all-targets --all-features
cargo +1.98.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.98.0 test --locked --all-targets --all-features
./scripts/check-workflow-supply-chain.py
```

The workflow policy script scans every Git-tracked workflow and rejects mutable
action references, non-fixed runners, excessive permissions, missing job
timeouts, and checkout credentials that would persist in the worktree. It also
runs negative fixtures for floating actions, floating runners, and write
permissions on every invocation.
