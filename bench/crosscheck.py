#!/usr/bin/env python3
"""Cross-check of the open-loop method against `pgbench -R` (plan 6.1).

Runs S2 with pgbench (prepared, rate-limited, per-client schedule) and with the libpq adapter in
`perconn` dispatch (the same schedule model) at the same rates and connection counts, and prints
the percentiles side by side. They should agree below saturation; if they do not, the harness is
wrong, not the database. pgbench latency = transaction time + schedule lag (its own -l log fields).

  crosscheck.py --url postgresql://... --pgbench pgbench --rates 2000,8000 --conns 8 [--scenario s1|s2]

pgbench sends text parameters and receives text results; the adapters use binary. S1 (`SELECT 1`) has one tiny column, so
it isolates the harness from that difference; S2 (ten columns) shows it. The first 2 s of the pgbench log are dropped, as
the adapter drops its warm-up.
"""
import argparse, json, os, subprocess, sys, tempfile, urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run as harness  # noqa: E402

S2 = """\\set id random(1, 100000)
SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = :id;
"""
S1 = "SELECT 1;" + chr(10)



def pgbench(a, rate, conns):
    u = urllib.parse.urlparse(a.url)
    env = dict(os.environ, PGPASSWORD=urllib.parse.unquote(u.password or ""))
    with tempfile.TemporaryDirectory() as d:
        script = os.path.join(d, "s2.sql")
        open(script, "w").write(S1 if a.scenario == "s1" else S2)
        cmd = [a.pgbench, "-n", "-M", "prepared", "-f", script, "-R", str(rate), "-c", str(conns), "-j", str(a.jobs or min(conns, 4)),
               "-T", str(a.duration), "-l", "--log-prefix", os.path.join(d, "log"),
               "-h", u.hostname, "-p", str(u.port or 5432), "-U", u.username, u.path.lstrip("/")]
        r = subprocess.run(cmd, env=env, capture_output=True, text=True, cwd=d)
        if r.returncode != 0:
            return {"error": r.stderr[-300:]}
        rows = []
        for f in os.listdir(d):
            if f.startswith("log"):
                for line in open(os.path.join(d, f)):
                    p = line.split()
                    # client_id transaction_no time script_no time_epoch time_us [schedule_lag]
                    rows.append((int(p[4]), int(p[2]) + (int(p[6]) if len(p) > 6 else 0)))
        t0 = min((r[0] for r in rows), default=0)
        lat = [l for (t, l) in rows if t >= t0 + 2]
        lat.sort()
        if not lat:
            return {"error": "no log lines"}
        q = lambda x: lat[min(len(lat) - 1, int(x * len(lat)))]
        return {"n": len(lat), "p50_us": q(.5), "p99_us": q(.99), "p999_us": q(.999)}


def ours(a, rate, conns):
    ns = argparse.Namespace(url=a.url, url_tls=None, root_cert=None, rt_threads=4)
    raw = harness.run_cell(ns, "libpq", a.scenario, "unpipelined", conns, rate, a.duration, 2, 0, "perconn")
    s = harness.summarize(raw)
    if s.get("failed"):
        return {"error": str(s.get("stderr"))[-300:]}
    return {"n": s["ok"], "p50_us": s["p50_us"], "p99_us": s["p99_us"], "p999_us": s["p999_us"]}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default=os.environ.get("BENCH_URL"))
    ap.add_argument("--pgbench", default="pgbench")
    ap.add_argument("--rates", default="2000,8000")
    ap.add_argument("--conns", type=int, default=8)
    ap.add_argument("--duration", type=int, default=20)
    ap.add_argument("--scenario", default="s2", choices=["s1", "s2"])
    ap.add_argument("--jobs", type=int, default=0, help="pgbench threads (default min(conns, 4))")
    a = ap.parse_args()
    print("| rate/s | conns | pgbench p50 us | p99 us | p99.9 us | libpq adapter p50 us | p99 us | p99.9 us |")
    print("|---:|---:|---:|---:|---:|---:|---:|---:|")
    for rate in [float(x) for x in a.rates.split(",")]:
        p, o = pgbench(a, rate, a.conns), ours(a, rate, a.conns)
        if "error" in p or "error" in o:
            print(f"| {rate:.0f} | {a.conns} | error: {p.get('error')} {o.get('error')} |")
            continue
        print(f"| {rate:.0f} | {a.conns} | {p['p50_us']} | {p['p99_us']} | {p['p999_us']} | "
              f"{o['p50_us']:.0f} | {o['p99_us']:.0f} | {o['p999_us']:.0f} |")


if __name__ == "__main__":
    main()
