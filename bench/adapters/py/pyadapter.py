#!/usr/bin/env python3
"""Python adapters (asyncpg, psycopg3 binary) of the karez benchmark. Same flags and output as
rustbench (bench/README.md). One process, one asyncio loop; arrivals come from a generator thread
that sleeps to the fixed schedule, so the loop's timer resolution does not blur the arrival times.

Adapter rules: connections open and statements prepared before the clock starts; every column of
every row is read in its native Python type and folded into the checksum; row count (and for S2 the
id) is checked; latency is measured from the intended start."""
import asyncio, json, os, struct, sys, threading, time

SQL = {
    "s1": "SELECT 1",
    "s2": "SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = {p}",
    "s3": "SELECT id, message FROM bench_fortune",
}
TAXI = "trip_id, vendor, pickup, dropoff, passengers, distance, ratecode, flag, pu, do_, payment, fare, extra, tip, tolls, total, congestion"
SQL["r1"] = f"SELECT {TAXI} FROM taxi_trips WHERE trip_id = {{p}}"
SQL["r2"] = f"SELECT {TAXI} FROM taxi_trips WHERE trip_id BETWEEN {{p}} AND {{q}} + 99 ORDER BY trip_id"
SQL["r3"] = "SELECT pu, count(*), max(fare), max(total) FROM taxi_trips WHERE pu = {p} GROUP BY pu"
ROWS = {"s1": 1, "s2": 1, "s3": 12, "r1": 1, "r2": 100, "r3": 1}
HAS_PARAM = {"s2", "r1", "r2", "r3"}


def kinds(sc):
    return ["r1", "r2", "r3"] if sc == "r4" else [sc]


def request(sc, rng, cfg):
    """One request: the concrete query and its parameter."""
    if sc == "r4":
        v = rng.next(100)
        sc = "r1" if v <= 80 else ("r2" if v <= 95 else "r3")
    if sc == "s2":
        return sc, rng.next(cfg["rows"])
    if sc == "r1":
        return sc, rng.next(cfg["taxi_rows"])
    if sc == "r2":
        return sc, rng.next(max(1, cfg["taxi_rows"] - 99))
    if sc == "r3":
        return sc, rng.next(260)
    return sc, 0


def sql_asyncpg(sc):
    return SQL[sc].replace("{p}", "$1").replace("{q}", "$1")


def sql_psycopg(sc):
    return SQL[sc].replace("{p}", "%s").replace("{q}", "%s")


def args_asyncpg(sc, want):
    return (want,) if sc in HAS_PARAM else ()


def args_psycopg(sc, want):
    from psycopg.types.numeric import Int4, Int8
    if sc == "r2":
        return (Int8(want), Int8(want))
    if sc == "r3":
        return (Int4(want),)
    return (Int8(want),) if sc in HAS_PARAM else None


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
        bulk_rows=int(float(m.get("bulk-rows", 100000))), taxi_rows=int(float(m.get("taxi-rows", 1000000))),
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
    return (r[0] + r[1] + r[2] + int(r[3]) + bits(r[4]) + len(r[5].encode()) + len(r[6].encode()) + r[7] + len(r[8].encode()) + r[9])


def us(dt):
    import calendar
    return calendar.timegm(dt.timetuple()) * 1_000_000 + dt.microsecond


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def fold_taxi(r):
    return (r[0] + r[1] + us(r[2]) + us(r[3]) + (r[4] or 0) + bits(r[5]) + (r[6] or 0) + (len(r[7]) if r[7] is not None else 0)
            + r[8] + r[9] + r[10] + bits(r[11]) + bits(r[12]) + bits(r[13]) + bits(r[14]) + bits(r[15])
            + (bits(r[16]) if r[16] is not None else 0))


def fold(sc, rows, want):
    if sc == "r3":
        if len(rows) > 1:
            raise RuntimeError(f"{len(rows)} rows, wanted at most 1")
    elif len(rows) != ROWS[sc]:
        raise RuntimeError(f"{len(rows)} rows, wanted {ROWS[sc]}")
    s = 0
    for i, r in enumerate(rows):
        if sc in ("r1", "r2"):
            w = want if sc == "r1" else want + i
            if r[0] != w:
                raise RuntimeError(f"{sc} returned id {r[0]}, wanted {w}")
            s += fold_taxi(r)
            continue
        if sc == "r3":
            if r[0] != want:
                raise RuntimeError(f"R3 returned zone {r[0]}, wanted {want}")
            s += r[0] + r[1] + bits(r[2]) + bits(r[3])
            continue
        if sc == "s1":
            s += int(r[0])
        elif sc == "s2":
            s += fold_s2(r, want)
        else:
            s += r[0] + len(r[1].encode())
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
            qsc, want = request(sc, rng, cfg)
            try:
                rows = await one(conn, qsc, want)
                summ = fold(qsc, rows, want)
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
    url = cfg["url"]

    async def warm(c):
        for k in kinds(sc):
            await c.fetch(sql_asyncpg(k), *args_asyncpg(k, 1))

    if cfg["pool"] > 0:
        pool = await asyncpg.create_pool(url, min_size=cfg["pool"], max_size=cfg["pool"])
        # open and prepare every pool connection before the clock starts
        held = [await pool.acquire() for _ in range(cfg["pool"])]
        for c in held:
            await warm(c)
        for c in held:
            await pool.release(c)

        async def one(_c, qsc, want):
            async with pool.acquire() as c:
                return await c.fetch(sql_asyncpg(qsc), *args_asyncpg(qsc, want))
        await run_workers(sh, [None] * cfg["conns"], one)
        await pool.close()
    else:
        conns = [await asyncpg.connect(url) for _ in range(cfg["conns"])]
        for c in conns:
            await warm(c)

        async def one(c, qsc, want):
            return await c.fetch(sql_asyncpg(qsc), *args_asyncpg(qsc, want))
        await run_workers(sh, conns, one)
        for c in conns:
            await c.close()
    import asyncpg as ap
    return f"asyncpg {ap.__version__}"


async def run_psycopg(sh):
    import psycopg
    cfg = sh.cfg
    sc = cfg["scenario"]

    async def exec_on(c, qsc, want):
        cur = c.cursor(binary=True)
        await cur.execute(sql_psycopg(qsc), args_psycopg(qsc, want), prepare=True)
        return await cur.fetchall()

    async def warm(c):
        for k in kinds(sc):
            await exec_on(c, k, 1)

    if cfg["pool"] > 0:
        from psycopg_pool import AsyncConnectionPool
        pool = AsyncConnectionPool(cfg["url"], min_size=cfg["pool"], max_size=cfg["pool"], open=False,
                                   kwargs={"autocommit": True})
        await pool.open(wait=True)
        held = [await pool.getconn() for _ in range(cfg["pool"])]
        for c in held:
            await warm(c)
        for c in held:
            await pool.putconn(c)

        async def one(_c, qsc, want):
            async with pool.connection() as c:
                return await exec_on(c, qsc, want)
        await run_workers(sh, [None] * cfg["conns"], one)
        await pool.close()
    else:
        conns = [await psycopg.AsyncConnection.connect(cfg["url"], autocommit=True) for _ in range(cfg["conns"])]
        for c in conns:
            await warm(c)
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
