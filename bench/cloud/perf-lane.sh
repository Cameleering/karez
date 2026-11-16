#!/usr/bin/env bash
# The performance experiments of docs/design/perf-program.md on one lane, run on the client droplet:
#   perf-lane.sh DB_PRIVATE_IP
# Output: /root/out/perf-lane1/ (run.py cells, floor.txt, copyvs.txt, iperf.txt). Needs ctl.sh up, stage-data, load-data and ship.
set -uo pipefail
DB="$1"
export PATH=/root/.cargo/bin:/usr/local/go/bin:$PATH
export BENCH_VENV=/root/venv
cd /root/karez/bench
PW="$(cat /root/bench-pw)"
export BENCH_URL="postgresql://bench:$PW@$DB:5432/bench?sslmode=disable"
export BENCH_TAXI_ROWS="$(cat /root/taxi_rows)"
OUT=/root/out/perf-lane1
mkdir -p "$OUT"
cp /root/machine-cl.txt "$OUT/machine-cl.txt" 2>/dev/null
ssh -o BatchMode=yes "root@$DB" cat /root/machine-db.txt > "$OUT/machine-db.txt" 2>/dev/null
say() { echo "$(date -u +%FT%TZ) $*"; }
bulk() { # bulk LABEL CLIENT SCENARIO REPEATS [ENV=VALUE...]
  local label="$1" client="$2" sc="$3" reps="$4"; shift 4
  say "bulk $label ($client $sc)"
  env "$@" python3 run.py bulk --clients "$client" --only "$sc" --repeats "$reps" --db-host "root@$DB" \
    --s5-rows 100000 --out "$OUT/$label" --lane perf --round 1 2>&1 | tail -3
}

say "network: iperf3 (client to database host, private network)"
export DEBIAN_FRONTEND=noninteractive
apt-get install -y -qq iperf3 >/dev/null 2>&1
ssh -o BatchMode=yes "root@$DB" "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq iperf3" >/dev/null 2>&1
ssh -o BatchMode=yes "root@$DB" "pkill iperf3; iperf3 -s -D" >/dev/null 2>&1
{ echo "# 1 stream, 10 s"; iperf3 -c "$DB" -t 10 2>&1 | tail -4; echo "# 4 streams, 10 s"; iperf3 -c "$DB" -t 10 -P 4 2>&1 | tail -4; echo "# rtt"; ping -c 20 -q "$DB" | tail -2; } > "$OUT/iperf.txt" 2>&1
ssh -o BatchMode=yes "root@$DB" "pkill iperf3" >/dev/null 2>&1

say "latency floor (trust role 'floor', one connection, closed loop)"
ssh -o BatchMode=yes "root@$DB" "sudo -u postgres psql -d bench -q -c \"CREATE ROLE floor LOGIN\" -c \"GRANT SELECT ON ALL TABLES IN SCHEMA public TO floor\"; echo 'host bench floor 10.0.0.0/8 trust' >> /etc/postgresql/18/main/pg_hba.conf; systemctl reload postgresql@18-main" >/dev/null 2>&1
for i in 1 2 3; do ./target/release/floor "postgresql://floor@$DB:5432/bench?sslmode=disable" 100000; done > "$OUT/floor.txt" 2>&1

say "COPY binary versus row messages (S4 table)"
ssh -o BatchMode=yes "root@$DB" "sudo -u postgres psql -d bench -q -c \"GRANT SELECT ON ALL TABLES IN SCHEMA public TO floor\"" >/dev/null 2>&1
./target/release/copyvs "postgresql://bench:$PW@$DB:5432/bench?sslmode=disable" 4 > "$OUT/copyvs.txt" 2>&1

say "single-connection scans, interleaved three rounds (A B C A B C ...) so a slow minute does not land on one client"
for r in 1 2 3; do
  for c in karez-blocking karez-blocking-chunk tokio-postgres pgx sqlx libpq; do bulk "s4-$c-r$r" "$c" s4 3; done
  for c in karez-blocking-chunk pgx tokio-postgres; do bulk "s8-$c-r$r" "$c" s8 2; done
done

say "partitioned scans: S4 and S8, k = 2 4 8 16 (32 on the larger classes), snapshot off; k=8 with snapshot"
for k in 2 4 8 16 32; do bulk "s4-part$k" karez-blocking-part s4 5 KAREZ_PARTS=$k KAREZ_SNAPSHOT=0; done
for k in 2 4 8 16 32; do bulk "s8-part$k" karez-blocking-part s8 3 KAREZ_PARTS=$k KAREZ_SNAPSHOT=0; done
bulk "s8-part8-snap" karez-blocking-part s8 3 KAREZ_PARTS=8 KAREZ_SNAPSHOT=1

say "S5, 100k rows: per-row, multi-row 1000, sqlx"
for c in karez-blocking sqlx; do bulk "s5-$c" "$c" s5 5; done
bulk "s5-mrow1000" karez-blocking-mrow s5 5 KAREZ_MROW_BATCH=1000

say "done"
