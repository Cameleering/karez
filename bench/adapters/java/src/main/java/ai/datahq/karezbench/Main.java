package ai.datahq.karezbench;

import com.zaxxer.hikari.HikariConfig;
import com.zaxxer.hikari.HikariDataSource;
import io.r2dbc.pool.ConnectionPool;
import io.r2dbc.pool.ConnectionPoolConfiguration;
import io.r2dbc.postgresql.PostgresqlConnectionConfiguration;
import io.r2dbc.postgresql.PostgresqlConnectionFactory;
import io.r2dbc.spi.Connection;
import io.vertx.core.Vertx;
import io.vertx.pgclient.PgBuilder;
import io.vertx.pgclient.PgConnectOptions;
import io.vertx.sqlclient.Pool;
import io.vertx.sqlclient.PoolOptions;
import io.vertx.sqlclient.Row;
import io.vertx.sqlclient.RowSet;
import io.vertx.sqlclient.Tuple;
import reactor.core.publisher.Flux;
import reactor.core.publisher.Mono;

import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.atomic.AtomicLongArray;
import java.util.concurrent.atomic.AtomicReference;

/**
 * JVM adapters of the karez benchmark: pgJDBC with HikariCP (defaults and tuned), vertx-pg-client and
 * r2dbc-postgresql. Same flags and output as rustbench (bench/README.md).
 *
 * Adapter rules: connections open and statements prepared before the clock starts; every column of
 * every row is read in its native Java type and folded into the checksum; the row count (and for S2
 * the id) is checked; latency is measured from the intended start.
 */
public final class Main {
    static final Map<String, String> SQL = Map.of(
            "s1", "SELECT 1",
            "s2", "SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = $1",
            "s3", "SELECT id, message FROM bench_fortune");
    static final Map<String, Integer> ROWS = Map.of("s1", 1, "s2", 1, "s3", 12);
    static final int BUCKETS = 64 * 60;

    static Map<String, String> arg = new HashMap<>();
    static String client, scenario, sql, url;
    static int conns, pool;
    static double rate;
    static long warmNs, endNs, rowsN;
    static boolean perConn;
    static long t0;
    static final AtomicLongArray hist = new AtomicLongArray(BUCKETS), svc = new AtomicLongArray(BUCKETS);
    static final AtomicLong ok = new AtomicLong(), errors = new AtomicLong(), dropped = new AtomicLong(),
            sent = new AtomicLong(), checksum = new AtomicLong();
    static final AtomicReference<String> firstError = new AtomicReference<>();

    static long now() { return System.nanoTime() - t0; }

    static int hidx(long v) {
        if (v < 128) return (int) v;
        int msb = 63 - Long.numberOfLeadingZeros(v);
        int shift = msb - 6;
        return Math.min(BUCKETS - 1, (shift << 6) + (int) (v >>> shift));
    }

    static void record(long intended, long start, long end, Throwable err, long sum) {
        if (intended < warmNs || intended >= endNs) return;
        if (err != null) {
            errors.incrementAndGet();
            firstError.compareAndSet(null, String.valueOf(err));
            return;
        }
        hist.incrementAndGet(hidx(Math.max(0, end - intended)));
        svc.incrementAndGet(hidx(Math.max(0, end - start)));
        ok.incrementAndGet();
        checksum.addAndGet(sum);
    }

    static final class Rng {
        long x;
        Rng(long seed) { x = (seed * 0x9E3779B97F4A7C15L) | 1; }
        long next(long n) {
            x ^= x >>> 12; x ^= x << 25; x ^= x >>> 27;
            return Long.remainderUnsigned((x * 0x2545F4914F6CDD1DL) >>> 11, n) + 1;
        }
    }

    interface Submit { void run(long id, long intended, long start) throws Exception; }

    static void die(String m) { System.err.println("javaadapter: " + m); System.exit(2); }

    public static void main(String[] a) throws Exception {
        for (int i = 0; i + 1 < a.length; i += 2) arg.put(a[i].replaceFirst("^--", ""), a[i + 1]);
        client = arg.get("client");
        url = arg.get("url");
        scenario = arg.getOrDefault("scenario", "s1");
        String mode = arg.getOrDefault("mode", "unpipelined");
        conns = (int) Double.parseDouble(arg.getOrDefault("conns", "1"));
        pool = (int) Double.parseDouble(arg.getOrDefault("pool", "0"));
        rate = Double.parseDouble(arg.getOrDefault("rate", "1000"));
        double duration = Double.parseDouble(arg.getOrDefault("duration", "10"));
        double warmup = Double.parseDouble(arg.getOrDefault("warmup", "2"));
        rowsN = (long) Double.parseDouble(arg.getOrDefault("rows", "100000"));
        perConn = "perconn".equals(arg.get("dispatch"));
        warmNs = (long) (warmup * 1e9);
        endNs = (long) ((warmup + duration) * 1e9);
        sql = SQL.get(scenario);
        boolean pipelined = mode.equals("pipelined");
        if (scenario.equals("s4") || scenario.equals("s5") || scenario.equals("s6")) {
            System.out.println(Bulk.run(client, url, scenario, (int) Double.parseDouble(arg.getOrDefault("repeats", "5")),
                    (long) Double.parseDouble(arg.getOrDefault("bulk-rows", "100000"))));
            System.exit(0);
        }

        String version;
        switch (client) {
            case "pgjdbc", "pgjdbc-tuned" -> {
                if (pipelined) die(client + ": unpipelined only");
                version = runJdbc(client.endsWith("tuned"));
            }
            case "vertx-pg-client" -> version = runVertx(pipelined);
            case "r2dbc-postgresql" -> {
                if (pipelined) die("r2dbc-postgresql: run unpipelined");
                version = runR2dbc();
            }
            default -> { die("unknown client " + client); return; }
        }
        double wall = (now()) / 1e9;
        double[] cpu = procStats();
        StringBuilder sb = new StringBuilder("{");
        sb.append("\"harness\":\"javaadapter\",\"client\":\"").append(client).append("\",\"version\":\"").append(version)
          .append("\",\"scenario\":\"").append(scenario.toUpperCase()).append("\",\"mode\":\"").append(pipelined ? "Pipelined" : "Unpipelined")
          .append("\",\"conns\":").append(conns).append(",\"pool\":").append(pool).append(",\"rate\":").append(rate)
          .append(",\"duration_s\":").append(duration).append(",\"warmup_s\":").append(warmup)
          .append(",\"dispatch\":\"").append(perConn ? "perconn" : "shared").append("\",\"rt_threads\":")
          .append(Runtime.getRuntime().availableProcessors())
          .append(",\"sent\":").append(sent.get()).append(",\"ok\":").append(ok.get()).append(",\"errors\":").append(errors.get())
          .append(",\"dropped\":").append(dropped.get()).append(",\"checksum\":").append(Long.toUnsignedString(checksum.get()))
          .append(",\"first_error\":").append(firstError.get() == null ? "null" : "\"" + firstError.get().replace("\\", "\\\\").replace("\"", "'").replace("\n", " ") + "\"")
          .append(",\"wall_s\":").append(wall).append(",\"cpu_user_s\":").append(cpu[0]).append(",\"cpu_sys_s\":").append(cpu[1])
          .append(",\"rss_kb\":").append((long) cpu[2]).append(",\"hist\":").append(pairs(hist)).append(",\"service_hist\":").append(pairs(svc)).append("}");
        System.out.println(sb);
        System.exit(0);
    }

    static String pairs(AtomicLongArray h) {
        StringBuilder s = new StringBuilder("[");
        boolean first = true;
        for (int i = 0; i < h.length(); i++) {
            long c = h.get(i);
            if (c > 0) { if (!first) s.append(','); first = false; s.append('[').append(i).append(',').append(c).append(']'); }
        }
        return s.append(']').toString();
    }

    static double[] procStats() {
        double user = 0, sys = 0, rss = 0;
        try {
            String s = Files.readString(Path.of("/proc/self/stat"));
            String[] f = s.substring(s.lastIndexOf(')') + 1).trim().split("\\s+");
            user = Double.parseDouble(f[11]) / 100; sys = Double.parseDouble(f[12]) / 100;
            for (String l : Files.readAllLines(Path.of("/proc/self/status")))
                if (l.startsWith("VmHWM:")) rss = Double.parseDouble(l.split("\\s+")[1]);
        } catch (Exception ignored) { }
        return new double[]{user, sys, rss};
    }

    /** Arrival dispatch. Workers call take(w); the generator thread feeds the queues. */
    static final class Arrivals {
        final List<LinkedBlockingQueue<Long>> qs = new ArrayList<>();
        Arrivals(int workers) {
            int nq = perConn ? workers : 1;
            for (int i = 0; i < nq; i++) qs.add(new LinkedBlockingQueue<>());
        }
        /** -1 when the window is over. */
        long take(int w) throws InterruptedException {
            if (rate == 0) { long n = now(); return n >= endNs ? -1 : n; }
            return qs.get(w % qs.size()).take();
        }
        void generate(int workers) {
            if (rate == 0) { sleepUntil(endNs); return; }
            double period = 1e9 / rate;
            for (long k = 0; ; k++) {
                long intended = (long) (k * period);
                if (intended >= endNs) break;
                long n = now();
                if (intended > n) sleepNs(intended - n);
                if (intended >= warmNs) sent.incrementAndGet();
                qs.get((int) (k % qs.size())).add(intended);
            }
            for (LinkedBlockingQueue<Long> q : qs) for (int i = 0; i < workers; i++) q.add(-1L);
        }
    }

    static void sleepNs(long ns) { java.util.concurrent.locks.LockSupport.parkNanos(ns); }
    static void sleepUntil(long ns) { long n; while ((n = now()) < ns) sleepNs(ns - n); }

    // ---- pgJDBC + HikariCP ---------------------------------------------------------------------------

    static String jdbcUrl(boolean tuned) {
        URI u = URI.create(url.replaceFirst("^postgres(ql)?://", "http://"));
        String base = "jdbc:postgresql://" + u.getHost() + ":" + (u.getPort() < 0 ? 5432 : u.getPort()) + u.getPath();
        String q = u.getQuery() == null ? "" : u.getQuery();
        String extra = tuned ? "&prepareThreshold=1&preparedStatementCacheQueries=256&preparedStatementCacheSizeMiB=5&tcpNoDelay=true" : "";
        return base + "?" + q + extra;
    }

    static long foldJdbc(ResultSet rs, long want) throws Exception {
        int n = 0; long sum = 0;
        while (rs.next()) {
            n++;
            switch (scenario) {
                case "s1" -> sum += rs.getInt(1);
                case "s2" -> {
                    long id = rs.getLong(1);
                    if (id != want) throw new IllegalStateException("S2 returned id " + id + ", wanted " + want);
                    sum += id + rs.getInt(2) + rs.getShort(3) + (rs.getBoolean(4) ? 1 : 0) + (long) rs.getDouble(5)
                            + rs.getString(6).length() + rs.getString(7).length() + rs.getLong(8) + rs.getString(9).length() + rs.getLong(10);
                }
                default -> sum += rs.getInt(1) + rs.getString(2).length();
            }
        }
        if (n != ROWS.get(scenario)) throw new IllegalStateException(n + " rows, wanted " + ROWS.get(scenario));
        return sum;
    }

    static String runJdbc(boolean tuned) throws Exception {
        URI u = URI.create(url.replaceFirst("^postgres(ql)?://", "http://"));
        String[] ui = u.getUserInfo().split(":", 2);
        HikariConfig hc = new HikariConfig();
        hc.setJdbcUrl(jdbcUrl(tuned));
        hc.setUsername(ui[0]);
        hc.setPassword(ui[1]);
        int size = pool > 0 ? pool : conns;
        hc.setMaximumPoolSize(size);
        hc.setMinimumIdle(size);
        hc.setConnectionTimeout(60_000);
        HikariDataSource ds = new HikariDataSource(hc);
        String jsql = sql.replace("$1", "?");
        // warm every connection (and its server-side statement) before the clock starts
        List<java.sql.Connection> held = new ArrayList<>();
        for (int i = 0; i < size; i++) {
            java.sql.Connection c = ds.getConnection();
            for (int k = 0; k < (tuned ? 2 : 8); k++) try (PreparedStatement ps = c.prepareStatement(jsql)) {
                if (scenario.equals("s2")) ps.setLong(1, 1);
                try (ResultSet rs = ps.executeQuery()) { foldJdbc(rs, 1); }
            }
            held.add(c);
        }
        for (java.sql.Connection c : held) c.close();
        Arrivals arr = new Arrivals(conns);
        t0 = System.nanoTime();
        System.err.println("T0");
        List<Thread> ws = new ArrayList<>();
        for (int w = 0; w < conns; w++) {
            final int wi = w;
            Thread t = new Thread(() -> {
                Rng rng = new Rng(wi + 1);
                try {
                    long intended;
                    while ((intended = arr.take(wi)) >= 0) {
                        long start = now();
                        if (start > endNs + 5_000_000_000L) { if (intended >= warmNs && intended < endNs) dropped.incrementAndGet(); continue; }
                        long id = scenario.equals("s2") ? rng.next(rowsN) : 0;
                        try (java.sql.Connection c = ds.getConnection(); PreparedStatement ps = c.prepareStatement(jsql)) {
                            if (scenario.equals("s2")) ps.setLong(1, id);
                            try (ResultSet rs = ps.executeQuery()) {
                                long sum = foldJdbc(rs, id);
                                record(intended, start, now(), null, sum);
                            }
                        } catch (Throwable e) { record(intended, start, now(), e, 0); }
                    }
                } catch (InterruptedException ignored) { }
            });
            t.start();
            ws.add(t);
        }
        arr.generate(conns);
        for (Thread t : ws) t.join();
        ds.close();
        return "pgjdbc " + org.postgresql.util.DriverInfo.DRIVER_VERSION + " + HikariCP" + (tuned ? " (tuned)" : " (defaults)");
    }

    // ---- vertx-pg-client ----------------------------------------------------------------------------

    static String query(String key) {
        String q = URI.create(url.replaceFirst("^postgres(ql)?://", "http://")).getQuery();
        if (q == null) return null;
        for (String kv : q.split("&")) if (kv.startsWith(key + "=")) return kv.substring(key.length() + 1);
        return null;
    }

    static String runVertx(boolean pipelined) throws Exception {
        Vertx vertx = Vertx.vertx();
        PgConnectOptions co = PgConnectOptions.fromUri(url.replaceFirst("\\?.*$", ""));
        if ("verify-full".equals(query("sslmode"))) {
            co.setSslMode(io.vertx.pgclient.SslMode.VERIFY_FULL).setHostnameVerificationAlgorithm("HTTPS")
              .setPemTrustOptions(new io.vertx.core.net.PemTrustOptions().addCertPath(query("sslrootcert")));
        }
        co.setCachePreparedStatements(true);
        co.setPipeliningLimit(pipelined ? 256 : 1);
        int size = pool > 0 ? pool : conns;
        Pool p = PgBuilder.pool().with(new PoolOptions().setMaxSize(size).setMaxWaitQueueSize(-1)).connectingTo(co).using(vertx).build();
        // warm: open the pool's connections and prepare
        CountDownLatch warm = new CountDownLatch(size * 4);
        for (int i = 0; i < size * 4; i++) {
            (scenario.equals("s2") ? p.preparedQuery(sql).execute(Tuple.of(1L)) : p.preparedQuery(sql).execute())
                    .onComplete(r -> warm.countDown());
        }
        warm.await();
        Arrivals arr = new Arrivals(1);
        t0 = System.nanoTime();
        System.err.println("T0");
        AtomicLong inflight = new AtomicLong();
        Thread gen = new Thread(() -> arr.generate(1));
        gen.start();
        Rng rng = new Rng(1);
        long intended;
        while ((intended = arr.take(0)) >= 0) {
            long start = now();
            long id = scenario.equals("s2") ? rng.next(rowsN) : 0;
            final long in = intended;
            inflight.incrementAndGet();
            (scenario.equals("s2") ? p.preparedQuery(sql).execute(Tuple.of(id)) : p.preparedQuery(sql).execute())
                    .onComplete(r -> {
                        try {
                            if (r.failed()) record(in, start, now(), r.cause(), 0);
                            else record(in, start, now(), null, foldVertx(r.result(), id));
                        } catch (Throwable e) { record(in, start, now(), e, 0); }
                        inflight.decrementAndGet();
                    });
        }
        gen.join();
        long deadline = System.nanoTime() + 6_000_000_000L;
        while (inflight.get() > 0 && System.nanoTime() < deadline) Thread.sleep(5);
        vertx.close().toCompletionStage().toCompletableFuture().get(10, TimeUnit.SECONDS);
        return "vertx-pg-client " + io.vertx.core.impl.VertxImpl.class.getPackage().getImplementationVersion();
    }

    static long foldVertx(RowSet<Row> rs, long want) {
        int n = 0; long sum = 0;
        for (Row r : rs) {
            n++;
            switch (scenario) {
                case "s1" -> sum += r.getInteger(0);
                case "s2" -> {
                    long id = r.getLong(0);
                    if (id != want) throw new IllegalStateException("S2 returned id " + id + ", wanted " + want);
                    sum += id + r.getInteger(1) + r.getShort(2) + (r.getBoolean(3) ? 1 : 0) + (long) (double) r.getDouble(4)
                            + r.getString(5).length() + r.getString(6).length() + r.getLong(7) + r.getString(8).length() + r.getLong(9);
                }
                default -> sum += r.getInteger(0) + r.getString(1).length();
            }
        }
        if (n != ROWS.get(scenario)) throw new IllegalStateException(n + " rows, wanted " + ROWS.get(scenario));
        return sum;
    }

    // ---- r2dbc-postgresql -----------------------------------------------------------------------------

    static String runR2dbc() throws Exception {
        URI u = URI.create(url.replaceFirst("^postgres(ql)?://", "http://"));
        String[] ui = u.getUserInfo().split(":", 2);
        PostgresqlConnectionConfiguration.Builder cc = PostgresqlConnectionConfiguration.builder()
                .host(u.getHost()).port(u.getPort() < 0 ? 5432 : u.getPort()).database(u.getPath().substring(1))
                .username(ui[0]).password(ui[1]).preparedStatementCacheQueries(256);
        if ("verify-full".equals(query("sslmode"))) cc = cc.sslMode(io.r2dbc.postgresql.client.SSLMode.VERIFY_FULL).sslRootCert(query("sslrootcert"));
        PostgresqlConnectionConfiguration ccf = cc.build();
        int size = pool > 0 ? pool : conns;
        ConnectionPool cp = new ConnectionPool(ConnectionPoolConfiguration.builder(new PostgresqlConnectionFactory(ccf))
                .initialSize(size).maxSize(size).acquireRetry(0).build());
        // warm every connection and its statement
        List<Connection> held = new ArrayList<>();
        for (int i = 0; i < size; i++) held.add(cp.create().block());
        for (Connection c : held) { for (int k = 0; k < 3; k++) queryR2(c, 1).block(); }
        for (Connection c : held) Mono.from(c.close()).block();
        Arrivals arr = new Arrivals(1);
        t0 = System.nanoTime();
        System.err.println("T0");
        AtomicLong inflight = new AtomicLong();
        Thread gen = new Thread(() -> arr.generate(1));
        gen.start();
        Rng rng = new Rng(1);
        long intended;
        while ((intended = arr.take(0)) >= 0) {
            long start = now();
            long id = scenario.equals("s2") ? rng.next(rowsN) : 0;
            final long in = intended;
            inflight.incrementAndGet();
            Mono.usingWhen(cp.create(), c -> queryR2(c, id), c -> Mono.from(c.close()))
                    .subscribe(sum -> record(in, start, now(), null, sum),
                            e -> { record(in, start, now(), e, 0); inflight.decrementAndGet(); },
                            inflight::decrementAndGet);
        }
        gen.join();
        long deadline = System.nanoTime() + 6_000_000_000L;
        while (inflight.get() > 0 && System.nanoTime() < deadline) Thread.sleep(5);
        cp.dispose();
        return "r2dbc-postgresql 1.0";
    }

    static Mono<Long> queryR2(Connection c, long id) {
        io.r2dbc.spi.Statement st = c.createStatement(sql);
        if (scenario.equals("s2")) st.bind("$1", id);
        return Flux.from(st.execute()).flatMap(res -> res.map((row, md) -> {
            switch (scenario) {
                case "s1": return (long) row.get(0, Integer.class);
                case "s2": {
                    long rid = row.get(0, Long.class);
                    if (rid != id) throw new IllegalStateException("S2 returned id " + rid + ", wanted " + id);
                    return rid + row.get(1, Integer.class) + row.get(2, Short.class) + (row.get(3, Boolean.class) ? 1 : 0)
                            + (long) (double) row.get(4, Double.class) + row.get(5, String.class).length() + row.get(6, String.class).length()
                            + row.get(7, Long.class) + row.get(8, String.class).length() + row.get(9, Long.class);
                }
                default: return (long) row.get(0, Integer.class) + row.get(1, String.class).length();
            }
        })).collectList().map(l -> {
            if (l.size() != ROWS.get(scenario)) throw new IllegalStateException(l.size() + " rows, wanted " + ROWS.get(scenario));
            long s = 0; for (long v : l) s += v; return s;
        });
    }
}
