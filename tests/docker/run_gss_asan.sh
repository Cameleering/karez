#!/usr/bin/env bash
# Run the Kerberos (GSSAPI) tests under AddressSanitizer (which includes LeakSanitizer), against the
# KDC and PostgreSQL 16 fixture. Linux or WSL, nightly Rust: `bash tests/docker/run_gss_asan.sh`.
# It checks the `unsafe` blocks of `karez-gss` that the written argument in docs/design/gss-safety.md
# covers: use after free, double free, out-of-bounds access and leaks on the Rust side of the FFI.
# The system GSS library itself is not instrumented, only its allocations are intercepted.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
cd "$root"
docker compose -f tests/docker/compose.yaml --profile gss up -d --build --wait kdc pggss
export KAREZ_TEST_GSS=127.0.0.1:54320
export KAREZ_REQUIRE_GSS=1
export KRB5_CONFIG="$root/tests/docker/gss/krb5-client.conf"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/karez-asan-target}"
export RUSTFLAGS="-Zsanitizer=address"
export RUSTDOCFLAGS="-Zsanitizer=address"
# leaks are errors; the test harness keeps a few threads alive at exit, which is not a leak of ours
export ASAN_OPTIONS="detect_leaks=1:abort_on_error=0:detect_stack_use_after_return=1"
target="$(rustc +nightly -vV | sed -n 's/^host: //p')"
cargo +nightly test --target "$target" -p karez-gss 2>&1 | tail -15
cargo +nightly test --target "$target" -p karez-pg --features gss --test auth gss 2>&1 | tail -25
