# Security policy

karez will parse untrusted network protocols, handle credentials and implement authentication,
so security reports and design reviews are especially welcome.

## Reporting a vulnerability

Please **don't open a public issue**. Report it privately through GitHub:
**Security → Report a vulnerability** on
[github.com/cameleering/karez](https://github.com/cameleering/karez/security/advisories/new).

Include the affected file or crate and version (or commit), steps to reproduce, and the impact
you expect. We aim to acknowledge reports within a few days, agree on a fix and a disclosure
date with you, and credit you in the advisory unless you prefer otherwise.

## Supported versions

karez is in the design stage and has no released versions. Until the first release, a report
about the plan or the research (for example a design rule that would be unsafe) is as welcome as
a report about code.

## Scope

In scope, once the code exists: every crate in this repository, in particular the protocol
decoders, the authentication and TLS paths, the connection-string and URL parser, the C ABI and
the ODBC, ADBC and JDBC exports.

Out of scope: vulnerabilities in the databases themselves, in the drivers of other projects, and
in third-party libraries; report those to their owners.

## Security rules karez is designed around

The plan ([docs/design/plan.md](docs/design/plan.md), section 7) derives seven rules from the
CVE history of existing drivers: no client-side inlining of literals, encoding-aware and
fail-closed escaping, checked arithmetic on every length, no code loading from connection
strings, server-initiated file actions off by default, quoted identifiers in driver-built SQL, and
careful credential handling. An independent audit and fuzzing of the decoders and the C ABI are
planned before 1.0.
