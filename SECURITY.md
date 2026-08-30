# Security model

This is privileged offline tooling. Run a released binary as the service's data
owner while the corresponding service is stopped. Do not run it against paths
that an untrusted user can replace.

The tool treats symlinks, hard-link aliases, special files, unknown manifest
fields, unknown versions, path traversal, checksum mismatches, and incomplete
backup directories as fatal errors. Output and restore destinations are never
silently overwritten.

Report vulnerabilities privately to `isarmg@163.com`. Do not attach production
databases, credentials, recordings, or backup manifests containing private path
information to a public report.
