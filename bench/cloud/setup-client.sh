#!/usr/bin/env bash
# Prepare the client host of one lane (Ubuntu 24.04, as root, from /root/karez): toolchains and every
# adapter. Pinned in bench/versions.env; crates by bench/Cargo.lock, Go by go.sum, Java by the pom.
set -euo pipefail
log() { echo "[setup-client] $*" >&2; }
export DEBIAN_FRONTEND=noninteractive
. /root/karez/bench/versions.env
cloud-init status --wait >/dev/null 2>&1 || true
systemctl stop unattended-upgrades apt-daily.timer apt-daily-upgrade.timer 2>/dev/null || true
for _ in $(seq 30); do apt-get update -qq && break; sleep 5; done
apt-get install -y -qq build-essential pkg-config curl ca-certificates gnupg lsb-release sysstat python3 python3-venv python3-pip \
  openjdk-21-jdk-headless maven libssl-dev git openssl >/dev/null
log "kernel settings (the same on every node)"
cat > /etc/sysctl.d/90-bench.conf <<'EOF'
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
net.ipv4.ip_local_port_range = 10240 65535
net.ipv4.tcp_tw_reuse = 1
net.core.netdev_max_backlog = 65536
fs.file-max = 4194304
fs.nr_open = 4194304
EOF
sysctl -q --system
log "libpq and pgbench (PostgreSQL 18 client tools from PGDG)"
install -d /usr/share/keyrings
curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc | gpg --dearmor -o /usr/share/keyrings/pgdg.gpg
echo "deb [signed-by=/usr/share/keyrings/pgdg.gpg] https://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list
apt-get update -qq
apt-get install -y -qq libpq-dev postgresql-client-18 postgresql-18 >/dev/null
systemctl disable --now postgresql >/dev/null 2>&1 || true   # only the tools are wanted
log "Rust $RUST_VERSION"
if ! command -v cargo >/dev/null; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "$RUST_VERSION" >/dev/null
fi
export PATH=/root/.cargo/bin:$PATH
rustc -V >&2
log "Go $GO_VERSION (checksum verified)"
if [ ! -x /usr/local/go/bin/go ]; then
  curl -fsSL "https://go.dev/dl/$GO_VERSION.linux-amd64.tar.gz" -o /tmp/go.tgz
  want="$(curl -fsSL 'https://go.dev/dl/?mode=json&include=all' | python3 -c "
import json,sys
for r in json.load(sys.stdin):
    if r['version']=='$GO_VERSION':
        for f in r['files']:
            if f['os']=='linux' and f['arch']=='amd64' and f['kind']=='archive': print(f['sha256'])")"
  echo "$want  /tmp/go.tgz" | sha256sum -c - >&2
  tar -C /usr/local -xzf /tmp/go.tgz
fi
export PATH=/usr/local/go/bin:$PATH
go version >&2
log "Python packages"
python3 -m venv /root/venv
/root/venv/bin/pip install -q "asyncpg==$ASYNCPG_VERSION" "psycopg[binary,pool]==$PSYCOPG_VERSION" "psycopg-pool==$PSYCOPG_POOL_VERSION" "uvloop==$UVLOOP_VERSION"
log "building the adapters"
cd /root/karez/bench
cargo build --release -q 2>&1 | tail -5 >&2
(cd adapters/go && go mod download && go build -o karez-bench-go .)
(cd adapters/java && mvn -q -B package -DskipTests 2>&1 | tail -5 >&2)
(cd adapters/c && cc -O2 -Wall -o libpq_adapter libpq_adapter.c -I"$(pg_config --includedir)" -L"$(pg_config --libdir)" -lpq -lpthread -lm)
# keys: the clients sample the database host's CPU over ssh
install -d -m 700 /root/.ssh
printf 'Host *\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n  LogLevel ERROR\n' > /root/.ssh/config
{
  echo "host: $(hostname)  kernel: $(uname -r)  cpus: $(nproc)  mem_mb: $(free -m | awk '/Mem:/{print $2}')"
  echo "cpu: $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2)"
  rustc -V; cargo -V; go version; java -version 2>&1 | head -1; mvn -v 2>/dev/null | head -1
  /root/venv/bin/python -V; /root/venv/bin/pip list 2>/dev/null | grep -E 'asyncpg|psycopg|uvloop'
  psql --version; pgbench --version
} > /root/machine-cl.txt 2>&1
log "done"
