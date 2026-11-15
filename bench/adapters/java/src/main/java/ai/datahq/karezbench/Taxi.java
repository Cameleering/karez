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
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.time.LocalDateTime;
import java.time.ZoneOffset;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicLong;

/** The NYC taxi scenarios R1 to R4 of the JVM adapters (real data, NULLs, timestamps). */
final class Taxi {
    static final String COLS = "trip_id, vendor, pickup, dropoff, passengers, distance, ratecode, flag, pu, do_, payment, fare, extra, tip, tolls, total, congestion";
    static final String R1 = "SELECT " + COLS + " FROM taxi_trips WHERE trip_id = $1";
    static final String R2 = "SELECT " + COLS + " FROM taxi_trips WHERE trip_id BETWEEN $1 AND $1 + 99 ORDER BY trip_id";
    static final String R3 = "SELECT pu, count(*), max(fare), max(total) FROM taxi_trips WHERE pu = $1 GROUP BY pu";
    static long taxiRows;

    static String sqlOf(String k) { return k.equals("r1") ? R1 : k.equals("r2") ? R2 : R3; }
    static int rowsOf(String k) { return k.equals("r2") ? 100 : 1; }
    static List<String> kinds(String sc) { return sc.equals("r4") ? List.of("r1", "r2", "r3") : List.of(sc); }

    /** The concrete query of one request, and its parameter. */
    static Object[] request(String sc, Main.Rng rng) {
        String k = sc;
        if (sc.equals("r4")) { long v = rng.next(100); k = v <= 80 ? "r1" : v <= 100 ? "r2" : "r3"; }
        long id = switch (k) {
            case "r1" -> rng.next(taxiRows);
            case "r2" -> rng.next(Math.max(1, taxiRows - 99));
            default -> rng.next(260);
        };
        return new Object[]{k, id};
    }

    static long bits(double d) { return Double.doubleToRawLongBits(d); }
    static long us(LocalDateTime t) { return t.toEpochSecond(ZoneOffset.UTC) * 1_000_000L + t.getNano() / 1000; }

    static void checkRows(String k, int n) {
        if (k.equals("r3") ? n > 1 : n != rowsOf(k)) throw new IllegalStateException(n + " rows, wanted " + rowsOf(k));
    }

    static void checkId(String k, long got, long id, int i) {
        long want = k.equals("r2") ? id + i : id;
        if (got != want) throw new IllegalStateException(k + " returned id " + got + ", wanted " + want);
    }

    // ---- pgJDBC -----------------------------------------------------------------------------------

    /** The checksum of the current row of an R1/R2/S8 result (17 columns). */
    static long rowSum(ResultSet rs) throws Exception {
        long s = rs.getLong(1) + rs.getShort(2) + us(rs.getObject(3, LocalDateTime.class)) + us(rs.getObject(4, LocalDateTime.class));
        short pass = rs.getShort(5); s += rs.wasNull() ? 0 : pass;
        s += bits(rs.getDouble(6));
        short rc = rs.getShort(7); s += rs.wasNull() ? 0 : rc;
        String flag = rs.getString(8); s += flag == null ? 0 : flag.length();
        s += rs.getInt(9) + rs.getInt(10) + rs.getShort(11) + bits(rs.getDouble(12)) + bits(rs.getDouble(13)) + bits(rs.getDouble(14))
                + bits(rs.getDouble(15)) + bits(rs.getDouble(16));
        double cg = rs.getDouble(17); s += rs.wasNull() ? 0 : bits(cg);
        return s;
    }

    static long foldJdbc(ResultSet rs, String k, long id) throws Exception {
        int n = 0; long sum = 0;
        while (rs.next()) {
            if (k.equals("r3")) {
                long pu = rs.getInt(1);
                if (pu != id) throw new IllegalStateException("R3 returned zone " + pu + ", wanted " + id);
                sum += pu + rs.getLong(2) + bits(rs.getDouble(3)) + bits(rs.getDouble(4));
            } else {
                checkId(k, rs.getLong(1), id, n);
                long s = rs.getLong(1) + rs.getShort(2) + us(rs.getObject(3, LocalDateTime.class)) + us(rs.getObject(4, LocalDateTime.class));
                short pass = rs.getShort(5); s += rs.wasNull() ? 0 : pass;
                s += bits(rs.getDouble(6));
                short rc = rs.getShort(7); s += rs.wasNull() ? 0 : rc;
                String flag = rs.getString(8); s += flag == null ? 0 : flag.length();
                s += rs.getInt(9) + rs.getInt(10) + rs.getShort(11) + bits(rs.getDouble(12)) + bits(rs.getDouble(13)) + bits(rs.getDouble(14))
                        + bits(rs.getDouble(15)) + bits(rs.getDouble(16));
                double cg = rs.getDouble(17); s += rs.wasNull() ? 0 : bits(cg);
                sum += s;
            }
            n++;
        }
        checkRows(k, n);
        return sum;
    }

    static String runJdbc(boolean tuned) throws Exception {
        URI u = URI.create(Main.url.replaceFirst("^postgres(ql)?://", "http://"));
        String[] ui = u.getUserInfo().split(":", 2);
        HikariConfig hc = new HikariConfig();
        hc.setJdbcUrl(Main.jdbcUrl(tuned));
        hc.setUsername(ui[0]); hc.setPassword(ui[1]);
        int size = Main.pool > 0 ? Main.pool : Main.conns;
        hc.setMaximumPoolSize(size); hc.setMinimumIdle(size); hc.setConnectionTimeout(60_000);
        HikariDataSource ds = new HikariDataSource(hc);
        List<java.sql.Connection> held = new ArrayList<>();
        for (int i = 0; i < size; i++) {
            java.sql.Connection c = ds.getConnection();
            for (int rep = 0; rep < (tuned ? 2 : 8); rep++) for (String k : kinds(Main.scenario)) {
                try (PreparedStatement ps = c.prepareStatement(sqlOf(k).replace("$1", "?"))) {
                    ps.setObject(1, k.equals("r3") ? (Object) 1 : (Object) 1L);
                    if (k.equals("r2")) ps.setLong(2, 1);
                    try (ResultSet rs = ps.executeQuery()) { while (rs.next()) { } }
                }
            }
            held.add(c);
        }
        for (java.sql.Connection c : held) c.close();
        Main.Arrivals arr = new Main.Arrivals(Main.conns);
        Main.t0 = System.nanoTime();
        System.err.println("T0");
        List<Thread> ws = new ArrayList<>();
        for (int w = 0; w < Main.conns; w++) {
            final int wi = w;
            Thread t = new Thread(() -> {
                Main.Rng rng = new Main.Rng(wi + 1);
                try {
                    long intended;
                    while ((intended = arr.take(wi)) >= 0) {
                        long start = Main.now();
                        if (start > Main.endNs + 5_000_000_000L) { if (intended >= Main.warmNs && intended < Main.endNs) Main.dropped.incrementAndGet(); continue; }
                        Object[] rq = request(Main.scenario, rng);
                        String k = (String) rq[0]; long id = (Long) rq[1];
                        try (java.sql.Connection c = ds.getConnection(); PreparedStatement ps = c.prepareStatement(sqlOf(k).replace("$1", "?"))) {
                            if (k.equals("r3")) ps.setInt(1, (int) id); else ps.setLong(1, id);
                            if (k.equals("r2")) ps.setLong(2, id);
                            try (ResultSet rs = ps.executeQuery()) {
                                long sum = foldJdbc(rs, k, id);
                                Main.record(intended, start, Main.now(), null, sum);
                            }
                        } catch (Throwable e) { Main.record(intended, start, Main.now(), e, 0); }
                    }
                } catch (InterruptedException ignored) { }
            });
            t.start();
            ws.add(t);
        }
        arr.generate(Main.conns);
        for (Thread t : ws) t.join();
        ds.close();
        return "pgjdbc " + org.postgresql.util.DriverInfo.DRIVER_VERSION + " + HikariCP" + (tuned ? " (tuned)" : " (defaults)");
    }

    // ---- vertx-pg-client ------------------------------------------------------------------------------

    static long foldVertx(RowSet<Row> rs, String k, long id) {
        int n = 0; long sum = 0;
        for (Row r : rs) {
            if (k.equals("r3")) {
                long pu = r.getInteger(0);
                if (pu != id) throw new IllegalStateException("R3 returned zone " + pu + ", wanted " + id);
                sum += pu + r.getLong(1) + bits(r.getDouble(2)) + bits(r.getDouble(3));
            } else {
                checkId(k, r.getLong(0), id, n);
                Short pass = r.getShort(4), rc = r.getShort(6); String flag = r.getString(7); Double cg = r.getDouble(16);
                sum += r.getLong(0) + r.getShort(1) + us(r.getLocalDateTime(2)) + us(r.getLocalDateTime(3)) + (pass == null ? 0 : pass)
                        + bits(r.getDouble(5)) + (rc == null ? 0 : rc) + (flag == null ? 0 : flag.length()) + r.getInteger(8) + r.getInteger(9)
                        + r.getShort(10) + bits(r.getDouble(11)) + bits(r.getDouble(12)) + bits(r.getDouble(13)) + bits(r.getDouble(14))
                        + bits(r.getDouble(15)) + (cg == null ? 0 : bits(cg));
            }
            n++;
        }
        checkRows(k, n);
        return sum;
    }

    static String runVertx(boolean pipelined) throws Exception {
        Vertx vertx = Vertx.vertx();
        PgConnectOptions co = PgConnectOptions.fromUri(Main.url.replaceFirst("\\?.*$", ""));
        if ("verify-full".equals(Main.query("sslmode"))) {
            co.setSslMode(io.vertx.pgclient.SslMode.VERIFY_FULL).setHostnameVerificationAlgorithm("HTTPS")
              .setPemTrustOptions(new io.vertx.core.net.PemTrustOptions().addCertPath(Main.query("sslrootcert")));
        }
        co.setCachePreparedStatements(true);
        co.setPipeliningLimit(pipelined ? 256 : 1);
        int size = Main.pool > 0 ? Main.pool : Main.conns;
        Pool p = PgBuilder.pool().with(new PoolOptions().setMaxSize(size).setMaxWaitQueueSize(-1)).connectingTo(co).using(vertx).build();
        List<String> ks = kinds(Main.scenario);
        CountDownLatch warm = new CountDownLatch(size * 4 * ks.size());
        for (int i = 0; i < size * 4; i++) for (String k : ks)
            p.preparedQuery(sqlOf(k)).execute(Tuple.of(k.equals("r3") ? (Object) 1 : (Object) 1L)).onComplete(r -> warm.countDown());
        warm.await();
        Main.Arrivals arr = new Main.Arrivals(1);
        Main.t0 = System.nanoTime();
        System.err.println("T0");
        AtomicLong inflight = new AtomicLong();
        Thread gen = new Thread(() -> arr.generate(1));
        gen.start();
        Main.Rng rng = new Main.Rng(1);
        long intended;
        while ((intended = arr.take(0)) >= 0) {
            long start = Main.now();
            Object[] rq = request(Main.scenario, rng);
            final String k = (String) rq[0]; final long id = (Long) rq[1];
            final long in = intended;
            inflight.incrementAndGet();
            p.preparedQuery(sqlOf(k)).execute(Tuple.of(k.equals("r3") ? (Object) (int) id : (Object) id))
                    .onComplete(r -> {
                        try {
                            if (r.failed()) Main.record(in, start, Main.now(), r.cause(), 0);
                            else Main.record(in, start, Main.now(), null, foldVertx(r.result(), k, id));
                        } catch (Throwable e) { Main.record(in, start, Main.now(), e, 0); }
                        inflight.decrementAndGet();
                    });
        }
        gen.join();
        long deadline = System.nanoTime() + 6_000_000_000L;
        while (inflight.get() > 0 && System.nanoTime() < deadline) Thread.sleep(5);
        vertx.close().toCompletionStage().toCompletableFuture().get(10, java.util.concurrent.TimeUnit.SECONDS);
        return "vertx-pg-client";
    }

    // ---- r2dbc-postgresql ---------------------------------------------------------------------------

    static Mono<Long> queryR2(Connection c, String k, long id) {
        io.r2dbc.spi.Statement st = c.createStatement(sqlOf(k));
        if (k.equals("r3")) st.bind("$1", (int) id); else st.bind("$1", id);
        int[] idx = {0};
        return Flux.from(st.execute()).flatMap(res -> res.map((row, md) -> {
            if (k.equals("r3")) {
                long pu = row.get(0, Integer.class);
                if (pu != id) throw new IllegalStateException("R3 returned zone " + pu + ", wanted " + id);
                return pu + row.get(1, Long.class) + bits(row.get(2, Double.class)) + bits(row.get(3, Double.class));
            }
            checkId(k, row.get(0, Long.class), id, idx[0]++);
            Short pass = row.get(4, Short.class), rc = row.get(6, Short.class); String flag = row.get(7, String.class); Double cg = row.get(16, Double.class);
            return row.get(0, Long.class) + row.get(1, Short.class) + us(row.get(2, LocalDateTime.class)) + us(row.get(3, LocalDateTime.class))
                    + (pass == null ? 0 : pass) + bits(row.get(5, Double.class)) + (rc == null ? 0 : rc) + (flag == null ? 0 : flag.length())
                    + row.get(8, Integer.class) + row.get(9, Integer.class) + row.get(10, Short.class) + bits(row.get(11, Double.class))
                    + bits(row.get(12, Double.class)) + bits(row.get(13, Double.class)) + bits(row.get(14, Double.class))
                    + bits(row.get(15, Double.class)) + (cg == null ? 0 : bits(cg));
        })).collectList().map(l -> {
            checkRows(k, l.size());
            long s = 0; for (long v : l) s += v; return s;
        });
    }

    static String runR2dbc() throws Exception {
        URI u = URI.create(Main.url.replaceFirst("^postgres(ql)?://", "http://"));
        String[] ui = u.getUserInfo().split(":", 2);
        PostgresqlConnectionConfiguration.Builder cc = PostgresqlConnectionConfiguration.builder()
                .host(u.getHost()).port(u.getPort() < 0 ? 5432 : u.getPort()).database(u.getPath().substring(1))
                .username(ui[0]).password(ui[1]).preparedStatementCacheQueries(256);
        if ("verify-full".equals(Main.query("sslmode"))) cc = cc.sslMode(io.r2dbc.postgresql.client.SSLMode.VERIFY_FULL).sslRootCert(Main.query("sslrootcert"));
        int size = Main.pool > 0 ? Main.pool : Main.conns;
        ConnectionPool cp = new ConnectionPool(ConnectionPoolConfiguration.builder(new PostgresqlConnectionFactory(cc.build()))
                .initialSize(size).maxSize(size).acquireRetry(0).build());
        List<Connection> held = new ArrayList<>();
        for (int i = 0; i < size; i++) held.add(cp.create().block());
        for (Connection c : held) for (String k : kinds(Main.scenario)) for (int rep = 0; rep < 3; rep++) {
            try { queryR2(c, k, 1).block(); } catch (Exception ignored) { }
        }
        for (Connection c : held) Mono.from(c.close()).block();
        Main.Arrivals arr = new Main.Arrivals(1);
        Main.t0 = System.nanoTime();
        System.err.println("T0");
        AtomicLong inflight = new AtomicLong();
        Thread gen = new Thread(() -> arr.generate(1));
        gen.start();
        Main.Rng rng = new Main.Rng(1);
        long intended;
        while ((intended = arr.take(0)) >= 0) {
            long start = Main.now();
            Object[] rq = request(Main.scenario, rng);
            final String k = (String) rq[0]; final long id = (Long) rq[1];
            final long in = intended;
            inflight.incrementAndGet();
            Mono.usingWhen(cp.create(), c -> queryR2(c, k, id), c -> Mono.from(c.close()))
                    .subscribe(sum -> Main.record(in, start, Main.now(), null, sum),
                            e -> { Main.record(in, start, Main.now(), e, 0); inflight.decrementAndGet(); },
                            inflight::decrementAndGet);
        }
        gen.join();
        long deadline = System.nanoTime() + 6_000_000_000L;
        while (inflight.get() > 0 && System.nanoTime() < deadline) Thread.sleep(5);
        cp.dispose();
        return "r2dbc-postgresql 1.0";
    }
}
