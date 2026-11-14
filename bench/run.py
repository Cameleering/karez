#!/usr/bin/env python3
"""Orchestrator of the karez benchmark: runs adapter cells, searches the SLO rate, builds tables.

Standard library only. Method: docs/design/plan.md section 6 and bench/README.md.

  run.py setup  --psql "psql URL"                load the dataset (bench/sql/schema.sql)
  run.py cell   --client C --scenario s2 ...     one cell, printed
  run.py sweep  --client C --scenario s2 ...     search the SLO throughput of one configuration
  run.py campaign --profile smoke|quick|full --out DIR   every configuration of the profile
  run.py report DIR                              tables from the raw files in DIR

Every cell appends one JSON line (adapter output plus orchestrator measurements) to
DIR/cells.jsonl. Nothing is overwritten and nothing is averaged away: the report reads the raw lines.
"""
import argparse, json, os, platform, resource, statistics, subprocess, sys, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))
CLIENTS = json.load(open(os.path.join(HERE, "clients.json")))
CLIENTS = {k: v for k, v in CLIENTS.items() if not k.startswith("_")}

# Scenario table. bound_ms: the p99 bound the SLO throughput is searched against (plan 6.1).
# The bounds are fixed before measuring and apply to every client alike.
SCENARIOS = {
    "s1": dict(sql="SELECT 1", bound_ms=2.0),
    "s2": dict(sql="point select", bound_ms=2.0),
    "s3": dict(sql="12-row select", bound_ms=3.0),
    "s7": dict(sql="point select over TLS", bound_ms=2.0),
}


# ---- histogram (same layout as rustbench/src/hist.rs) ---------------------------------------------

def lower_bound(idx):
    if idx < 128:
        return idx
    shift = (idx >> 6) - 1
    return (idx - (shift << 6)) << shift


def bucket_mid(idx):
    lo = lower_bound(idx)
    hi = lower_bound(idx + 1)
    return (lo + hi) // 2


def percentile(pairs, q):
    total = sum(c for _, c in pairs)
    if total == 0:
        return None
    target = max(1, int(round(q * total)))
    run = 0
    for i, c in sorted(pairs):
        run += c
        if run >= target:
            return bucket_mid(i)
    return bucket_mid(sorted(pairs)[-1][0])


# ---- running one cell -------------------------------------------------------------------------------

def proc_cpu(pid):
    try:
        f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
        return (int(f[11]) + int(f[12])) / os.sysconf("SC_CLK_TCK")
    except Exception:
        return None


def stat_busy(host):
    """Busy and total jiffies of a host's CPUs from /proc/stat, locally or over ssh."""
    cmd = ["cat", "/proc/stat"] if host in (None, "local") else ["ssh", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", host, "cat /proc/stat"]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=10).stdout
        v = [int(x) for x in out.splitlines()[0].split()[1:]]
        idle = v[3] + v[4]
        return sum(v) - idle, sum(v)
    except Exception:
        return None


def run_cell(a, client, scenario, mode, conns, rate, duration, warmup, pool=0, dispatch="shared",
             db_host=None, extra=None, probe=False, tls=False):
    spec = CLIENTS[client]
    # JIT-compiled clients need a longer warm-up (plan 6.2: 60 s for JVM load tests; probes use half)
    warmup = max(warmup, spec.get("warmup_min", 0) * (0.5 if probe else 1))
    exe = spec["cmd"][0]
    if not os.path.isabs(exe):
        exe = os.path.join(HERE, exe)
    cmd = [exe] + spec["cmd"][1:] + [
        "--client", client, "--url", (a.url_tls if tls and a.url_tls else a.url), "--scenario", scenario, "--mode", mode,
        "--conns", str(conns), "--pool", str(pool), "--rate", str(rate),
        "--duration", str(duration), "--warmup", str(warmup), "--dispatch", dispatch,
        "--rt-threads", str(a.rt_threads),
    ]
    if a.root_cert:
        cmd += ["--root-cert", a.root_cert]
    if extra:
        cmd += extra
    t_start = time.monotonic()
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    marks = {}
    err_lines = []

    def read_err():
        for line in p.stderr:
            if line.strip() == "T0" and "t0" not in marks:
                marks["t0"] = time.monotonic()
            else:
                err_lines.append(line.rstrip())
    th = threading.Thread(target=read_err, daemon=True)
    th.start()
    out_buf = []
    th2 = threading.Thread(target=lambda: out_buf.append(p.stdout.read()), daemon=True)
    th2.start()
    cpu0 = cpu1 = None
    db0 = db1 = None
    # sample process CPU (and the database host) at the start and end of the measured window
    while p.poll() is None and "t0" not in marks:
        time.sleep(0.005)
    if "t0" in marks and p.poll() is None:
        t0 = marks["t0"]
        while time.monotonic() < t0 + warmup and p.poll() is None:
            time.sleep(0.002)
        if p.poll() is None:
            cpu0 = proc_cpu(p.pid)
            ts0 = time.monotonic()
            db0 = stat_busy(db_host) if db_host else None
        # sample a little before the end: the process exits right after the window closes
        while time.monotonic() < t0 + warmup + duration - 0.08 and p.poll() is None:
            time.sleep(0.002)
        if p.poll() is None:
            cpu1 = proc_cpu(p.pid)
            ts1 = time.monotonic()
            db1 = stat_busy(db_host) if db_host else None
    p.wait()
    th.join(timeout=5)
    th2.join(timeout=5)
    out = "".join(out_buf).strip()
    wall = time.monotonic() - t_start
    if p.returncode != 0 or not out:
        return {"failed": True, "client": client, "scenario": scenario, "mode": mode, "conns": conns,
                "rate": rate, "returncode": p.returncode, "stderr": "\n".join(err_lines[-10:])}
    r = json.loads(out.splitlines()[-1])
    r["orch_wall_s"] = round(wall, 2)
    if cpu0 is not None and cpu1 is not None:
        r["cpu_window_s"] = round(cpu1 - cpu0, 3)
        r["cpu_window_dt"] = round(ts1 - ts0, 3)
    if db0 and db1:
        r["db_cpu_busy_frac"] = round((db1[0] - db0[0]) / max(1, db1[1] - db0[1]), 4)
    return r


def run_bulk(a, client, scenario, repeats, rows):
    """One bulk cell (closed loop): the adapter prints seconds and CPU-seconds of each timed pass."""
    spec = CLIENTS[client]
    exe = spec["cmd"][0]
    if not os.path.isabs(exe):
        exe = os.path.join(HERE, exe)
    cmd = [exe] + spec["cmd"][1:] + ["--client", client, "--url", a.url, "--scenario", scenario, "--repeats", str(repeats),
                                      "--bulk-rows", str(rows), "--rt-threads", str(a.rt_threads)]
    t = time.monotonic()
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=3600)
    except subprocess.TimeoutExpired:
        return {"failed": True, "client": client, "scenario": scenario, "stderr": "timeout"}
    if p.returncode != 0 or not p.stdout.strip():
        return {"failed": True, "client": client, "scenario": scenario, "returncode": p.returncode,
                "stderr": (p.stderr or "")[-600:]}
    r = json.loads(p.stdout.strip().splitlines()[-1])
    r["orch_wall_s"] = round(time.monotonic() - t, 1)
    return r


def summarize(r):
    """Derived numbers of one raw cell."""
    if r.get("failed"):
        return r
    h = r["hist"]
    s = dict(
        client=r["client"], scenario=(r.get("label") or r["scenario"]).lower(), mode=r["mode"].lower(), conns=r["conns"],
        pool=r["pool"], rate=r["rate"], sent=r["sent"], ok=r["ok"], errors=r["errors"], dropped=r["dropped"],
        p50_us=(percentile(h, .50) or 0) / 1e3, p99_us=(percentile(h, .99) or 0) / 1e3,
        p999_us=(percentile(h, .999) or 0) / 1e3, rss_mb=round(r["rss_kb"] / 1024, 1),
        achieved=r["ok"] / r["duration_s"],
    )
    if "cpu_window_s" in r and r["ok"] and r.get("cpu_window_dt"):
        reqs = r["ok"] / r["duration_s"] * r["cpu_window_dt"]
        s["cpu_s_per_mreq"] = round(r["cpu_window_s"] / reqs * 1e6, 3)
    if "db_cpu_busy_frac" in r:
        s["db_cpu_busy_frac"] = r["db_cpu_busy_frac"]
    return s


def passes(s, bound_ms):
    return (not s.get("failed") and s["errors"] == 0 and s["dropped"] == 0
            and s["ok"] >= 0.995 * s["sent"] and s["p99_us"] <= bound_ms * 1000)


def machine_info():
    info = dict(host=platform.node(), kernel=platform.release(), python=platform.python_version(),
                cpus=os.cpu_count())
    try:
        for l in open("/proc/cpuinfo"):
            if l.startswith("model name"):
                info["cpu_model"] = l.split(":", 1)[1].strip()
                break
        info["governor"] = open("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").read().strip()
    except Exception:
        pass
    try:
        info["mem_mb"] = int(open("/proc/meminfo").readline().split()[1]) // 1024
    except Exception:
        pass
    return info


# ---- the SLO search ---------------------------------------------------------------------------------

class Log:
    def __init__(self, out):
        self.out = out
        os.makedirs(out, exist_ok=True)
        self.path = os.path.join(out, "cells.jsonl")

    def add(self, raw, **tags):
        raw = dict(raw)
        raw.update(tags)
        with open(self.path, "a") as f:
            f.write(json.dumps(raw) + "\n")


def sweep(a, log, client, scenario, mode, conns, pool=0, dispatch="shared", hint=None, tags=None, tls=False):
    """Highest rate whose p99 stays under the scenario bound: geometric ramp, then bisection with
    short probes, then `a.repeats` full runs at the chosen rate (stepping down 5% if the median fails).
    Returns the summaries of the final runs and the chosen rate."""
    tags = dict(tags or {})
    bound = SCENARIOS[scenario]["bound_ms"]
    rate = hint or a.start_rate
    lo, hi = 0.0, None

    def probe(rt):
        raw = run_cell(a, client, scenario, mode, conns, rt, a.probe_s, a.probe_warm, pool, dispatch, probe=True, tls=tls)
        log.add(raw, phase="probe", **tags)
        s = summarize(raw)
        print(f"    probe {rt:>9.0f}/s: " + ("FAILED " + str(raw.get("stderr", ""))[:200] if s.get("failed") else
              f"p99 {s['p99_us']:.0f}us ok {s['ok']}/{s['sent']} err {s['errors']} drop {s['dropped']}"), flush=True)
        return s

    # ramp
    for _ in range(14):
        s = probe(rate)
        if s.get("failed"):
            return [], 0.0
        if passes(s, bound):
            lo = rate
            rate *= 2
        else:
            hi = rate
            break
        if rate > a.max_rate:
            break
    if lo == 0:
        # even the first rate failed: search downwards
        while rate > 20 and lo == 0:
            rate /= 2
            s = probe(rate)
            if s.get("failed"):
                return [], 0.0
            if passes(s, bound):
                lo = rate
        hi = hi or rate * 2
    # bisect
    if hi:
        for _ in range(a.bisect_steps):
            mid = (lo + hi) / 2
            if hi - lo < 0.04 * lo:
                break
            s = probe(mid)
            if passes(s, bound):
                lo = mid
            else:
                hi = mid
    rate = lo
    # confirm with full runs; step down while the median p99 fails
    for _ in range(6):
        runs = []
        for k in range(a.repeats):
            raw = run_cell(a, client, scenario, mode, conns, rate, a.duration, a.warmup, pool, dispatch, db_host=a.db_host, tls=tls)
            log.add(raw, phase="final", rep=k, slo_rate_candidate=rate, **tags)
            runs.append(summarize(raw))
        good = [x for x in runs if not x.get("failed")]
        med_p99 = statistics.median(x["p99_us"] for x in good) if good else 1e12
        okc = sum(1 for x in good if passes(x, bound))
        print(f"    confirm {rate:>9.0f}/s: median p99 {med_p99:.0f}us, {okc}/{len(runs)} runs inside the bound", flush=True)
        if good and med_p99 <= bound * 1000 and okc >= (len(runs) + 1) // 2:
            return good, rate
        rate *= 0.95
        if rate < 20:
            break
    return [], 0.0


# ---- commands ----------------------------------------------------------------------------------------

def cmd_setup(a):
    sql = open(os.path.join(HERE, "sql", "schema.sql")).read()
    r = subprocess.run(a.psql, input=sql, shell=True, text=True, capture_output=True)
    print(r.stdout[-500:], r.stderr[-500:])
    sys.exit(r.returncode)


def common_args(p):
    p.add_argument("--url", default=os.environ.get("BENCH_URL"), help="postgresql:// URL (or env BENCH_URL)")
    p.add_argument("--url-tls", default=os.environ.get("BENCH_URL_TLS"), help="URL with sslmode=verify-full, for S7 (or env BENCH_URL_TLS)")
    p.add_argument("--root-cert", default=None)
    p.add_argument("--rt-threads", type=int, default=4)
    p.add_argument("--db-host", default=None, help="ssh target of the database host, to sample its CPU")
    p.add_argument("--duration", type=float, default=15)
    p.add_argument("--warmup", type=float, default=3)
    p.add_argument("--probe-s", type=float, default=4)
    p.add_argument("--probe-warm", type=float, default=1)
    p.add_argument("--repeats", type=int, default=5)
    p.add_argument("--start-rate", type=float, default=2000)
    p.add_argument("--max-rate", type=float, default=400000)
    p.add_argument("--bisect-steps", type=int, default=5)


def cmd_cell(a):
    raw = run_cell(a, a.client, a.scenario, a.mode, a.conns, a.rate, a.duration, a.warmup, a.pool, a.dispatch, db_host=a.db_host)
    Log(a.out).add(raw, phase="manual")
    print(json.dumps(summarize(raw), indent=1))


def cmd_sweep(a):
    log = Log(a.out)
    runs, rate = sweep(a, log, a.client, a.scenario, a.mode, a.conns, a.pool, a.dispatch)
    print(f"SLO rate {rate:.0f}/s; final runs p99(us):", [round(r['p99_us']) for r in runs])


PROFILES = {
    # smoke: finishes in a couple of minutes, only proves the machinery works
    "smoke": dict(repeats=2, duration=4, warmup=1, probe_s=2, probe_warm=1, bisect_steps=2,
                  configs=[("s2", "unpipelined", 8, 0)], bulk=dict(repeats=2, s5_rows=20000, s6_rows=100000)),
    # quick: a first look, about 10 minutes per client
    "quick": dict(repeats=3, duration=8, warmup=2, probe_s=3, probe_warm=1, bisect_steps=3,
                  configs=[("s1", "unpipelined", 64, 0), ("s2", "unpipelined", 1, 0), ("s2", "unpipelined", 64, 0),
                           ("s3", "unpipelined", 64, 0)], bulk=dict(repeats=3, s5_rows=100000, s6_rows=1000000)),
    # repeat: what rounds 2 and 3 repeat (plan 6.2: at least 5 runs per cell are met across the rounds)
    "repeat": dict(repeats=5, duration=15, warmup=3, probe_s=3, probe_warm=1, bisect_steps=4,
                   configs=[("s1", "unpipelined", 64, 0), ("s2", "unpipelined", 1, 0), ("s2", "unpipelined", 64, 0),
                            ("s2", "unpipelined", 512, 0), ("s3", "unpipelined", 64, 0)]),
    # full: plan section 6: connection sweep for S2, S1, S3, pipelined separately, S8 pool
    "full": dict(repeats=5, duration=20, warmup=4, probe_s=4, probe_warm=1, bisect_steps=5,
                 configs=[("s1", "unpipelined", 64, 0)]
                         + [("s2", "unpipelined", c, 0) for c in (1, 8, 64, 256, 512)]
                         + [("s3", "unpipelined", 64, 0),
                            ("s2", "pipelined", 8, 0),
                            ("s2", "unpipelined", 512, 32),
                            ("s7", "unpipelined", 64, 0)],
                 bulk=dict(repeats=5, s5_rows=100000, s6_rows=10_000_000)),
}


def cmd_campaign(a):
    prof = dict(PROFILES[a.profile])
    for k in ("repeats", "duration", "warmup", "probe_s", "probe_warm", "bisect_steps"):
        setattr(a, k, prof[k])
    a.only = a.only
    log = Log(a.out)
    with open(os.path.join(a.out, "machine.json"), "w") as f:
        json.dump(dict(machine_info(), lane=a.lane, round=a.round, profile=a.profile, url_host=a.url.split("@")[-1].split("/")[0],
                       started=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())), f, indent=1)
    clients = a.clients.split(",") if a.clients else list(CLIENTS)
    summary = []
    for client in clients:
        spec = CLIENTS[client]
        for (sc, mode, conns, pool) in prof["configs"]:
            if mode not in spec["modes"]:
                continue
            if pool and not spec.get("pool"):
                continue
            if a.only and a.only not in f"{sc}-{mode}-{conns}-{pool}":
                continue
            print(f"[{client}] {sc} {mode} conns={conns} pool={pool}", flush=True)
            tags = dict(lane=a.lane, round=a.round, profile=a.profile)
            if sc == "s7" and (not a.url_tls or not spec.get("tls", True)):
                print("  skipped S7: " + ("no --url-tls" if not a.url_tls else "this adapter has no TLS support yet")); continue
            runs, rate = sweep(a, log, client, "s2" if sc == "s7" else sc, mode, conns, pool, tags=dict(tags, label=sc), tls=(sc == "s7"))
            summary.append((client, sc, mode, conns, pool, rate))
            log.add({"client": client, "scenario": sc, "mode": mode, "conns": conns, "pool": pool,
                     "slo_rate": rate, "n_final": len(runs)}, phase="result", **tags)
    if prof.get("bulk"):
        for client in clients:
            run_bulk_for(a, log, client, prof["bulk"], dict(lane=a.lane, round=a.round, profile=a.profile))
    print("campaign done:", len(summary), "configurations")


def run_bulk_for(a, log, client, bp, tags):
    spec = CLIENTS[client]
    for sc in spec.get("bulk", []):
        if a.only and a.only not in f"bulk-{sc}":
            continue
        rows = {"s4": 1_000_000, "s5": bp["s5_rows"], "s6": bp["s6_rows"]}[sc]
        print(f"[{client}] bulk {sc} rows={rows}", flush=True)
        raw = run_bulk(a, client, sc, bp["repeats"], rows)
        log.add(raw, phase="bulk", **tags)
        if raw.get("failed"):
            print("    FAILED", str(raw.get("stderr"))[:300], flush=True)
        else:
            secs = sorted(x["secs"] for x in raw["runs"])
            print(f"    median {secs[len(secs)//2]:.3f}s rows {raw['runs'][0]['rows']}", flush=True)


def cmd_bulk(a):
    log = Log(a.out)
    bp = dict(repeats=a.repeats, s5_rows=a.s5_rows, s6_rows=a.s6_rows)
    clients = a.clients.split(",") if a.clients else [c for c in CLIENTS if CLIENTS[c].get("bulk")]
    for client in clients:
        run_bulk_for(a, log, client, bp, dict(lane=a.lane, round=a.round, profile="bulk"))


def cmd_report(a):
    rows = [json.loads(l) for l in open(os.path.join(a.dir, "cells.jsonl"))]
    finals = [summarize(r) | {"lane": r.get("lane"), "round": r.get("round"), "rate_cand": r.get("slo_rate_candidate")}
              for r in rows if r.get("phase") == "final" and not r.get("failed")]
    results = [r for r in rows if r.get("phase") == "result"]
    out = []
    out.append("# Benchmark report\n")
    out.append("Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in "
               "`machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.\n")
    # one table per scenario/mode/conns/pool
    keys = sorted({(r["scenario"], r["mode"], r["conns"], r["pool"]) for r in results})
    for key in keys:
        sc, mode, conns, pool = key
        out.append(f"\n## {sc.upper()} {mode}, {conns} connections" + (f", pool {pool}" if pool else "")
                   + f" (p99 bound {SCENARIOS[sc]['bound_ms']} ms)\n")
        out.append("| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |")
        out.append("|---|---|---:|---:|---:|---:|---:|---:|---:|")
        groups = {}
        for r in results:
            if (r["scenario"], r["mode"], r["conns"], r["pool"]) == key:
                groups.setdefault((r.get("lane"), r.get("round")), []).append(r)
        for (lane, rnd), rs in sorted(groups.items(), key=lambda kv: str(kv[0])):
            for r in sorted(rs, key=lambda r: -r["slo_rate"]):
                fin = [f for f in finals if f["client"] == r["client"] and (f["scenario"], f["mode"], f["conns"], f["pool"]) == key
                       and f["lane"] == lane and f["round"] == rnd and abs(f["rate_cand"] - r["slo_rate"]) < 1e-6]
                if not fin:
                    out.append(f"| {r['client']} | {lane}/{rnd} | did not meet the bound | | | | | | 0 |")
                    continue
                med = lambda k: statistics.median(f[k] for f in fin if k in f) if any(k in f for f in fin) else float("nan")
                out.append(f"| {r['client']} | {lane}/{rnd} | {r['slo_rate']:.0f} | {med('p50_us'):.0f} | {med('p99_us'):.0f} | "
                           f"{med('p999_us'):.0f} | {med('cpu_s_per_mreq'):.2f} | {med('rss_mb'):.0f} | {len(fin)} |")
    bulks = [r for r in rows if r.get("phase") == "bulk"]
    if bulks:
        out.append("\n## Bulk scenarios (closed loop; seconds are per timed pass, medians)\n")
        out.append("S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the "
                   "headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must "
                   "fold the same S4 checksum, or the row says MISMATCH.\n")
        for sc in ("s4", "s5", "s6"):
            grp = [r for r in bulks if r.get("scenario", "").lower() == sc and not r.get("failed")]
            if not grp:
                continue
            ref = {}
            for r in grp:
                ref.setdefault((r.get("lane"), r.get("round")), r["runs"][0]["checksum"] if sc == "s4" else 0)
            out.append(f"\n### {sc.upper()}\n")
            out.append("| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |")
            out.append("|---|---|---:|---:|---:|---:|---:|---:|---|")
            for r in sorted(grp, key=lambda r: (str(r.get("lane")), str(r.get("round")), statistics.median(x["secs"] for x in r["runs"]))):
                secs = statistics.median(x["secs"] for x in r["runs"])
                cpu = statistics.median(x["cpu_s"] for x in r["runs"])
                n = r["runs"][0]["rows"]
                ok = all(x["checksum"] == r["runs"][0]["checksum"] for x in r["runs"]) and (sc != "s4" or r["runs"][0]["checksum"] == ref[(r.get("lane"), r.get("round"))])
                out.append(f"| {r['client']} ({r['version']}) | {r.get('lane')}/{r.get('round')} | {n} | {secs:.3f} | {n / secs:,.0f} | "
                           f"{cpu / n * 1e6:.2f} | {r['rss_kb'] / 1024:.0f} | {len(r['runs'])} | {'ok' if ok else 'MISMATCH'} |")
        fails = [r for r in rows if r.get("phase") == "bulk" and r.get("failed")]
        for r in fails:
            out.append(f"\n_failed: {r['client']} {r.get('scenario')}: {str(r.get('stderr'))[:200]}_")
    open(os.path.join(a.dir, "report.md"), "w").write("\n".join(out) + "\n")
    print("\n".join(out))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("setup"); p.add_argument("--psql", required=True); p.set_defaults(f=cmd_setup)
    for name, f in (("cell", cmd_cell), ("sweep", cmd_sweep)):
        p = sub.add_parser(name); common_args(p); p.set_defaults(f=f)
        p.add_argument("--client", required=True); p.add_argument("--scenario", default="s2")
        p.add_argument("--mode", default="unpipelined"); p.add_argument("--conns", type=int, default=8)
        p.add_argument("--pool", type=int, default=0); p.add_argument("--dispatch", default="shared")
        p.add_argument("--rate", type=float, default=2000); p.add_argument("--out", default="bench-out")
    p = sub.add_parser("campaign"); common_args(p); p.set_defaults(f=cmd_campaign)
    p.add_argument("--profile", choices=list(PROFILES), default="smoke"); p.add_argument("--out", required=True)
    p.add_argument("--clients", default=None); p.add_argument("--lane", default="local"); p.add_argument("--round", default="1")
    p.add_argument("--only", default=None, help="substring filter on scenario-mode-conns-pool")
    p = sub.add_parser("bulk"); common_args(p); p.set_defaults(f=cmd_bulk)
    p.add_argument("--out", required=True); p.add_argument("--clients", default=None); p.add_argument("--lane", default="local")
    p.add_argument("--round", default="1"); p.add_argument("--only", default=None)
    p.add_argument("--s5-rows", type=int, default=100000); p.add_argument("--s6-rows", type=int, default=1000000)
    p = sub.add_parser("report"); p.add_argument("dir"); p.set_defaults(f=cmd_report)
    a = ap.parse_args()
    if a.cmd in ("cell", "sweep", "campaign", "bulk") and not a.url:
        ap.error("--url or BENCH_URL is required")
    a.f(a)


if __name__ == "__main__":
    main()
