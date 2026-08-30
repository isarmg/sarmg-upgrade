# Security model

This is privileged offline tooling. Run a released binary as the service's data
owner while the corresponding service is stopped. Do not run it against paths
that an untrusted user can replace.

The tool treats symlinks, hard-link aliases, special files, unknown manifest
fields, unknown versions, path traversal, checksum mismatches, and incomplete
backup directories as fatal errors. Output and restore destinations are never
silently overwritten.

An interrupted restore is not guessed forward or backward. Preserve its
adjacent recovery directory and run `recover-sqlite` with an explicit `commit`
or `rollback` decision while the service remains stopped.

Version upgrades require exact `--product`, `--from-version`, and `--to-version`
arguments. Before SQLite parses any old state, the tool byte-clones the main
database and all present SQLite sidecars under the exclusive product maintenance
lock. Validation, backup creation, and target construction operate on that
clone; the original generation is touched only by the final durable recovery
journal switch.

Sentinel upgrades additionally acquire the runtime and MediaMTX locks after the
database maintenance lock. They refuse a mismatched config, companion contract,
recording root, orphan recording path, or undecryptable camera credential before
publishing a backup or changing the database. The private base64 credentials-key file
must decode to exactly 32 bytes; its path and contents are not written to the
backup or recovery journal and are never included in JSON output. Only its
non-secret SHA-256 identifier appears in the manifest and JSON output. The composite
backup does contain the MediaMTX config and recordings, which may themselves be
sensitive and must be protected accordingly.

Report vulnerabilities privately to `isarmg@163.com`. Do not attach production
databases, credentials, recordings, or backup manifests containing private path
information to a public report.
