# Integration test database

A throwaway PostgreSQL 16 for `crates/karez-pg/tests/pg_integration.rs`. Everything here is
test-only: the passwords below are public and must never be used anywhere else.

## Run

```sh
docker compose -f tests/docker/compose.yaml up -d --wait     # about 25 s: starts the server and loads the seed
KAREZ_TEST_PG=127.0.0.1:54316 KAREZ_REQUIRE_SERVER=1 cargo test -p karez-pg --test pg_integration
docker compose -f tests/docker/compose.yaml down -v          # removes the container and its volume
```

The host port is `127.0.0.1:54316` (override with `KAREZ_TEST_PG16_PORT`). Containers and volumes
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

## TLS

Every server has TLS on with a throwaway CA generated at start and written to `tests/docker/certs/ca-<port>.crt`
(git-ignored). The TLS tests run with the `tls` feature:

```sh
KAREZ_TEST_PG=127.0.0.1:54316 KAREZ_REQUIRE_SERVER=1 \
  cargo test -p karez-pg -p karez-rt --features karez-pg/tls,karez-rt/tls
```
