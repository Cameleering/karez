#!/usr/bin/env python3
"""Python adapters (asyncpg, psycopg3 binary) of the karez benchmark. Same flags and output as
rustbench (bench/README.md). One process, one asyncio loop; arrivals come from a generator thread
that sleeps to the fixed schedule, so the loop's timer resolution does not blur the arrival times.

Adapter rules: connections open and statements prepared before the clock starts; every column of
every row is read in its native Python type and folded into the checksum; row count (and for S2 the
id) is checked; latency is measured from the intended start."""
import asyncio, json, os, sys, threading, time

SQL = {
    "s1": "SELECT 1",
    "s2": "SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = {p}",
    "s3": "SELECT id, message FROM bench_fortune",
}
ROWS = {"s1": 1, "s2": 1, "s3": 12}


def hidx(v):
    if v < 128:
        return v
    shift = v.bit_length() - 1 - 6
    return (shift << 6) + (v >> shift)


def die(msg):
    sys.stderr.write(f"pyadapter: {msg}\n")
    sys.exit(2)


def parse():
    m, a = {}, sys.argv[1:]
    for i in range(0, len(a), 2):
        m[a[i].lstrip("-")] = a[i + 1]
    c = dict(
        client=m["client"], url=m["url"], scenario=m.get("scenario", "s1"), mode=m.get("mode", "unpipelined"),
        conns=int(float(m.get("conns", 1))), pool=int(float(m.get("pool", 0))), rate=float(m.get("rate", 1000)),
        duration=float(m.get("duration", 10)), warmup=float(m.get("warmup", 2)), rows=int(float(m.get("rows", 100000))),
        dispatch=m.get("dispatch", "shared"), repeats=int(float(m.get("repeats", 5))),
        bulk_rows=int(float(m.get("bulk-rows", 100000))),
    )
    return c


class Rng:
    def __init__(self, seed):
        self.x = (seed * 0x9E3779B97F4A7C15 | 1) & (2**64 - 1)

    def next(self, n):
        x = self.x
        x ^= x >> 12
        x ^= (x << 25) & (2**64 - 1)
        x ^= x >> 27
        self.x = x
        return (((x * 0x2545F4914F6CDD1D) & (2**64 - 1)) >> 11) % n + 1


class Shared:
    def __init__(self, cfg):
        self.cfg = cfg
        self.warm_ns = int(cfg["warmup"] * 1e9)
        self.end_ns = int((cfg["warmup"] + cfg["duration"]) * 1e9)
        self.hist, self.svc = {}, {}
        self.ok = self.errors = self.dropped = self.sent = self.checksum = 0
        self.first_error = None
        self.t0 = None

    def start(self):
        self.t0 = time.perf_counter_ns()
        sys.stderr.write("T0\n")
        sys.stderr.flush()

    def now(self):
        return time.perf_counter_ns() - self.t0

    def record(self, intended, start, end, err, summ=0):
        if intended < self.warm_ns or intended >= self.end_ns:
            return
        if err is None:
            i = hidx(max(0, end - intended))
            self.hist[i] = self.hist.get(i, 0) + 1
            j = hidx(max(0, end - start))
            self.svc[j] = self.svc.get(j, 0) + 1
            self.ok += 1
            self.checksum = (self.checksum + summ) & (2**64 - 1)
        else:
            self.errors += 1
            if self.first_error is None:
                self.first_error = str(err)

    def drop(self, intended):
        if self.warm_ns <= intended < self.end_ns:
            self.dropped += 1


def generator(sh, loop, queues):
    period = 1e9 / sh.cfg["rate"]
    k = 0
    nq = len(queues)
    while True:
        batch = [[] for _ in range(nq)]
        now = sh.now()
        n = 0
        while True:
            intended = int(k * period)
            if intended >= sh.end_ns:
                break
            if intended > now and n > 0:
                break
            if intended > now:
                time.sleep((intended - now) / 1e9)
                now = sh.now()
            if intended >= sh.warm_ns:
                sh.sent += 1
            batch[k % nq].append(intended)
            k += 1
            n += 1
            if n >= 256:
                break
        for q, items in zip(queues, batch):
            if items:
                loop.call_soon_threadsafe(lambda q=q, items=items: [q.put_nowait(x) for x in items])
        if int(k * period) >= sh.end_ns:
            break
    for q in queues:
        loop.call_soon_threadsafe(lambda q=q: [q.put_nowait(None) for _ in range(sh.cfg["conns"])])


def proc_stats():
    user = sys_ = 0.0
    rss = 0
    try:
        f = open("/proc/self/stat").read().rsplit(")", 1)[1].split()
        tick = os.sysconf("SC_CLK_TCK")
        user, sys_ = int(f[11]) / tick, int(f[12]) / tick
        for l in open("/proc/self/status"):
            if l.startswith("VmHWM:"):
                rss = int(l.split()[1])
    except Exception:
        pass
    return user, sys_, rss


def fold_s2(r, want):
    if r[0] != want:
        raise RuntimeError(f"S2 returned id {r[0]}, wanted {want}")
    return (r[0] + r[1] + r[2] + int(r[3]) + (hash(r[4]) & 0xFFFFFFFF) + len(r[5]) + len(r[6]) + r[7] + len(r[8]) + r[9])


def fold(sc, rows, want):
    if len(rows) != ROWS[sc]:
        raise RuntimeError(f"{len(rows)} rows, wanted {ROWS[sc]}")
    s = 0
    for r in rows:
        if sc == "s1":
            s += int(r[0])
        elif sc == "s2":
            s += fold_s2(r, want)
        else:
            s += r[0] + len(r[1])
    return s & (2**64 - 1)


async def run_workers(sh, workers, one):
    """workers: list of (conn_or_none). one(conn, intended_id) -> awaitable returning (rows). """
    cfg = sh.cfg
    sc = cfg["scenario"]
    loop = asyncio.get_running_loop()
    nq = len(workers) if cfg["dispatch"] == "perconn" else 1
    queues = [asyncio.Queue() for _ in range(nq)]
    closed = cfg["rate"] == 0

    async def worker(w, conn):
        rng = Rng(w + 1)
        q = queues[w % nq]
        while True:
            if closed:
                intended = sh.now()
                if intended >= sh.end_ns:
                    break
                await asyncio.sleep(0)
            else:
                intended = await q.get()
                if intended is None:
                    break
            start = sh.now()
            if start > sh.end_ns + 5_000_000_000:
                sh.drop(intended)
                continue
            want = rng.next(cfg["rows"]) if sc == "s2" else 0
            try:
                rows = await one(conn, want)
                summ = fold(sc, rows, want)
                sh.record(intended, start, sh.now(), None, summ)
            except Exception as e:  # noqa
                sh.record(intended, start, sh.now(), e)

    sh.start()
    if not closed:
        th = threading.Thread(target=generator, args=(sh, loop, queues), daemon=True)
        th.start()
    await asyncio.gather(*(worker(w, c) for w, c in enumerate(workers)))


async def run_asyncpg(sh):
    import asyncpg
    cfg = sh.cfg
    sc = cfg["scenario"]
    sql = SQL[sc].format(p="$1")
    url = cfg["url"]
    if cfg["pool"] > 0:
        pool = await asyncpg.create_pool(url, min_size=cfg["pool"], max_size=cfg["pool"])
        # open and prepare every pool connection before the clock starts
        held = [await pool.acquire() for _ in range(cfg["pool"])]
        for c in held:
            await c.fetch(sql, 1) if sc == "s2" else await c.fetch(sql)
        for c in held:
            await pool.release(c)

        async def one(_c, want):
            async with pool.acquire() as c:
                return await (c.fetch(sql, want) if sc == "s2" else c.fetch(sql))
        await run_workers(sh, [None] * cfg["conns"], one)
        await pool.close()
    else:
        conns = [await asyncpg.connect(url) for _ in range(cfg["conns"])]
        for c in conns:
            await c.fetch(sql, 1) if sc == "s2" else await c.fetch(sql)

        async def one(c, want):
            return await (c.fetch(sql, want) if sc == "s2" else c.fetch(sql))
        await run_workers(sh, conns, one)
        for c in conns:
            await c.close()
    import asyncpg as ap
    return f"asyncpg {ap.__version__}"


async def run_psycopg(sh):
    import psycopg
    from psycopg.types.numeric import Int8
    cfg = sh.cfg
    sc = cfg["scenario"]
    sql = SQL[sc].format(p="%s")

    async def exec_on(c, want):
        cur = c.cursor(binary=True)
        await cur.execute(sql, (Int8(want),) if sc == "s2" else None, prepare=True)
        return await cur.fetchall()

    if cfg["pool"] > 0:
        from psycopg_pool import AsyncConnectionPool
        pool = AsyncConnectionPool(cfg["url"], min_size=cfg["pool"], max_size=cfg["pool"], open=False,
                                   kwargs={"autocommit": True})
        await pool.open(wait=True)
        held = [await pool.getconn() for _ in range(cfg["pool"])]
        for c in held:
            await exec_on(c, 1)
        for c in held:
            await pool.putconn(c)

        async def one(_c, want):
            async with pool.connection() as c:
                return await exec_on(c, want)
        await run_workers(sh, [None] * cfg["conns"], one)
        await pool.close()
    else:
        conns = [await psycopg.AsyncConnection.connect(cfg["url"], autocommit=True) for _ in range(cfg["conns"])]
        for c in conns:
            await exec_on(c, 1)
        await run_workers(sh, conns, exec_on)
        for c in conns:
            await c.close()
    return f"psycopg {psycopg.__version__} (binary)"


def main():
    cfg = parse()
    if cfg["mode"] == "pipelined":
        die(f"{cfg['client']}: unpipelined only")
    if cfg["scenario"] in ("s4", "s5", "s6"):
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        import bulk
        bulk.run(cfg)
        return
    sh = Shared(cfg)
    runner = {"asyncpg": run_asyncpg, "psycopg3": run_psycopg}.get(cfg["client"])
    if not runner:
        die(f"unknown client {cfg['client']} (asyncpg, psycopg3)")
    t = time.time()
    loop = "asyncio"
    try:
        import uvloop
        uvloop.install()
        loop = "uvloop"
    except Exception:
        pass
    version = asyncio.run(runner(sh)) + f" on {loop}"
    user, sys_, rss = proc_stats()
    print(json.dumps(dict(
        harness="pyadapter", client=cfg["client"], version=version, scenario=cfg["scenario"].upper(),
        mode="Unpipelined", conns=cfg["conns"], pool=cfg["pool"], rate=cfg["rate"], duration_s=cfg["duration"],
        warmup_s=cfg["warmup"], dispatch=cfg["dispatch"], rt_threads=1, sent=sh.sent, ok=sh.ok, errors=sh.errors,
        dropped=sh.dropped, checksum=sh.checksum, first_error=sh.first_error, wall_s=round(time.time() - t, 3),
        cpu_user_s=user, cpu_sys_s=sys_, rss_kb=rss,
        hist=sorted(sh.hist.items()), service_hist=sorted(sh.svc.items()))))


if __name__ == "__main__":
    main()
