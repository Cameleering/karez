# Integration test database

A throwaway PostgreSQL 16 for `crates/karez-pg/tests/pg_integration.rs`. Everything here is
test-only: the passwords below are public and must never be used anywhere else.

## Run

```sh
docker compose -f tests/docker/compose.yaml up -d --wait     # about 25 s: starts the server and loads the seed
KAREZ_TEST_PG=127.0.0.1:54316 KAREZ_REQUIRE_SERVER=1 cargo test -p karez-pg --test pg_integration
docker compose -f tests/docker/compose.yaml down -v          # removes the container and its volume
```

The host port is `127.0.0.1:54316` (override with `KAREZ_TEST_PG16_PORT`). Profiles `pg13`, `pg14`, `pg15`, `pg17` and `pg18` start the other majors on ports 54313, 54314, 54315, 54317 and 54318
(`docker compose -f tests/docker/compose.yaml --profile pg13 up -d --wait pg13`); the suite passes on all six. A container made from another copy of the repository can lose its published port
when compose recreates the project network: recreate it with the same command from this folder. Containers and volumes
are named `karez-test-*`.

`KAREZ_TEST_PG` is the server address (`host:port`). If it is unset or the server is not
reachable, each test prints `SKIPPED (not verified against a real server): <reason>` (use
`-- --nocapture` to see it) and returns. With `KAREZ_REQUIRE_SERVER=1` a missing server is a
failure instead, so a verification run cannot pass by doing nothing. A run without a server is not
a verification.

## What is in the database

| thing | detail |
|---|---|
| roles | `karez_scram` (password `karez_test_only_scram`, SCRAM-SHA-256, owns the schema), `karez_clear` (password `karez_test_only_clear`, cleartext `password` method, read-only), `karez_trust` (no password, `trust`, read-only), `postgres` (password `karez_test_only_superuser_pw`) |
| `pg_hba.conf` | one rule per role; everything else rejected |
| schema | customers, addresses, categories, products, inventory, orders, order_items, payments, refunds, invoices, inventory_movements, audit_log; a view `v_order_summary`; a sequence `order_no_seq`; foreign keys, unique, check and not-null constraints, defaults, indexes |
| size | 400,000 rows in `audit_log` and `order_items`, 300,000 in `inventory_movements`, 150,000 in `orders`, 120,000 in `payments` |
| seed | deterministic: every value is a function of the row number (no `random()`) |
| `*_zoo` tables | awkward values: integer limits, NaN/infinity/negative zero/subnormal floats, empty, long, multi-byte, emoji and quoted text, dates from 4713 BC to year 294000, `bytea` from 0 bytes to 5 MiB, uuids, booleans, all-NULL and never-NULL columns |
| `unsupported_zoo` | `numeric`, `jsonb`, `int4[]`, `interval`, an enum and `inet`, for the unsupported-type tests |

The seed finishes by creating a `seed_done` table; the compose health check waits for it.

## Primary and standby

`standby.yaml` starts a primary (port 54361) and a streaming standby (54362) of PostgreSQL 16, built with `pg_basebackup -R`, for `crates/karez-pg/tests/standby.rs`:

```sh
docker compose -f tests/docker/standby.yaml up -d --wait
KAREZ_TEST_PRIMARY=127.0.0.1:54361 KAREZ_TEST_STANDBY=127.0.0.1:54362 KAREZ_REQUIRE_STANDBY=1 cargo test -p karez-pg --test standby
docker compose -f tests/docker/standby.yaml down -v
```

The failover test (`crates/karez-pg/tests/failover.rs`) **destroys this fixture** (it kills the primary and promotes the standby), so it runs only with `KAREZ_TEST_FAILOVER=1`, needs the `docker` command, and the fixture must be recreated afterwards (`down -v`, then `up -d --wait`):

```sh
KAREZ_TEST_PRIMARY=127.0.0.1:54361 KAREZ_TEST_STANDBY=127.0.0.1:54362 KAREZ_TEST_FAILOVER=1 KAREZ_REQUIRE_FAILOVER=1 cargo test -p karez-pg --test failover
```

## SASLprep

`crates/karez-pg/tests/saslprep.rs` creates a role `karez_prep` as the superuser (the `pg_hba.conf` here has a rule for it) with a password the server normalises (`U+2168`, which is `IX` after SASLprep). Run it without and with the feature: `cargo test -p karez-pg [--features saslprep] --test saslprep`.

## TLS

Every server has TLS on with a throwaway CA generated at start and written to `tests/docker/certs/ca-<port>.crt`
(git-ignored). The file is written in the folder the container was *created from*: a container made from another copy of the repository (or another worktree) leaves a stale file here and
the TLS tests fail with a certificate error. Recreate it with `docker compose -f tests/docker/compose.yaml up -d --wait pg16` from this folder. The TLS tests run with the `tls` feature:

```sh
KAREZ_TEST_PG=127.0.0.1:54316 KAREZ_REQUIRE_SERVER=1 \
  cargo test -p karez-pg -p karez-rt --features karez-pg/tls,karez-rt/tls
```

## OAuth and Kerberos servers

Two more fixtures are **built**, not pulled, and need downloads from Debian's mirrors while building (the compiler and PostgreSQL 18's headers for the first, the Kerberos packages for the second):

```sh
docker compose -f tests/docker/compose.yaml --profile oauth up -d --build --wait oauth     # PostgreSQL 18 with a test OAuth validator, port 54319
KAREZ_TEST_OAUTH=127.0.0.1:54319 KAREZ_REQUIRE_OAUTH=1 cargo test -p karez-pg --test auth oauth_server

docker compose -f tests/docker/compose.yaml --profile gss up -d --build --wait kdc pggss   # a KDC (54388) and PostgreSQL 16 with Kerberos (54320)
KAREZ_TEST_GSS=127.0.0.1:54320 KAREZ_REQUIRE_GSS=1 KRB5_CONFIG=$PWD/tests/docker/gss/krb5-client.conf \
  cargo test -p karez-pg --features gss --test auth gss_server                              # Unix only
```

```sh
docker compose -f tests/docker/compose.yaml --profile cert up -d --wait pgcert   # PostgreSQL 17 with a client-certificate rule, port 54321
KAREZ_TEST_CERT=127.0.0.1:54321 KAREZ_REQUIRE_CERT=1 cargo test -p karez-pg --features tls --test auth client_certificate
```

The `cert` role is `karez_cert`; `pg_hba.conf` accepts it only over TLS with a certificate signed by the fixture CA (`hostssl all karez_cert all cert clientcert=verify-full`). The client certificate and key (SEC1 and PKCS#8) are written next to the CA in `tests/docker/certs` (git-ignored). The same service also serves `sslnegotiation=direct`.

The OAuth validator accepts `karez-token-for-<role>` and nothing else; the Kerberos user is `karez_gss@KAREZ.TEST` with the public password `karez_test_only_gss`. The KDC writes the service keytab to `tests/docker/gss/keytabs` (git-ignored). Everything here is test-only.
The PostgreSQL 18 TLS fixture signs its certificate with SHA-384, so SCRAM channel binding there uses SHA-384 (the others use SHA-256).

## Windows

Docker runs in WSL here, and a Windows process reaches `127.0.0.1:54316` through the WSL localhost relay. **The relay stalls when a test sends and receives
several hundred KiB at the same time.** Two tests do that on purpose and fail on Windows through the relay: `execute_many_does_not_deadlock_when_requests_and_results_are_both_large`
and `a_failing_statement_in_a_large_batch_leaves_the_connection_usable` (they hang for about a minute, then the connection is reset). The code is not at fault: a plain Python echo server
stalls the same way through the relay at about 190 KB of two-way traffic and passes 20 MB inside WSL; and both tests pass in 0.25 s on Windows when the traffic goes to the WSL VM's own address
(checked 2026-11-18, PostgreSQL 16 container, Windows 11, WSL2). On Windows run the suite with the script, which starts `forward.py` in WSL and points `KAREZ_TEST_PG` at it:

```powershell
powershell -File tests/docker/windows-test.ps1                                   # cargo test --workspace
powershell -File tests/docker/windows-test.ps1 test -p karez-pg --test pg_integration
```

The TLS tests check the server name `localhost`, so run those through the relay address as shown above; their traffic is small. Or run the whole suite inside WSL, where none of this applies.
