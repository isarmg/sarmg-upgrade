# Security model

This is privileged offline tooling. Run a released binary with only the
privileges required to read and preserve the selected product resources while
the corresponding service is stopped. Pass explicit service identities where
an adapter requires them. Do not run it against paths that an untrusted user
can replace.

The tool treats symlinks, hard-link aliases, special files, unknown manifest
fields, unknown versions, path traversal, checksum mismatches, and incomplete
backup directories as fatal errors. Output and restore destinations are never
silently overwritten.

An interrupted restore is not guessed forward or backward. Preserve its
adjacent recovery directory and run `recover-sqlite` with an explicit product,
expected version, and `commit` or `rollback` decision while the service remains
stopped.

Generic SQLite backup, verification, restore, and recovery accept only the
code-owned official current identity for the explicitly selected Host Monitoring
or Sunshine Manager product. A database or journal is rejected even when its
self-reported version, revision, schema hash, manifest, and actual schema are
mutually consistent but absent from that allowlist. Composite products cannot
use the generic recovery entry point.

Version upgrades require exact `--product`, `--from-version`, and `--to-version`
arguments. Before SQLite parses any old state, the tool byte-clones the main
database and all present SQLite sidecars under the exclusive product maintenance
lock. Validation, backup creation, and target construction operate on that
clone; the original generation is touched only by the final durable recovery
journal switch.

Sentinel upgrades additionally acquire the runtime and MediaMTX locks after the
database maintenance lock. They refuse a mismatched config, companion contract,
recording root, orphan recording path, or undecryptable camera credential before
publishing a backup or changing the database. The private base64 credentials-key
file must decode to exactly 32 bytes; its path and contents are not written to the
backup or recovery journal and are never included in JSON output. The adapter
uses that external key to decrypt the 0.1 raw AES-GCM credentials and emits only
the pinned 0.2 HKDF-SHA256-derived canonical JSON envelopes. Each envelope is
authenticated against its camera UUID and exact database field, so a value
cannot be moved between cameras or between URL, username, and password fields.
Recovery commit revalidates every envelope with the explicitly supplied key;
rollback never needs to reinterpret old credential bytes. Only the key's
non-secret SHA-256 identifier and the non-secret envelope contract appear in the
manifest and JSON output. The composite backup does contain the MediaMTX config
and recordings, which may themselves be sensitive and must be protected
accordingly.

Dufs upgrades require an exact protected YAML config, numeric service uid/gid,
state directory, and shared root. The config is opened without following links,
must have the exact supported owner/mode and no extended POSIX ACL, and is
hashed and identity-checked before and after use. Account names, PHC strings,
and owner digests are never written to the manifest, recovery journal, or JSON
output. The backup does include the protected config and the complete shared
tree, so the entire backup must be treated as sensitive.

The Dufs adapter holds the database maintenance lock and then a nonblocking
exclusive lock on the anchored shared-root descriptor. Because the Dufs runtime
does not honor the database lock, the adapter atomically installs a fixed
non-SQLite blocker at `state.sqlite3` before changing any private stage
directory. The original database generation and sidecars remain in the durable
adjacent recovery directory until a verified commit. Explicit commit or
rollback recovery re-acquires the same locks; an unprovable or mixed generation
keeps the blocker in place instead of permitting either Dufs version to create
a replacement database.

Report vulnerabilities privately to `isarmg@163.com`. Do not attach production
databases, credentials, recordings, or backup manifests containing private path
information to a public report.
