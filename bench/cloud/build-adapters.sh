#!/usr/bin/env bash
# Build every adapter on a client droplet (from /root/karez/bench). Safe to rerun after `ctl.sh ship`.
set -euo pipefail
log() { echo "[build-adapters] $*" >&2; }
export PATH=/root/.cargo/bin:/usr/local/go/bin:$PATH
log "building the adapters"
cd /root/karez/bench
# a shipped tree keeps the files' old modification times, which would make cargo and Maven think nothing changed
find . -type f \( -name '*.rs' -o -name '*.go' -o -name '*.java' -o -name '*.c' -o -name '*.toml' -o -name pom.xml \) -not -path './target/*' -exec touch {} +
cargo build --release -q 2>&1 | tail -5 >&2
(cd adapters/go && go mod download && go build -o karez-bench-go .)
(cd adapters/java && mvn -q -B package -DskipTests 2>&1 | tail -5 >&2)
(cd adapters/c && cc -O2 -Wall -o libpq_adapter libpq_adapter.c -I"$(pg_config --includedir)" -L"$(pg_config --libdir)" -lpq -lpthread -lm)
