#!/usr/bin/env bash
# The exit measurements of M1 step 9 (docs/design/m1-plan.md), one lane, on the client droplet:
#   lane-exit.sh LANE ROUND DB          (called by lane-run.sh when PROFILE=exit)
#
#   1. verify: every open-loop client folds the same checksum on the same request sequence
#   2. S6, COPY of 10,000,000 rows, plain, karez against the other clients and PgBulkInsert (the plan's gate: at least 1.5x PgBulkInsert)
#   3. bulk over TLS (sslmode=verify-full): S4, S5 and S6, with the plain S4 and S5 of the same clients next to them on the same droplets
#   4. open loop S1, S2, S3 and S8, each plain and over TLS (profile `exit` in run.py)
# Cells are tagged round "x1" so nothing here is ranked against cells from other droplets. The order puts the gate first.
set -uo pipefail
LANE="$1"; ROUND="$2"; DB="$3"
export PATH=/root/.cargo/bin:/usr/local/go/bin:$PATH
export BENCH_VENV=/root/venv
cd /root/karez/bench
PW="$(cat /root/bench-pw)"
export BENCH_URL="postgresql://bench:$PW@$DB:5432/bench?sslmode=disable"
export BENCH_URL_TLS="postgresql://bench:$PW@$DB:5432/bench?sslmode=verify-full&sslrootcert=/root/ca.crt"
OUT="/root/out/lane$LANE-round$ROUND-exit"
mkdir -p "$OUT"
cp /root/machine-cl.txt "$OUT/machine-cl.txt" 2>/dev/null
ssh -o BatchMode=yes "root@$DB" cat /root/machine-db.txt > "$OUT/machine-db.txt" 2>/dev/null
R=x1
COMMON=(--out "$OUT" --lane "lane$LANE" --round "$R" --db-host "root@$DB" --root-cert /root/ca.crt)
S6C="karez-blocking,tokio-postgres,sqlx,asyncpg,psycopg3,pgx,libpq,pgjdbc,pgjdbc-tuned,pgbulkinsert"
BULKC="karez-blocking,karez-blocking-chunk,tokio-postgres,sqlx,asyncpg,psycopg3,pgx,libpq,pgjdbc-tuned"
OPENC="karez-blocking,tokio-postgres,sqlx,pgx,libpq,asyncpg,pgjdbc-tuned"
echo "$(date -u +%FT%TZ) lane $LANE exit measurements (round label $R)"
python3 run.py verify --clients "karez-blocking,karez-shared,tokio-postgres,sqlx,pgx,libpq,asyncpg,pgjdbc-tuned" --root-cert /root/ca.crt > "$OUT/verify.txt" 2>&1
echo "verify: rc=$?"
# 2. S6 plain, 10M rows
python3 run.py bulk --clients "$S6C" --only bulk-s6 --repeats 3 --s5-rows 100000 --s6-rows 10000000 "${COMMON[@]}"
echo "$(date -u +%FT%TZ) S6 plain done"
python3 run.py report "$OUT" > /dev/null 2>&1
# 3. bulk plain and over TLS
for sc in bulk-s4 bulk-s5; do
  python3 run.py bulk --clients "$BULKC" --only "$sc" --repeats 3 --s5-rows 100000 --s6-rows 10000000 "${COMMON[@]}"
  python3 run.py bulk --bulk-tls --clients "$BULKC" --only "$sc" --repeats 3 --s5-rows 100000 --s6-rows 10000000 "${COMMON[@]}"
done
python3 run.py bulk --bulk-tls --clients "$S6C" --only bulk-s6 --repeats 3 --s5-rows 100000 --s6-rows 10000000 "${COMMON[@]}"
echo "$(date -u +%FT%TZ) bulk done"
python3 run.py report "$OUT" > /dev/null 2>&1
# 4. open loop
python3 run.py campaign --profile exit --clients "$OPENC" "${COMMON[@]}"
python3 run.py report "$OUT" > /dev/null 2>&1
echo "$(date -u +%FT%TZ) lane $LANE exit measurements finished"
