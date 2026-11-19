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
  "root@$(ip_of "karez-bench-l$1-cl")" "systemctl is-active karez-campaign 2>/dev/null; true" 2>/dev/null | head -1 | grep . || echo unreachable; }

# One campaign at a time: a lock file with this script's pid. Create the file STOP in the state directory to make the
# script stop waiting at its next check, pull what exists and delete the droplets (a stopped background task does not
# stop this script; the stop file does).
LOCK="$STATE/campaign.lock"
if [ -f "$LOCK" ] && kill -0 "$(cat "$LOCK" 2>/dev/null)" 2>/dev/null; then echo "another campaign.sh runs (pid $(cat "$LOCK"))" >&2; exit 1; fi
echo $$ > "$LOCK"
trap 'rm -f "$LOCK"' EXIT
rm -f "$STATE/STOP"
stop_requested() { [ -f "$STATE/STOP" ]; }

read -r -a PROFILES <<< "${1:?profiles, e.g. \"full repeat repeat\"}"
round=$(( ${ROUND_START:-1} - 1 ))   # ROUND_START=3 makes the first round of this run round 3 (client rotation follows the round number)
for profile in "${PROFILES[@]}"; do
  round=$((round + 1))
  if stop_requested; then log "STOP file present; not starting round $round"; break; fi
  log "round $round: profile $profile"
  $CTL run "$round" "$profile" >> "$LOG" 2>&1
  sleep 90   # let the units start
  lost=0
  while :; do
    states=""
    busy=0
    gone=0
    for l in ${LANES:-1 2 3}; do
      s="$(lane_state $l)"; states="$states $l=$s"
      case "$s" in active|activating) busy=1 ;; unreachable) gone=1 ;; esac
    done
    log "round $round states:$states"
    # a lane unreachable for 20 checks in a row (40 minutes) is given up on
    if [ "$gone" = 1 ]; then lost=$((lost + 1)); else lost=0; fi
    [ "$lost" -ge 20 ] && { log "a lane stayed unreachable; giving up the wait"; break; }
    [ "$busy" = 0 ] && [ "$gone" = 0 ] && break
    # STOP_AT (epoch seconds): stop waiting, pull what exists, delete the droplets before the safety net does
    if stop_requested; then log "STOP file present; pulling partial results"; break; fi
    if [ -n "${STOP_AT:-}" ] && [ "$(date +%s)" -ge "$STOP_AT" ]; then log "STOP_AT reached; pulling partial results"; break; fi
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
