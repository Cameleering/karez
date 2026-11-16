#!/usr/bin/env bash
# Operator of the karez benchmark campaign on DigitalOcean (run from the owner's machine, Git Bash).
#
#   ctl.sh up              create 6 droplets (3 lanes x db + client), ship the tree, set every node up
#   ctl.sh stage-data      download the NYC taxi parquet files here and copy them to the db hosts
#   ctl.sh load-data       load them into PostgreSQL on every lane (also called by setup)
#   ctl.sh ship            ship the current tree again and rebuild the adapters on the clients
#   ctl.sh status          droplets, setup state, running campaign units, safety-net deadline
#   ctl.sh run ROUND PROFILE   start the campaign of one round on every lane (detached on the droplets)
#   ctl.sh tail LANE       last lines of a lane's campaign log
#   ctl.sh pull            copy results from the clients to bench/results/<campaign>/
#   ctl.sh ssh LANE db|cl  open a shell
#   ctl.sh down            final pull, then delete the droplets and the SSH key (droplets.sh down)
#
# Layout: lane N has karez-bench-lN-db (PostgreSQL 18, private IP) and karez-bench-lN-cl (the clients
# under test). Clients rotate across lanes between rounds so every client meets every droplet pair;
# karez runs in every lane and round (docs/design/plan.md 6.2, simoom's benchmark-review.md 5).
#
# Environment: DO_TOKEN_FILE (token file, never printed), CAMPAIGN (label, default campaign1),
# SAFETY_HOURS (hard deadline for deletion, default 10, at most 10).
set -uo pipefail
export MSYS_NO_PATHCONV=1
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CLOUD="$ROOT/bench/cloud"
STATE="${BENCH_STATE:-$ROOT/bench/target-cloud}"
export BENCH_STATE="$STATE"
CAMPAIGN="${CAMPAIGN:-campaign1}"
OUT="$ROOT/bench/results/$(date -u +%F)-$CAMPAIGN"
KEY="$STATE/karez-bench-key"
DO_TOKEN_FILE="${DO_TOKEN_FILE:-C:\\Users\\SudheeshPM\\Documents\\do.config.txt}"
export DO_TOKEN_FILE
# LANES="1" runs one lane only (two droplets: karez-bench-l1-db and karez-bench-l1-cl); default is all three
LANES="${LANES:-1 2 3}"
DO_NAMES=""; for _l in $LANES; do DO_NAMES="$DO_NAMES karez-bench-l$_l-db karez-bench-l$_l-cl"; done
export DO_NAMES="${DO_NAMES# }"
mkdir -p "$STATE"
SSH_OPTS=(-i "$KEY" -o StrictHostKeyChecking=no -o UserKnownHostsFile="$STATE/known_hosts" -o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=30)

# an overdue safety net fires before anything else
bash "$CLOUD/safety-net.sh" due 2>&1 | grep -v "not armed" >&2 || true

ip_of() { # ip_of NAME [public|private]
  awk -v n="$1" -v c="${2:-public}" '$1==n { print (c=="public" ? $2 : $3) }' "$STATE/droplets.txt"
}
rssh() { # rssh NAME cmd...
  local n="$1"; shift
  ssh "${SSH_OPTS[@]}" "root@$(ip_of "$n")" "$@"
}
rscp() { scp "${SSH_OPTS[@]}" "$@"; }

# the client groups; karez runs in every lane and round
KAREZ="karez-blocking,karez-shared,karez-blocking-chunk"
GROUPS_=("libpq,tokio-postgres,pgjdbc,asyncpg" "deadpool-postgres,pgx,vertx-pg-client,psycopg3,sqlx-tuned" "sqlx,pgjdbc-tuned,r2dbc-postgresql,zero-postgres")

pack() {
  local tarball="$STATE/karez-tree.tar.gz"
  ( cd "$ROOT" && tar --exclude=target --exclude=.git --exclude='bench/target-cloud' --exclude='bench/results' --exclude=certs \
      -czf "$tarball" bench crates Cargo.toml Cargo.lock docs/design/plan.md ) || return 1
  echo "$tarball"
}

case "${1:-status}" in
  up)
    [ -f "$KEY" ] || ssh-keygen -q -t ed25519 -N '' -C karez-bench -f "$KEY"
    DO_SSH_PUB="$KEY.pub" bash "$CLOUD/droplets.sh" up || exit 1
    bash "$CLOUD/safety-net.sh" arm "${SAFETY_HOURS:-10}" || echo "WARNING: safety net not armed" >&2
    tarball="$(pack)" || exit 1
    echo "waiting for ssh"
    while read -r name pub priv status _ <&3; do
      for _ in $(seq 60); do rssh "$name" true 2>/dev/null </dev/null && break; sleep 5; done
    done 3< "$STATE/droplets.txt"
    # private IPs of the db hosts go to every node
    for l in $LANES; do echo "$(ip_of karez-bench-l$l-db private)"; done > "$STATE/db-private-ips.txt"
    mkdir -p "$STATE/logs"
    pids=()
    while read -r name pub priv status _ <&3; do
      ( rscp "$tarball" "root@$pub:/root/karez-tree.tar.gz" >/dev/null &&
        rssh "$name" "mkdir -p /root/karez && tar xzf /root/karez-tree.tar.gz -C /root/karez && find /root/karez/bench -type f \( -name '*.sh' -o -name '*.env' -o -name '*.py' -o -name '*.json' \) -exec sed -i 's/\r\$//' {} +" &&
        case "$name" in
          *-db) rssh "$name" "bash /root/karez/bench/cloud/setup-db.sh $priv" ;;
          *-cl) rssh "$name" "bash /root/karez/bench/cloud/setup-client.sh" ;;
        esac ) > "$STATE/logs/setup-$name.log" 2>&1 &
      pids+=($!)
    done 3< "$STATE/droplets.txt"
    fail=0; for p in "${pids[@]}"; do wait "$p" || fail=1; done
    echo "setup finished (failures: $fail); logs in $STATE/logs"
    [ "$fail" = 0 ] || { tail -n 5 "$STATE"/logs/setup-*.log; exit 1; }
    bash "$0" link || exit 1
    echo "nodes ready"
    ;;
  ship)
    # ship the current tree again and rebuild the adapters on the clients (after a code change)
    tarball="$(pack)" || exit 1
    while read -r name pub priv status _ <&3; do
      rscp "$tarball" "root@$pub:/root/karez-tree.tar.gz" >/dev/null &&
      rssh "$name" "tar xzf /root/karez-tree.tar.gz -C /root/karez && find /root/karez/bench -type f \( -name '*.sh' -o -name '*.env' -o -name '*.py' -o -name '*.json' \) -exec sed -i 's/\r\$//' {} +" || echo "ship to $name failed" >&2
    done 3< "$STATE/droplets.txt"
    for l in $LANES; do
      ( rssh "karez-bench-l$l-cl" "bash /root/karez/bench/cloud/build-adapters.sh" > "$STATE/logs/build-l$l.log" 2>&1 || echo "build on lane $l failed (see logs/build-l$l.log)" >&2 ) &
    done
    wait
    echo "shipped and rebuilt"
    ;;
  link)
    # copy the client's trust root (the db's throwaway CA) to its client
    for l in $LANES; do
      rscp "root@$(ip_of karez-bench-l$l-db):/root/ca.crt" "$STATE/ca-l$l.crt" >/dev/null &&
      rscp "$STATE/ca-l$l.crt" "root@$(ip_of karez-bench-l$l-cl):/root/ca.crt" >/dev/null
      rssh "karez-bench-l$l-db" "cat /root/taxi_rows" | rssh "karez-bench-l$l-cl" "cat > /root/taxi_rows"
      rssh "karez-bench-l$l-db" "cat /root/bench-pw" | rssh "karez-bench-l$l-cl" "cat > /root/bench-pw && chmod 600 /root/bench-pw"
      # the client samples its database host's CPU over ssh: the bench key (throwaway, deleted with the droplets)
      rscp "$KEY" "root@$(ip_of karez-bench-l$l-cl):/root/.ssh/id_ed25519" >/dev/null
      rssh "karez-bench-l$l-cl" "chmod 600 /root/.ssh/id_ed25519"
    done
    echo "nodes linked"
    ;;
  stage-data)
    # the NYC taxi parquet files: downloaded here (CloudFront refuses the droplets), copied to every db host
    mkdir -p "$STATE/taxi"
    for m in 2024-01 2024-02 2024-03 2024-04; do
      f="$STATE/taxi/yellow_$m.parquet"
      [ -s "$f" ] || curl -sSL -o "$f" "https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_$m.parquet"
    done
    for l in $LANES; do
      rssh "karez-bench-l$l-db" "mkdir -p /tmp/taxi" && rscp "$STATE"/taxi/yellow_*.parquet "root@$(ip_of karez-bench-l$l-db):/tmp/taxi/" > /dev/null &&
        echo "lane $l: data staged"
    done
    ;;
  load-data)
    for l in $LANES; do
      pw="$(rssh "karez-bench-l$l-db" "cat /root/bench-pw")"
      priv="$(ip_of karez-bench-l$l-db private)"
      ( rssh "karez-bench-l$l-db" "bash /root/karez/bench/cloud/load-data.sh $pw $priv" > "$STATE/logs/load-l$l.log" 2>&1 && echo "lane $l: data loaded" || echo "lane $l: load FAILED (logs/load-l$l.log)" ) &
    done
    wait
    ;;
  status)
    bash "$CLOUD/droplets.sh" status 2>&1
    bash "$CLOUD/safety-net.sh" status 2>&1 | tail -4
    if [ -f "$STATE/droplets.txt" ]; then
      for l in $LANES; do
        echo "lane $l:"; rssh "karez-bench-l$l-cl" "systemctl is-active karez-campaign 2>&1; ls /root/out 2>/dev/null | tail -3; tail -n 2 /root/campaign.log 2>/dev/null" 2>&1 | sed 's/^/  /'
      done
    fi
    ;;
  run)
    round="${2:?round}"; profile="${3:?profile (full|repeat|quick|smoke)}"
    for l in $LANES; do
      g=$(( (l - 1 + round - 1) % 3 ))      # lane l runs group (l + round) mod 3
      clients="$KAREZ,${GROUPS_[$g]}"
      dbp="$(ip_of karez-bench-l$l-db private)"
      echo "round $round lane $l: $clients"
      rssh "karez-bench-l$l-cl" "systemctl stop karez-campaign 2>/dev/null; systemctl reset-failed karez-campaign 2>/dev/null; \
        systemd-run -q --unit=karez-campaign --collect -p LimitNOFILE=1048576 -p StandardOutput=append:/root/campaign.log -p StandardError=append:/root/campaign.log \
        bash /root/karez/bench/cloud/lane-run.sh $l $round $profile $dbp '$clients'" || echo "lane $l failed to start" >&2
    done
    ;;
  tail)
    l="${2:?lane}"; rssh "karez-bench-l$l-cl" "tail -n ${3:-25} /root/campaign.log"
    ;;
  pull)
    mkdir -p "$OUT"
    for l in $LANES; do
      n="karez-bench-l$l-cl"
      [ -n "$(ip_of $n)" ] || continue
      mkdir -p "$OUT/lane$l"
      rssh "$n" "cd /root && tar czf - --ignore-failed-read --exclude='*-smoke' out campaign.log machine-cl.txt 2>/dev/null; true" | tar xzf - -C "$OUT/lane$l" 2>/dev/null || echo "lane $l: nothing to pull yet" >&2
      rssh "karez-bench-l$l-db" "cat /root/machine-db.txt" > "$OUT/lane$l/machine-db.txt" 2>/dev/null || true
    done
    echo "pulled to $OUT"
    ;;
  ssh)
    exec ssh "${SSH_OPTS[@]}" "root@$(ip_of "karez-bench-l${2:?lane}-${3:?db|cl}")"
    ;;
  down)
    bash "$0" pull || true
    bash "$CLOUD/droplets.sh" down
    bash "$CLOUD/safety-net.sh" disarm 2>&1 | tail -2
    ;;
  *) sed -n 2,20p "$0"; exit 1 ;;
esac
