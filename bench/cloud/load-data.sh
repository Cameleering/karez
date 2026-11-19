#!/usr/bin/env bash
# Load the NYC taxi data on the database host and write the machine record. Usage: load-data.sh PASSWORD PRIVATE_IP
# The parquet files are expected in /tmp/taxi (shipped by `ctl.sh stage-data`: CloudFront answers 403 to the
# droplets' addresses, so the operator downloads them and copies them over).
set -euo pipefail
PW="${1:?password}"; PRIV="${2:?private ip}"
log() { echo "[load-data] $*" >&2; }
log "NYC taxi trips (public data, 4 months of 2024)"
export DEBIAN_FRONTEND=noninteractive
apt-get install -y -qq python3-venv python3-pip >/dev/null
[ -x /root/venv/bin/python ] || python3 -m venv /root/venv
/root/venv/bin/pip install -q pyarrow "psycopg[binary]"
/root/venv/bin/python /root/karez/bench/data/load_taxi.py --url "postgresql://bench:$PW@127.0.0.1/bench"   --months 2024-01,2024-02,2024-03,2024-04 --rows-file /root/taxi_rows >&2
# settings that matter for the report, written next to the results
{
  echo "host: $(hostname)  private ip: $PRIV"
  echo "kernel: $(uname -r)  cpus: $(nproc)  mem_mb: $(free -m | awk '/Mem:/{print $2}')"
  PGPASSWORD="$PW" psql -h 127.0.0.1 -U bench -d bench -tAc "select name||'='||setting from pg_settings where name in ('max_connections','shared_buffers','jit','synchronous_commit','fsync','wal_level','ssl','password_encryption','huge_pages','huge_pages_status') order by 1"
  PGPASSWORD="$PW" psql -h 127.0.0.1 -U bench -d bench -tAc "select version()"
  PGPASSWORD="$PW" psql -h 127.0.0.1 -U bench -d bench -tAc "select 'taxi_trips rows='||count(*)||' size='||pg_size_pretty(pg_total_relation_size('taxi_trips')) from taxi_trips"
} > /root/machine-db.txt
log "done"
