#!/usr/bin/env bash
# Run every fuzz target for a fixed time each (default 60 s), in WSL or Linux with nightly Rust and
# cargo-fuzz installed: `bash fuzz/run.sh [seconds]`.
#
# The seed corpus in `fuzz/corpus/<target>` is read only: new inputs go to a working corpus under
# `$FUZZ_WORK` (default `~/karez-fuzz-work`), so the repository does not grow. A crash is written to
# `fuzz/artifacts/<target>/`. The seeds are made by the ignored test `dump_seed_corpus`
# (see fuzz/README.md).
set -u
secs="${1:-60}"
cd "$(dirname "$0")"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/karez-fuzz-target}"
work="${FUZZ_WORK:-$HOME/karez-fuzz-work}"
status=0
for t in frame message row scram oauth cert config_url; do
    echo "=== $t ($secs s)"
    mkdir -p "$work/$t"
    cargo +nightly fuzz run "$t" "$work/$t" "corpus/$t" -- \
        -max_total_time="$secs" -timeout=10 -max_len=4096 2>&1 \
        | grep -E "^(#[0-9]+\s+(INITED|DONE)|Done |SUMMARY|==ERROR|thread|panicked|Test unit written)" \
        | tail -4
    rc=${PIPESTATUS[0]}
    if [ "$rc" -ne 0 ]; then
        echo "FAILED: $t (exit $rc)"
        status=1
    fi
done
exit $status
