#!/usr/bin/env bash
# Prepare the database host of one lane (Ubuntu 24.04, as root): PostgreSQL 18 from the PGDG
# repository, the same configuration on every lane, TLS with a throwaway CA, the benchmark dataset.
#
#   setup-db.sh <private-ip>
#
# PostgreSQL 18, not 17: zero-postgres 0.9 requires protocol 3.2 (PostgreSQL 18); every client runs
# against the same server. karez speaks protocol 3.0 and is checked on 18 here for the first time.
set -euo pipefail
PRIV="${1:?private ip}"
log() { echo "[setup-db] $*" >&2; }
export DEBIAN_FRONTEND=noninteractive
cloud-init status --wait >/dev/null 2>&1 || true
systemctl stop unattended-upgrades apt-daily.timer apt-daily-upgrade.timer 2>/dev/null || true
for _ in $(seq 30); do apt-get update -qq && break; sleep 5; done
apt-get install -y -qq curl ca-certificates gnupg openssl sysstat python3 lsb-release >/dev/null
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
log "PostgreSQL 18 (PGDG)"
install -d /usr/share/keyrings
curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc | gpg --dearmor -o /usr/share/keyrings/pgdg.gpg
echo "deb [signed-by=/usr/share/keyrings/pgdg.gpg] https://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list
apt-get update -qq
apt-get install -y -qq postgresql-18 >/dev/null
systemctl stop postgresql

log "TLS: throwaway CA and server certificate (SAN: $PRIV, localhost)"
D=/etc/postgresql/18/main
cd /root
openssl ecparam -name prime256v1 -genkey -noout -out ca.key
openssl req -new -x509 -key ca.key -sha256 -days 3 -subj "/CN=karez-bench-ca" -out ca.crt
openssl ecparam -name prime256v1 -genkey -noout -out server.key
openssl req -new -key server.key -subj "/CN=karez-bench-db" -out server.csr
printf "subjectAltName=IP:%s,DNS:localhost\nbasicConstraints=CA:FALSE\n" "$PRIV" > ext.cnf
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out server.crt -days 3 -sha256 -extfile ext.cnf 2>/dev/null
install -o postgres -g postgres -m 600 server.key $D/server.key
install -o postgres -g postgres -m 644 server.crt $D/server.crt

log "configuration"
PW="$(openssl rand -hex 16)"
echo "$PW" > /root/bench-pw
chmod 600 /root/bench-pw
cat > $D/conf.d/90-bench.conf <<EOF
listen_addresses = '127.0.0.1,$PRIV'
max_connections = 1000
shared_buffers = 2GB
effective_cache_size = 6GB
work_mem = 16MB
maintenance_work_mem = 512MB
password_encryption = scram-sha-256
ssl = on
ssl_cert_file = '$D/server.crt'
ssl_key_file = '$D/server.key'
max_wal_size = 8GB
checkpoint_timeout = 15min
jit = off
log_min_messages = warning
track_io_timing = off
EOF
# only the private network may connect; both TLS and plain connections are allowed (S7 chooses)
sed -i '/^host/d' $D/pg_hba.conf
cat >> $D/pg_hba.conf <<EOF
host    bench    bench    $PRIV/32      scram-sha-256
host    bench    bench    10.0.0.0/8    scram-sha-256
host    bench    bench    127.0.0.1/32  scram-sha-256
host    all      postgres 127.0.0.1/32  scram-sha-256
EOF
systemctl start postgresql
for _ in $(seq 30); do pg_isready -q -h 127.0.0.1 && break; sleep 1; done
sudo -u postgres psql -v ON_ERROR_STOP=1 -q <<EOF
CREATE ROLE bench LOGIN PASSWORD '$PW';
CREATE DATABASE bench OWNER bench;
EOF
log "dataset"
PGPASSWORD="$PW" psql -q -v ON_ERROR_STOP=1 -h 127.0.0.1 -U bench -d bench -f /root/karez/bench/sql/schema.sql 2>&1 | grep -v NOTICE || true
PGPASSWORD="$PW" psql -h 127.0.0.1 -U bench -d bench -tAc "select version(), (select count(*) from bench_item), (select count(*) from bench_wide)" >&2
bash /root/karez/bench/cloud/load-data.sh "$PW" "$PRIV"
