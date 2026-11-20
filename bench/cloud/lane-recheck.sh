#!/usr/bin/env bash
# The recheck of 2026-11-20 (one lane, client droplet): lane-recheck.sh LANE ROUND DB (called by lane-run.sh when PROFILE=recheck)
#   1. S4 (1,000,000 rows), the chunk path against the row view and tokio-postgres, plain and over TLS, in alternating order
#      (the exit run of 2026-11-19 found the chunk path 44% slower over TLS, which did not reproduce locally): five passes per cell,
#      three rounds with the order reversed in the middle one
#   2. S8 (512 workers on a pool of 32), plain and over TLS, after the pool fix (24b389e): karez-blocking against libpq and pgx
# Cells are tagged round "x2". Nothing here is ranked against cells from other droplets.
set -uo pipefail
LANE="$1"; ROUND="$2"; DB="$3"
export PATH=/root/.cargo/bin:/usr/local/go/bin:$PATH
export BENCH_VENV=/root/venv
cd /root/karez/bench
PW="$(cat /root/bench-pw)"
export BENCH_URL="postgresql://bench:$PW@$DB:5432/bench?sslmode=disable"
export BENCH_URL_TLS="postgresql://bench:$PW@$DB:5432/bench?sslmode=verify-full&sslrootcert=/root/ca.crt"
OUT="/root/out/lane$LANE-round$ROUND-recheck"
mkdir -p "$OUT"
cp /root/machine-cl.txt "$OUT/machine-cl.txt" 2>/dev/null
ssh -o BatchMode=yes "root@$DB" cat /root/machine-db.txt > "$OUT/machine-db.txt" 2>/dev/null
R=x2
COMMON=(--out "$OUT" --lane "lane$LANE" --round "$R" --db-host "root@$DB" --root-cert /root/ca.crt)
echo "$(date -u +%FT%TZ) lane $LANE recheck (round label $R)"
s4() { # s4 TLS(0|1) CLIENT
  local t=(); [ "$1" = 1 ] && t=(--bulk-tls)
  python3 run.py bulk "${t[@]}" --clients "$2" --only bulk-s4 --repeats 5 --s5-rows 100000 --s6-rows 10000000 "${COMMON[@]}"
}
for r in 1 2 3; do
  if [ "$r" = 2 ]; then order="tokio-postgres karez-blocking karez-blocking-chunk"; else order="karez-blocking-chunk karez-blocking tokio-postgres"; fi
  for tls in 1 0; do
    for c in $order; do echo "$(date -u +%FT%TZ) S4 round $r tls=$tls $c"; s4 $tls $c; done
  done
done
echo "$(date -u +%FT%TZ) S4 done"
python3 run.py report "$OUT" > /dev/null 2>&1
python3 run.py campaign --profile recheck --clients "karez-blocking,libpq,pgx" "${COMMON[@]}"
python3 run.py report "$OUT" > /dev/null 2>&1
echo "$(date -u +%FT%TZ) lane $LANE recheck finished"
