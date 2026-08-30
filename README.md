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
- Raw external keys are requirements in a manifest, not contents of a backup.
  An adapter may define a protected configuration as a sensitive backup
  resource; its manifest still contains only hashes and aggregate metadata.

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
isarmg-upgrade upgrade-dufs --product dufs-ram \\
  --from-version 0.49.7 --to-version 0.50.0 \\
  --database /var/lib/dufs/state.sqlite3 --state-dir /var/lib/dufs \\
  --shared-root /srv/dufs --config /etc/dufs/dufs.yml \\
  --service-uid 991 --service-gid 991 \\
  --backup-output /srv/backup/dufs-0.49.7-before-upgrade \\
  --max-tree-entries 1000000 --max-tree-logical-bytes 1099511627776 \\
  --max-tree-backup-bytes 1099511627776 --max-entries-per-directory 100000
isarmg-upgrade verify-dufs-source-backup --product dufs-ram \\
  --from-version 0.49.7 --to-version 0.50.0 \\
  --input /srv/backup/dufs-0.49.7-before-upgrade \\
  --config /etc/dufs/dufs.yml --shared-root /srv/dufs \\
  --service-uid 991 --service-gid 991
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

The registered adapters support exactly Host Monitoring `0.6.0 -> 0.7.0`,
Sunshine Manager `0.6.0 -> 0.7.0`, composite Sentinel Monitor
`0.1.0 -> 0.2.0`, and composite Dufs `0.49.7 -> 0.50.0`. The SQLx-based
adapters validate their exact ledgers and SHA-384 checksums. All adapters clone
the source generation without opening it, publish a verified
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

The Dufs adapter accepts only the official v0.49.7 schema-v5 generation:
SQLite `application_id=0x44554653`, `user_version=5`, no
`product_metadata`, and schema SHA-256
`3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423`.
The source tag is pinned to commit
`5b098e2a8f05557b72efdf7929f4ccef3a3af837`; the target contract is pinned to
`2369bd990abf4c1492ca16178f2f66765104be25`. The target is a newly built
v0.50.0 revision-1 database with the same data-schema fingerprint and exact
`dufs-ram` metadata; the old database is never altered in place.

The protected YAML config supplies the only owner-mapping authority. Every old
`SHA256(username)` owner in operations, upload sessions, and purge jobs is
rewritten to `SHA256("dufs-durable-owner-v1\0" || username)`. Unknown owners
and old/new/target-key collisions are rejected before backup publication. The
adapter validates the shared-root device/inode binding, active upload stage
identity, purge resource identity, and reserved namespaces. It atomically
renames each exact v0.49.7 private directory
`.dufs-quarantine-00000000-0000-0000-0000-000000000000.hold` to
`.dufs-upload-stages`; upload `target_revision` bytes are preserved.

The immutable Dufs backup contains the byte-exact raw SQLite generation and
sidecars, a recovered canonical source database, the protected config, and a
metadata-, hard-link-, sparse-file-, symlink-, and xattr-aware shared-tree
copy. Its manifest marks the protected config as sensitive without including
usernames, password hashes, or owner digests. Required tree budgets are
operator-selected and recorded. Lock order is config anchor, exclusive database
maintenance lock, then nonblocking exclusive shared-root lock.

Dufs does not honor the database maintenance lock at runtime. The durable
journal therefore atomically exchanges a fixed non-SQLite blocker into
`state.sqlite3` before any tree rename, and leaves it there until the target is
fully verified. Resolve a crash explicitly; ambiguous generations remain
blocked rather than letting either Dufs version initialize a new database:

```console
isarmg-upgrade recover-dufs-upgrade --product dufs-ram \\
  --from-version 0.49.7 --to-version 0.50.0 \\
  --database /var/lib/dufs/state.sqlite3 \\
  --state-dir /var/lib/dufs --shared-root /srv/dufs \\
  --config /etc/dufs/dufs.yml --service-uid 991 --service-gid 991 \\
  --recovery /var/lib/dufs/.state.sqlite3.dufs-ram.upgrade-recovery \\
  --action rollback
```

The generic SQLite-only commands have a code-owned allowlist containing only
Host Monitoring `0.7.0`, revision `1`, schema SHA-256
`2f63778e94b345d100c10f8b45b98f06e39590547f6b1d65f9b5b0e7f6989328`, and
Sunshine Manager `0.7.0`, revision `1`, schema SHA-256
`1e55653f9b9b4805873164e52b79d399aec4fe327a8648218d4cbcb16b561b98`.
Database metadata, the actual schema, backup manifest, explicit product, and
restore/recovery journal must all agree with that allowlist. A different but
self-consistent identity is rejected. Sentinel and Dufs have only their exact
composite commands above; Photo commands are added only together with a tested
product adapter.

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
