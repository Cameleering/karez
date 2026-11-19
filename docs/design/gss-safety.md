# `karez-gss`: the unsafe code and why it is sound

_Written 2026-11-19 with M1 step 7. Plan section 7.2 allows `unsafe` in named crates, each block with a `SAFETY:` comment, counted. This note is the argument behind those comments. There is no paid audit (STATUS.md); this
text, the tests and public review are the assurance._

## What the crate is

`crates/karez-gss` has no dependencies and 12 `unsafe` blocks, all in `src/lib.rs`, all calls into one C library: the system GSS-API (MIT Kerberos' `libgssapi_krb5.so.2` on Linux, the `GSS` framework on macOS).
It exposes a small safe API: `Context::new(target, Credentials)`, `Context::step(input) -> output token`, `Context::is_complete()`. On Windows it compiles to a stub that returns an error (SSPI is not implemented).
Nothing else in karez uses it unless `karez-pg`'s `gss` feature is on; the core, the protocol crates and `karez-pg` without the feature keep `#![forbid(unsafe_code)]` (the workspace lint; `karez-gss` sets its own lints).

## The rules every block relies on

1. **Handles are owned, created once, released once.** The three handle types (`gss_name_t`, `gss_ctx_id_t`, `gss_cred_id_t`) are raw pointers stored in fields of `Context`. Each is produced by exactly one call
   (`gss_import_name`, `gss_init_sec_context`, `gss_acquire_cred_with_password`) and released in `Drop` (the user-name handle used to acquire credentials is released right after, once). `Context` is neither `Clone` nor `Send`
   (it holds raw pointers), so no handle is used from two places or from another thread.
2. **A failed constructor still releases what it built.** `Context::new` builds the struct first and then acquires credentials, so an error return runs `Drop`.
3. **Buffers lent to C outlive the call and are only read.** An input buffer is a descriptor pointing into a Rust slice that is alive for the whole call (the function takes the slice by reference); the C signatures treat it as `const`.
4. **Buffers filled by C are copied, then released.** An output descriptor starts as zero (null pointer, length 0). After the call the bytes are copied into a `Vec` while the descriptor's memory is valid, and `gss_release_buffer` is called exactly
   once, on success and on error. Nothing keeps a pointer into the library's memory.
5. **Statuses are checked after every call**, using the standard error mask (`GSS_ERROR`), and the minor status is a local `u32`. A returned status is never used as a pointer or an index.
6. **No callbacks, no global state, no threads, no `static mut`.** The two name-type OIDs are `'static` byte arrays defined in the crate, not read from the library's exported variables.
7. **Bounded loops.** The error-text loop (`gss_display_status` can return several messages) stops after 16 rounds, whatever the library says.

## The blocks

| # | where | call | what makes it sound |
|---|---|---|---|
| 1 | `error` | `gss_display_status` | rule 5, 6: writes only the local `minor`, `ctx` and a zero descriptor; null mech means the default |
| 2 | `error` | `slice::from_raw_parts` over the filled buffer | rule 4: the library allocated `length` bytes at `value` (checked non-null); the copy happens before the release |
| 3 | `error` | `gss_release_buffer` | rule 4: the descriptor was filled by block 1 and is released once |
| 4 | `import` | `gss_import_name` | rule 3: the input points into the caller's slice; the OID is `'static`; the returned name is owned by the caller (rule 1) |
| 5 | `Context::new` | `gss_acquire_cred_with_password` | rule 3: the password buffer points into a `&str`; the user-name handle is from block 4; the credential is stored in the struct (rule 1) |
| 6 | `Context::new` | `gss_release_name` (user name) | rule 1: released once; the credential keeps its own copy of the principal |
| 7 | `step` | `gss_init_sec_context` | rule 1, 3, 4: handles are the struct's own (the context handle is null on the first call and the library's afterwards); the input token points into the caller's slice; optional outputs we do not want are null |
| 8 | `step` | `slice::from_raw_parts` over the output token | rule 4 |
| 9 | `step` | `gss_release_buffer` | rule 4: released once, also on the error path |
| 10 | `Drop` | `gss_delete_sec_context` | rule 1: only if non-null; a null output buffer means no token is wanted |
| 11 | `Drop` | `gss_release_name` (target) | rule 1 |
| 12 | `Drop` | `gss_release_cred` | rule 1 |

## What the argument does not cover

- **The library itself.** A bug in `libgssapi_krb5` is outside this crate. The crate passes valid handles and buffers and nothing else.
- **ABI assumptions.** `OM_uint32` is `u32`, the buffer descriptor is `{size_t, void*}` and the OID descriptor `{OM_uint32, void*}`, which is the layout of the GSS-API C binding (RFC 2744) as MIT Kerberos declares it. The tests link and run on
  x86-64 Linux against MIT Kerberos 1.20 only. Heimdal, macOS's `GSS` framework and 32-bit targets are untested.
- **`gss_acquire_cred_with_password`** exists in MIT Kerberos 1.11 and later and in Heimdal; a library without it fails at link time, not at run time.

## How it is tested

- Unit tests (`cargo test -p karez-gss`, Linux): names are imported without a network; a missing ticket is a readable error, not a crash; 50 failed acquisitions in a row leak nothing that the process can see (a crude check; the leak-detector run is below).
- Integration tests (`crates/karez-pg/tests/auth.rs`, module `gss_server`): a real KDC and a real PostgreSQL (`tests/docker/gss`): a login with a password succeeds with mutual authentication; a wrong password, an unknown service name and a host
  name that is not the service principal fail with the library's own message; the secret is not in the error.
- **AddressSanitizer with LeakSanitizer [measured here], 2026-11-19:** `bash tests/docker/run_gss_asan.sh` (WSL2, nightly rustc 1.102.0 with `-Zsanitizer=address`, MIT Kerberos 1.20 in the system, the KDC and PostgreSQL 16 fixture). Both test sets above pass with the sanitizer on: the 3 unit tests of `karez-gss` and the 5 `gss_server` tests, with
  no report of a use after free, double free, out-of-bounds access or leak. The test binary was checked for the instrumentation (`__asan_*` and `__lsan_*` symbols), and a control program that forgets a `Box` was reported as a leak under the same options, so a clean result means something. Limits: the standard library and the system GSS library are not instrumented
  (ASan intercepts their allocations, so a leak or double free through `karez-gss` is seen, but a bad access inside the C library is not); it covers the paths the tests reach (a login, a wrong password, an unknown service, a bad host, a failed import), not every `gss_*` error return; only MIT Kerberos was run.
- **Not done:** Miri cannot run FFI, Valgrind was not run, and ASan was run once, not in CI. Malformed server tokens are not tested: the system library parses them, and no test sends one.
