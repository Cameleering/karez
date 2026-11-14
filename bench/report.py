#!/usr/bin/env python3
"""Campaign report: merge the raw cells of every lane and round under a results directory and build
one table per configuration with all clients in it, losses included, and the ratio to karez computed
only between rows of the same lane and round (the published figure is the median of those ratios).

  report.py bench/results/2026-11-15-campaign1            writes REPORT.md next to the lanes

Reads every `cells.jsonl` below the directory (lane1/out/lane1-round1-full/cells.jsonl, ...).
Standard library only. Nothing is estimated: a number is a median of raw runs.
"""
import glob, json, os, statistics, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run as harness  # noqa: E402

REFS = ("karez-blocking", "karez-shared")


def load(root):
    rows = []
    for f in sorted(glob.glob(os.path.join(root, "**", "cells.jsonl"), recursive=True)):
        for line in open(f):
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def med(xs):
    xs = [x for x in xs if x is not None]
    return statistics.median(xs) if xs else None


def fmt(x, nd=0, none="-"):
    return none if x is None else f"{x:,.{nd}f}"


def ratio_cell(vals):
    """vals: list of per-(lane,round) ratios."""
    if not vals:
        return "-"
    lo, hi = min(vals), max(vals)
    return f"{statistics.median(vals):.2f} (n={len(vals)}, {lo:.2f}-{hi:.2f})"


def oltp(rows, out):
    finals = [r for r in rows if r.get("phase") == "final" and not r.get("failed")]
    results = [r for r in rows if r.get("phase") == "result"]
    by_cfg = {}
    for r in results:
        key = (r["scenario"], r["mode"], r["conns"], r["pool"])
        by_cfg.setdefault(key, []).append(r)
    for key in sorted(by_cfg, key=lambda k: (k[0], k[1], k[2], k[3])):
        sc, mode, conns, pool = key
        bound = harness.SCENARIOS[sc]["bound_ms"]
        out.append(f"\n### {sc.upper()} {mode}, {conns} connections" + (f", pool {pool}" if pool else "")
                   + f" (p99 bound {bound} ms)\n")
        # entry per (client, lane, round)
        entries = {}
        for r in by_cfg[key]:
            lr = (r.get("lane"), str(r.get("round")))
            fin = [harness.summarize(f) for f in finals
                   if f["client"] == r["client"] and (f.get("label") or f["scenario"]).lower() == sc and f["mode"].lower() == mode
                   and f["conns"] == conns and f["pool"] == pool and (f.get("lane"), str(f.get("round"))) == lr
                   and abs(f.get("slo_rate_candidate", -1) - r["slo_rate"]) < 1e-6]
            entries[(r["client"], lr)] = dict(rate=r["slo_rate"], fin=fin)
        clients = sorted({c for c, _ in entries}, key=lambda c: (c not in REFS, c))
        out.append("| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |")
        out.append("|---|---:|---:|---|---|---:|---:|---:|---:|---:|")
        table = []
        for c in clients:
            lrs = [lr for (cc, lr) in entries if cc == c]
            rate_med = med([entries[(c, lr)]["rate"] for lr in lrs if entries[(c, lr)]["rate"] > 0])
            ratios = {}
            for ref in REFS:
                vals = []
                for lr in lrs:
                    e, k = entries[(c, lr)], entries.get((ref, lr))
                    if k and k["rate"] > 0:
                        vals.append(e["rate"] / k["rate"])
                ratios[ref] = vals
            fin = [s for lr in lrs for s in entries[(c, lr)]["fin"]]
            table.append((rate_med or 0, c, len(lrs), rate_med, ratios, fin))
        for _, c, n, rate_med, ratios, fin in sorted(table, key=lambda t: -t[0]):
            out.append(f"| {c} | {n} | {fmt(rate_med)} | {ratio_cell(ratios['karez-blocking'])} | {ratio_cell(ratios['karez-shared'])} | "
                       f"{fmt(med([s['p50_us'] for s in fin]))} | {fmt(med([s['p99_us'] for s in fin]))} | {fmt(med([s['p999_us'] for s in fin]))} | "
                       f"{fmt(med([s.get('cpu_s_per_mreq') for s in fin]), 1)} | {fmt(med([s['rss_mb'] for s in fin]), 0)} |")
        missing = [c for c in clients if all(entries[(c, lr)]["rate"] == 0 for (cc, lr) in entries if cc == c)]
        if missing:
            out.append(f"\nDid not meet the p99 bound at any rate: {', '.join(missing)}.")


def bulk(rows, out):
    bulks = [r for r in rows if r.get("phase") == "bulk"]
    if not bulks:
        return
    out.append("\n## Bulk scenarios (closed loop)\n")
    out.append("Seconds are the median of the timed passes of one cell. The ratio compares rows/s of a client with "
               "`karez-blocking` (row view) of the same lane and round; the chunk path row is a different access path and "
               "is labelled. CPU-s per million rows is the process CPU over the pass; RSS is the peak of the whole process.\n")
    for sc in ("s4", "s5", "s6"):
        grp = [r for r in bulks if str(r.get("scenario", "")).lower() == sc]
        if not grp:
            continue
        ok = [r for r in grp if not r.get("failed")]
        entries = {}
        for r in ok:
            lr = (r.get("lane"), str(r.get("round")))
            n = r["runs"][0]["rows"]
            secs = med([x["secs"] for x in r["runs"]])
            entries[(r["client"], lr)] = dict(rps=n / secs, secs=secs, cpu=med([x["cpu_s"] for x in r["runs"]]) / n * 1e6,
                                              rss=r["rss_kb"] / 1024, n=n, sums={x["checksum"] for x in r["runs"]}, version=r["version"])
        out.append(f"\n### {sc.upper()}\n")
        out.append("| client | lanes x rounds | rows | median s | rows/s | vs karez-blocking | CPU-s per M rows | peak RSS MB | checksum |")
        out.append("|---|---:|---:|---:|---:|---|---:|---:|---|")
        clients = sorted({c for c, _ in entries}, key=lambda c: (c != "karez-blocking", c))
        sums_by_lr = {}
        for (c, lr), e in entries.items():
            sums_by_lr.setdefault(lr, set()).update(e["sums"])
        for c in clients:
            lrs = [lr for (cc, lr) in entries if cc == c]
            vals = [entries[(c, lr)]["rps"] / entries[("karez-blocking", lr)]["rps"] for lr in lrs if ("karez-blocking", lr) in entries]
            e = [entries[(c, lr)] for lr in lrs]
            good = all(len(sums_by_lr[lr]) == 1 for lr in lrs) if sc == "s4" else True
            out.append(f"| {c} | {len(lrs)} | {e[0]['n']} | {fmt(med([x['secs'] for x in e]), 3)} | {fmt(med([x['rps'] for x in e]))} | "
                       f"{ratio_cell(vals)} | {fmt(med([x['cpu'] for x in e]), 2)} | {fmt(med([x['rss'] for x in e]))} | {'ok' if good else 'MISMATCH'} |")
        for r in [r for r in grp if r.get("failed")]:
            out.append(f"\n_failed: {r['client']} {sc}: {str(r.get('stderr'))[:160]}_")


def machines(root, out):
    out.append("\n## Machines\n")
    for f in sorted(glob.glob(os.path.join(root, "**", "machine*.txt"), recursive=True))[:6]:
        out.append(f"\n`{os.path.relpath(f, root)}`\n\n```\n{open(f).read().strip()}\n```")
    for f in sorted(glob.glob(os.path.join(root, "**", "machine.json"), recursive=True))[:3]:
        out.append(f"\n`{os.path.relpath(f, root)}`: `{json.dumps(json.load(open(f)))}`")


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    rows = load(root)
    out = ["# Benchmark report", "",
           "Built by `bench/report.py` from the raw cells (`cells.jsonl`) of every lane and round. Every number is `[measured here]`, "
           "on the machines listed at the end, on the dates in the raw files. **Ratios compare clients only between rows of the same "
           "lane and round; the figure shown is their median with the count and range.** Absolute numbers belong to their droplets. "
           "S1 to S3 and S8 are open loop with the p99 bound given in each heading; the SLO rate is the highest rate searched at which the "
           "median p99 of the final runs stayed inside it. Losses are in the same tables as wins.", ""]
    n_lr = len({(r.get("lane"), str(r.get("round"))) for r in rows if r.get("phase") in ("result", "bulk")})
    out.append(f"Lanes x rounds in this report: {n_lr}.\n")
    out.append("## Open-loop scenarios")
    oltp(rows, out)
    bulk(rows, out)
    machines(root, out)
    path = os.path.join(root, "REPORT.md")
    open(path, "w").write("\n".join(out) + "\n")
    print(f"wrote {path} ({len(rows)} raw cells)")


if __name__ == "__main__":
    main()
