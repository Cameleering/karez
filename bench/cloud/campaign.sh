#!/usr/bin/env bash
# Run the rounds of a campaign one after another on the existing droplets, pull the results after each
# round, and delete the droplets at the end. Meant to run detached (the operator's machine need not be
# watched; the safety net deletes everything at its deadline whatever happens here).
#
#   campaign.sh "full repeat repeat"        one profile per round (see bench/run.py PROFILES)
#   KEEP=1 campaign.sh "smoke"              do not delete the droplets afterwards
#
# Prerequisite: `ctl.sh up` has finished. Output: bench/target-cloud/campaign.log and the pulled results.
set -uo pipefail
export MSYS_NO_PATHCONV=1
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CTL="bash $ROOT/bench/cloud/ctl.sh"
STATE="${BENCH_STATE:-$ROOT/bench/target-cloud}"
LOG="$STATE/campaign.log"
KEY="$STATE/karez-bench-key"
log() { printf '%s %s\n' "$(date -u +%FT%TZ)" "$*" | tee -a "$LOG"; }
ip_of() { awk -v n="$1" '$1==n { print $2 }' "$STATE/droplets.txt"; }
lane_state() { ssh -i "$KEY" -o StrictHostKeyChecking=no -o UserKnownHostsFile="$STATE/known_hosts" -o BatchMode=yes -o ConnectTimeout=15 \
  "root@$(ip_of "karez-bench-l$1-cl")" "systemctl is-active karez-campaign" 2>/dev/null || echo unreachable; }

read -r -a PROFILES <<< "${1:?profiles, e.g. \"full repeat repeat\"}"
round=0
for profile in "${PROFILES[@]}"; do
  round=$((round + 1))
  log "round $round: profile $profile"
  $CTL run "$round" "$profile" >> "$LOG" 2>&1
  sleep 90   # let the units start
  lost=0
  while :; do
    states=""
    busy=0
    gone=0
    for l in 1 2 3; do
      s="$(lane_state $l)"; states="$states $l=$s"
      case "$s" in active|activating) busy=1 ;; unreachable) gone=1 ;; esac
    done
    log "round $round states:$states"
    # a lane unreachable for 20 checks in a row (40 minutes) is given up on
    if [ "$gone" = 1 ]; then lost=$((lost + 1)); else lost=0; fi
    [ "$lost" -ge 20 ] && { log "a lane stayed unreachable; giving up the wait"; break; }
    [ "$busy" = 0 ] && [ "$gone" = 0 ] && break
    bash "$ROOT/bench/cloud/safety-net.sh" due >> "$LOG" 2>&1
    sleep 120
  done
  $CTL pull >> "$LOG" 2>&1
  log "round $round pulled"
done
log "all rounds done"
if [ "${KEEP:-0}" = 1 ]; then
  log "KEEP=1: droplets left running; delete them with: bash bench/cloud/ctl.sh down"
else
  $CTL down >> "$LOG" 2>&1
  log "droplets deleted"
fi
