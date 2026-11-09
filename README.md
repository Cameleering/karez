# karez

A fast, small, standards-shaped database connectivity library for Rust: one API and native
drivers for PostgreSQL, MySQL and MariaDB, SQL Server and SQLite, with parity targets for JDBC,
ODBC and ADBC. Results are decoded straight from the wire into Arrow-layout columnar chunks, with
a zero-cost row view on top, and the same core is exported as an ADBC driver, an ODBC driver and
a JDBC driver so other languages and tools can use it.

A karez is an underground channel that carries water from a deep aquifer to where it is used, by
gravity alone. karez is part of the [Cameleer](https://github.com/pmsudhi/cameleer) family, next
to simoom, Cameleer's HTTP engine.

Status: design stage; nothing is built yet. See [docs/design/plan.md](docs/design/plan.md) for
the plan and its go/no-go milestone, [docs/design/research.md](docs/design/research.md) for the
evidence, and [docs/research/](docs/research/) for the seven research notes behind it.

Licensed under the Apache License 2.0.
