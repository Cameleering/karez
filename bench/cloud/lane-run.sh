#!/usr/bin/env bash
# One lane's campaign round, on the client droplet: lane-run.sh LANE ROUND PROFILE DB_PRIVATE_IP CLIENTS
set -uo pipefail
LANE="$1"; ROUND="$2"; PROFILE="$3"; DB="$4"; CLIENTS="$5"
export PATH=/root/.cargo/bin:/usr/local/go/bin:$PATH
export BENCH_VENV=/root/venv
cd /root/karez/bench
PW="$(cat /root/bench-pw)"
export BENCH_URL="postgresql://bench:$PW@$DB:5432/bench?sslmode=disable"
export BENCH_URL_TLS="postgresql://bench:$PW@$DB:5432/bench?sslmode=verify-full&sslrootcert=/root/ca.crt"
export BENCH_TAXI_ROWS="$(cat /root/taxi_rows)"
if [ "$PROFILE" = tail ]; then exec bash /root/karez/bench/cloud/lane-tail.sh "$LANE" "$ROUND" "$DB"; fi
OUT="/root/out/lane$LANE-round$ROUND-$PROFILE"
mkdir -p "$OUT"
cp /root/machine-cl.txt "$OUT/machine-cl.txt" 2>/dev/null
ssh -o BatchMode=yes "root@$DB" cat /root/machine-db.txt > "$OUT/machine-db.txt" 2>/dev/null
echo "$(date -u +%FT%TZ) lane $LANE round $ROUND profile $PROFILE clients $CLIENTS"
if [ "$PROFILE" = full ] || [ "$PROFILE" = smoke ] || [ "$PROFILE" = lean2 ]; then
  # every client must produce the same checksum on the same request sequence (see run.py verify)
  python3 run.py verify --clients "$CLIENTS" --root-cert /root/ca.crt > "$OUT/verify.txt" 2>&1
  echo "verify: rc=$? (see verify.txt)"
fi
if [ "${XCHECK:-0}" = 1 ]; then
  # pgbench cross-check of the open-loop method (bench/crosscheck.py), S1 and S2, low rates, on this droplet pair
  for sc in s1 s2; do
    python3 crosscheck.py --url "$BENCH_URL" --pgbench pgbench --rates 1000,2000 --conns 8 --duration 20 --scenario $sc --jobs 4 > "$OUT/crosscheck-$sc.txt" 2>&1
    echo "crosscheck $sc: rc=$? (see crosscheck-$sc.txt)"
  done
fi
python3 run.py campaign --profile "$PROFILE" --out "$OUT" --clients "$CLIENTS" --lane "lane$LANE" --round "$ROUND" \
  --db-host "root@$DB" --root-cert /root/ca.crt
rc=$?
python3 run.py report "$OUT" > /dev/null 2>&1
echo "$(date -u +%FT%TZ) lane $LANE round $ROUND finished, rc=$rc"
