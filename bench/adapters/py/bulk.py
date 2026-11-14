"""Bulk scenarios (S4 scan, S5 batch insert, S6 COPY) of the Python adapters. Closed loop: `--repeats`
timed passes after one untimed warm-up; prints seconds and CPU-seconds of each, and peak RSS.
See bench/rustbench/src/bulk.rs for the checksum of S4 and the row recipe of S5 and S6."""
import asyncio, json, struct, sys, time

S4_SQL = "SELECT id, a, b, c, d, e, s1, s2, flag, f FROM bench_wide"
COLS = "id, a, b, c, d, e, s1, s2, flag, f"
M64 = 2**64 - 1


def gen_row(i):
    return (i, i % 100000, (i * 17) % 65536, i * 10007, i / 3.0, (i % 1000) * 0.25, f"row-{i}",
            f"{(i * 0x9E3779B97F4A7C15F39CC0605CEDC835) & (2**128 - 1):032x}", i % 2 == 0, i % 1000)


def gen_text(i):
    r = gen_row(i)
    return "\t".join([str(r[0]), str(r[1]), str(r[2]), str(r[3]), repr(r[4]), repr(r[5]), r[6], r[7],
                      "t" if r[8] else "f", str(r[9])]) + "\n"


def fold(rows):
    s = 0
    for r in rows:
        s += r[0] + r[1] + r[2] + r[3] + struct.unpack("<Q", struct.pack("<d", r[4]))[0] \
            + struct.unpack("<Q", struct.pack("<d", r[5]))[0] + len(r[6]) + len(r[7]) + int(r[8]) + r[9]
    return len(rows), s & M64


def cpu_now():
    try:
        f = open("/proc/self/stat").read().rsplit(")", 1)[1].split()
        import os
        t = os.sysconf("SC_CLK_TCK")
        return (int(f[11]) + int(f[12])) / t
    except Exception:
        return 0.0


def rss_kb():
    try:
        for l in open("/proc/self/status"):
            if l.startswith("VmHWM:"):
                return int(l.split()[1])
    except Exception:
        pass
    return 0


async def timed(fn):
    c0, t0 = cpu_now(), time.perf_counter()
    rows, s = await fn()
    return dict(secs=round(time.perf_counter() - t0, 4), cpu_s=round(cpu_now() - c0, 3), rows=rows, checksum=s)


async def run_asyncpg(cfg):
    import asyncpg
    c = await asyncpg.connect(cfg["url"])
    sc = cfg["scenario"]
    n_rows = cfg["bulk_rows"]
    INS = f"INSERT INTO bench_ins ({COLS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"

    async def s4():
        return fold(await c.fetch(S4_SQL))

    async def s5(n):
        await c.execute("TRUNCATE bench_ins")
        async with c.transaction():
            await c.executemany(INS, (gen_row(i) for i in range(1, n + 1)))
        return await c.fetchval("SELECT count(*) FROM bench_ins"), 0

    async def s6(n):
        await c.execute("TRUNCATE bench_ins")
        await c.copy_records_to_table("bench_ins", records=(gen_row(i) for i in range(1, n + 1)),
                                      columns=COLS.split(", "))
        return await c.fetchval("SELECT count(*) FROM bench_ins"), 0

    one = {"s4": s4, "s5": lambda: s5(n_rows), "s6": lambda: s6(n_rows)}[sc]
    warm = {"s4": s4, "s5": lambda: s5(1000), "s6": lambda: s6(1000)}[sc]
    await warm()
    runs = [await timed(one) for _ in range(cfg["repeats"])]
    await c.close()
    import asyncpg as ap
    return f"asyncpg {ap.__version__}", runs


async def run_psycopg(cfg):
    import psycopg
    c = await psycopg.AsyncConnection.connect(cfg["url"], autocommit=True)
    sc = cfg["scenario"]
    n_rows = cfg["bulk_rows"]
    INS = f"INSERT INTO bench_ins ({COLS}) VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)"

    async def s4():
        cur = c.cursor(binary=True)
        await cur.execute(S4_SQL, prepare=True)
        return fold(await cur.fetchall())

    async def s5(n):
        await c.execute("TRUNCATE bench_ins")
        async with c.transaction():
            cur = c.cursor()
            await cur.executemany(INS, [gen_row(i) for i in range(1, n + 1)])
        cur = await c.execute("SELECT count(*) FROM bench_ins")
        return (await cur.fetchone())[0], 0

    async def s6(n):
        await c.execute("TRUNCATE bench_ins")
        cur = c.cursor()
        async with cur.copy(f"COPY bench_ins ({COLS}) FROM STDIN") as copy:
            buf = []
            for i in range(1, n + 1):
                buf.append(gen_text(i))
                if len(buf) >= 500:
                    await copy.write("".join(buf))
                    buf = []
            if buf:
                await copy.write("".join(buf))
        cur = await c.execute("SELECT count(*) FROM bench_ins")
        return (await cur.fetchone())[0], 0

    one = {"s4": s4, "s5": lambda: s5(n_rows), "s6": lambda: s6(n_rows)}[sc]
    warm = {"s4": s4, "s5": lambda: s5(1000), "s6": lambda: s6(1000)}[sc]
    await warm()
    runs = [await timed(one) for _ in range(cfg["repeats"])]
    await c.close()
    return f"psycopg {psycopg.__version__} (binary)", runs


def run(cfg):
    try:
        import uvloop
        uvloop.install()
    except Exception:
        pass
    runner = {"asyncpg": run_asyncpg, "psycopg3": run_psycopg}[cfg["client"]]
    version, runs = asyncio.run(runner(cfg))
    print(json.dumps(dict(harness="pyadapter", bulk=True, client=cfg["client"], version=version,
                          scenario=cfg["scenario"].upper(), rows_target=cfg["bulk_rows"], runs=runs, rss_kb=rss_kb())))
