# Decision record: TLS through an optional rustls feature

_2026-11-13. Decided by the owner (answer to "how should we add TLS?"). This record stands in for the issue
that AGENTS.md rule 5 asks for before an optional dependency is added; no GitHub issue was opened, because
nothing was pushed._

## Decision

TLS is an optional cargo feature `tls` of `karez-pg` (and `karez-rt`, which passes it through). With the
feature off, karez has no dependencies. With it on, karez-pg depends on:

- `rustls` 0.23 with the `ring` crypto provider, `std` and `tls12` (default features off), and
- `webpki-roots` 1.x, the Mozilla root store, used when no root file is given.

## Why

- Writing TLS 1.3, X.509 validation and the ciphers from scratch is weeks of work with a high risk of a
  security bug, and no audit. rustls is widely deployed and reviewed.
- The `ring` provider built on this Windows machine with no extra tools (16 crates, 11 s); `aws-lc-rs`
  needs more (CMake, NASM) and was not tried.
- `cameleer-tls` stays a later, opt-in backend (STATUS "decisions already made").

## Behaviour

- Default `sslmode` is `verify-full` (plan section 1.2). Without the feature, connecting needs an explicit
  `sslmode=disable`, and any other mode fails with a clear error that names the feature.
- `sslmode=disable | require | verify-ca | verify-full`. `require` encrypts but trusts any certificate and
  stops passive listening only. `verify-ca` checks the chain, not the name. `verify-full` checks both.
- `sslrootcert=<PEM file>` or `system` (the built-in roots). The file is read only as a list of CA
  certificates.
- The handshake starts with an SSLRequest and runs before any startup message or password.

## Limits (all stated in the results note)

- Not supported over TLS yet: sharing the connection in `karez-rt` (it needs a second handle on the socket),
  and `execute_many` batches over 64 KiB (a clear error).
- Cancel requests go over a fresh plaintext connection, as PostgreSQL accepts before 17.
- No client certificates, no `SCRAM-SHA-256-PLUS` channel binding, no CRL or OCSP.
- The dependency tree is rustls, rustls-webpki, ring, webpki-roots and their small helpers; it is audited
  by others, not by us, and is outside the pre-1.0 audit scope the owner set (core, protocol crates, ODBC FFI).
