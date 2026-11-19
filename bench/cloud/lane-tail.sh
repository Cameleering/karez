#!/usr/bin/env bash
# The tail of round 3 (2026-11-18): what the stop time cut off, on fresh droplets, with karez baselines on the same droplets.
# Called by lane-run.sh when PROFILE=tail:  lane-tail.sh LANE ROUND DB
# Cells are tagged round "3t" so ratios are computed only against karez-blocking run here, never against the old droplets.
#   lane 1: open loop for zero-postgres, r2dbc R2/R4; bulk for the karez variants, diesel, sqlx, pgjdbc-tuned, r2dbc
#   lane 3: open loop for sqlx-tuned, psycopg3 R4;     bulk for the karez variants, diesel, pgx, vertx, psycopg3
set -uo pipefail
LANE="$1"; ROUND="$2"; DB="$3"
export PATH=/root/.cargo/bin:/usr/local/go/bin:$PATH
export BENCH_VENV=/root/venv
cd /root/karez/bench
PW="$(cat /root/bench-pw)"
export BENCH_URL="postgresql://bench:$PW@$DB:5432/bench?sslmode=disable"
export BENCH_URL_TLS="postgresql://bench:$PW@$DB:5432/bench?sslmode=verify-full&sslrootcert=/root/ca.crt"
export BENCH_TAXI_ROWS="$(cat /root/taxi_rows)"
OUT="/root/out/lane$LANE-round$ROUND-tail"
mkdir -p "$OUT"
cp /root/machine-cl.txt "$OUT/machine-cl.txt" 2>/dev/null
ssh -o BatchMode=yes "root@$DB" cat /root/machine-db.txt > "$OUT/machine-db.txt" 2>/dev/null
R=3t
KV="karez-blocking,karez-blocking-chunk,karez-blocking-part,karez-blocking-mrow"
BASE="karez-blocking,karez-shared"
case "$LANE" in
  1) OPEN="zero-postgres"; TAILC="r2dbc-postgresql"; TAILONLY="r2- r4-"; BULK="$KV,diesel,sqlx,pgjdbc-tuned,r2dbc-postgresql"
     VER="karez-blocking,karez-shared,zero-postgres,r2dbc-postgresql,sqlx,pgjdbc-tuned,diesel" ;;
  3) OPEN="sqlx-tuned"; TAILC="psycopg3"; TAILONLY="r4-"; BULK="$KV,diesel,pgx,vertx-pg-client,psycopg3"
     VER="karez-blocking,karez-shared,sqlx-tuned,psycopg3,pgx,vertx-pg-client,diesel" ;;
  *) echo "no tail plan for lane $LANE"; exit 2 ;;
esac
COMMON=(--out "$OUT" --lane "lane$LANE" --round "$R" --db-host "root@$DB" --root-cert /root/ca.crt)
echo "$(date -u +%FT%TZ) lane $LANE tail (round label $R)"
python3 run.py verify --clients "$VER" --root-cert /root/ca.crt > "$OUT/verify.txt" 2>&1
echo "verify: rc=$?"
if [ "${XCHECK:-0}" = 1 ]; then
  for sc in s1 s2; do
    python3 crosscheck.py --url "$BENCH_URL" --pgbench pgbench --rates 1000,2000 --conns 8 --duration 20 --scenario $sc --jobs 4 > "$OUT/crosscheck-$sc.txt" 2>&1
    echo "crosscheck $sc: rc=$?"
  done
fi
# 1. open loop: the karez baselines and the tail client on every configuration (--only "ed-" keeps the bulk part of the profile out)
python3 run.py campaign --profile lean2 --clients "$BASE,$OPEN" --only "ed-" "${COMMON[@]}"
echo "$(date -u +%FT%TZ) open loop done"
# 2. the tail client's missing configurations
for o in $TAILONLY; do
  python3 run.py campaign --profile lean2 --clients "$TAILC" --only "$o" "${COMMON[@]}"
done
echo "$(date -u +%FT%TZ) tail clients done"
# 3. bulk (same sizes as the lean2 profile)
python3 run.py bulk --clients "$BULK" --repeats 3 --s5-rows 100000 --s6-rows 3000000 "${COMMON[@]}"
python3 run.py report "$OUT" > /dev/null 2>&1
echo "$(date -u +%FT%TZ) lane $LANE round $ROUND tail finished"
