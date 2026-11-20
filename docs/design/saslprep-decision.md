# SASLprep for SCRAM passwords: an optional dependency (decision note, 2026-11-20)

Stands in for the issue that AGENTS.md asks for before an optional dependency is added (as [tls-decision.md](tls-decision.md) does for TLS). No issue was opened.

## The problem

PostgreSQL stores a SCRAM password as the verifier of `SASLprep(password)` (RFC 4013, a profile of RFC 3454 stringprep): non-ASCII spaces become a space, "mapped to nothing" characters (the soft hyphen, zero-width joiners) are dropped, the text is normalised to Unicode NFKC, and prohibited characters are refused. libpq applies the same before it computes the client proof. If the preparation fails (a prohibited character), both sides use the password as typed.

For an ASCII password the preparation is the identity. For a non-ASCII password it changes the bytes whenever the text is not already in that form: a Roman numeral `U+2168` is `IX`, a fullwidth letter is its ASCII letter, a letter plus a combining accent (typed on some systems) is the precomposed letter. A client that sends the raw bytes fails the login with `28P01`, which looks like a wrong password.

## Why not std only

Step 1 (the mapping) and step 3 (the prohibited ranges) need tables of a few hundred ranges. Step 2 (NFKC) needs the Unicode decomposition tables, thousands of entries, which std does not expose. Writing them here would copy a Unicode data set into the repository, and a mistake in it would be a login failure that is hard to see.

## Decision

- An optional feature `saslprep` on `karez-pg` (and `karez`, which passes it through) pulls in the `stringprep` crate (0.1.5; it depends on `unicode-bidi`, `unicode-normalization`, `unicode-properties` and `tinyvec`). It is **off by default**; the core and the protocol crates stay dependency-free, and `karez-proto-pg`'s SCRAM client still takes the password as given.
- `karez-pg` prepares the password (feature on): an ASCII password is untouched; a non-ASCII one goes through `stringprep::saslprep`, and one that cannot be prepared is used as typed, as PostgreSQL and libpq do.
- Feature off: a non-ASCII password is used as typed. That is correct whenever it is already in prepared form, which is usual for text typed on a keyboard. If the login then fails with `28P01`, the error's hint says that PostgreSQL normalises passwords and names the feature, so the failure does not read as a wrong password.

## Test

`crates/karez-pg/tests/saslprep.rs` makes a role whose stored password is `U+2168` (so the verifier is of `IX`) and logs in with `IX`, `U+2168` and `I` + soft hyphen + `X`. Without the feature only `IX` logs in and the others fail with `28P01` and the hint; with it all three log in; wrong passwords fail either way. Run on PostgreSQL 16 on 2026-11-20 (`KAREZ_TEST_PG=127.0.0.1:54316 cargo test -p karez-pg [--features saslprep] --test saslprep`).

## Not covered

The hint is only for SCRAM and only when the server's answer is `28P01`. MD5 and cleartext logins send the password as typed, as PostgreSQL itself does for them. A password that SASLprep refuses (for example one with a control character) is used as typed on both sides; that path is not tested against a server.
