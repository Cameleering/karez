package ai.datahq.karezbench;

import com.zaxxer.hikari.HikariConfig;
import com.zaxxer.hikari.HikariDataSource;
import io.vertx.core.Vertx;
import io.vertx.pgclient.PgBuilder;
import io.vertx.pgclient.PgConnectOptions;
import io.vertx.sqlclient.Pool;
import io.vertx.sqlclient.PoolOptions;
import io.vertx.sqlclient.Row;
import io.vertx.sqlclient.Tuple;
import io.r2dbc.postgresql.PostgresqlConnectionConfiguration;
import io.r2dbc.postgresql.PostgresqlConnectionFactory;
import reactor.core.publisher.Flux;
import reactor.core.publisher.Mono;

import java.io.IOException;
import java.io.Reader;
import java.math.BigInteger;
import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.atomic.AtomicLong;

/**
 * Bulk scenarios (S4 scan, S5 batch insert, S6 COPY) of the JVM adapters. Closed loop: one untimed
 * warm-up pass, then `repeats` timed passes. See bench/rustbench/src/bulk.rs for the checksum of S4
 * and the row recipe of S5 and S6.
 */
final class Bulk {
    static final String S4 = "SELECT id, a, b, c, d, e, s1, s2, flag, f FROM bench_wide";
    static final String COLS = "id, a, b, c, d, e, s1, s2, flag, f";
    static final String INS = "INSERT INTO bench_ins (" + COLS + ") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";
    static final BigInteger M128 = BigInteger.ONE.shiftLeft(128).subtract(BigInteger.ONE);
    static final BigInteger MUL = new BigInteger("9E3779B97F4A7C15F39CC0605CEDC835", 16);

    interface Pass { long[] run() throws Exception; }   // {rows, checksum}

    static String s2(long i) {
        String h = BigInteger.valueOf(i).multiply(MUL).and(M128).toString(16);
        return "0".repeat(32 - h.length()) + h;
    }

    static String text(long i) {
        return i + "\t" + (i % 100000) + "\t" + ((i * 17) % 65536) + "\t" + (i * 10007) + "\t" + (i / 3.0) + "\t"
                + ((i % 1000) * 0.25) + "\trow-" + i + "\t" + s2(i) + "\t" + (i % 2 == 0 ? "t" : "f") + "\t" + (i % 1000) + "\n";
    }

    static double cpu() {
        try {
            String s = Files.readString(Path.of("/proc/self/stat"));
            String[] f = s.substring(s.lastIndexOf(')') + 1).trim().split("\\s+");
            return (Double.parseDouble(f[11]) + Double.parseDouble(f[12])) / 100;
        } catch (Exception e) { return 0; }
    }

    static long rss() {
        try {
            for (String l : Files.readAllLines(Path.of("/proc/self/status")))
                if (l.startsWith("VmHWM:")) return Long.parseLong(l.split("\\s+")[1]);
        } catch (Exception ignored) { }
        return 0;
    }

    static String run(String client, String url, String sc, int repeats, long bulkRows) throws Exception {
        Pass one, warm;
        String version;
        URI u = URI.create(url.replaceFirst("^postgres(ql)?://", "http://"));
        String[] ui = u.getUserInfo().split(":", 2);
        switch (client) {
            case "pgjdbc", "pgjdbc-tuned" -> {
                boolean tuned = client.endsWith("tuned");
                HikariConfig hc = new HikariConfig();
                hc.setJdbcUrl(Main.jdbcUrl(tuned) + (tuned ? "&reWriteBatchedInserts=true" : ""));
                hc.setUsername(ui[0]); hc.setPassword(ui[1]);
                hc.setMaximumPoolSize(1); hc.setMinimumIdle(1);
                HikariDataSource ds = new HikariDataSource(hc);
                Connection c = ds.getConnection();
                c.setAutoCommit(false);
                String ins = INS.replaceAll("\\$\\d+", "?");
                Pass s4 = () -> {
                    try (PreparedStatement ps = c.prepareStatement(S4)) {
                        ps.setFetchSize(tuned ? 10000 : 0);      // defaults buffer the whole result
                        long rows = 0, sum = 0;
                        try (ResultSet rs = ps.executeQuery()) {
                            while (rs.next()) {
                                rows++;
                                sum += rs.getLong(1) + rs.getInt(2) + rs.getInt(3) + rs.getLong(4)
                                        + Double.doubleToRawLongBits(rs.getDouble(5)) + Double.doubleToRawLongBits(rs.getDouble(6))
                                        + rs.getString(7).length() + rs.getString(8).length() + (rs.getBoolean(9) ? 1 : 0) + rs.getShort(10);
                            }
                        }
                        c.commit();
                        return new long[]{rows, sum};
                    }
                };
                version = "pgjdbc " + org.postgresql.util.DriverInfo.DRIVER_VERSION + (tuned ? " (tuned)" : " (defaults)");
                Pass s8 = () -> {
                    try (PreparedStatement ps = c.prepareStatement("SELECT " + Taxi.COLS + " FROM taxi_trips")) {
                        ps.setFetchSize(tuned ? 20000 : 0);      // defaults buffer the whole result: 13M rows will not fit
                        long rows = 0, sum = 0;
                        try (ResultSet rs = ps.executeQuery()) {
                            while (rs.next()) { rows++; sum += Taxi.rowSum(rs); }
                        }
                        c.commit();
                        return new long[]{rows, sum};
                    }
                };
                if (sc.equals("s4")) { one = s4; warm = s4; }
                else if (sc.equals("s8")) { one = s8; warm = () -> new long[]{0, 0}; }
                else if (sc.equals("s5")) {
                    java.util.function.LongFunction<Pass> mk = n -> () -> {
                        try (java.sql.Statement st = c.createStatement()) { st.execute("TRUNCATE bench_ins"); }
                        try (PreparedStatement ps = c.prepareStatement(ins)) {
                            for (long i = 1; i <= n; i++) {
                                ps.setLong(1, i); ps.setInt(2, (int) (i % 100000)); ps.setInt(3, (int) ((i * 17) % 65536));
                                ps.setLong(4, i * 10007); ps.setDouble(5, i / 3.0); ps.setDouble(6, (i % 1000) * 0.25);
                                ps.setString(7, "row-" + i); ps.setString(8, s2(i)); ps.setBoolean(9, i % 2 == 0); ps.setShort(10, (short) (i % 1000));
                                ps.addBatch();
                                if (i % 1000 == 0) ps.executeBatch();
                            }
                            ps.executeBatch();
                        }
                        c.commit();
                        return count(c);
                    };
                    one = mk.apply(bulkRows); warm = mk.apply(1000);
                } else {
                    java.util.function.LongFunction<Pass> mk = n -> () -> {
                        try (java.sql.Statement st = c.createStatement()) { st.execute("TRUNCATE bench_ins"); }
                        c.commit();
                        org.postgresql.copy.CopyManager cm = new org.postgresql.copy.CopyManager(c.unwrap(org.postgresql.core.BaseConnection.class));
                        Reader r = new Reader() {
                            long next = 1; String buf = ""; int pos = 0;
                            @Override public int read(char[] out, int off, int len) throws IOException {
                                if (pos >= buf.length()) {
                                    if (next > n) return -1;
                                    StringBuilder sb = new StringBuilder();
                                    while (next <= n && sb.length() < 64 * 1024) sb.append(text(next++));
                                    buf = sb.toString(); pos = 0;
                                }
                                int k = Math.min(len, buf.length() - pos);
                                buf.getChars(pos, pos + k, out, off); pos += k;
                                return k;
                            }
                            @Override public void close() { }
                        };
                        cm.copyIn("COPY bench_ins (" + COLS + ") FROM STDIN", r);
                        c.commit();
                        return count(c);
                    };
                    one = mk.apply(bulkRows); warm = mk.apply(1000);
                }
            }
            case "pgbulkinsert" -> {
                // PgBulkInsert (binary COPY) on a pgjdbc connection; S6 only. Its row source is a lazy stream of row numbers,
                // so the row recipe is generated inside the timed pass, as in every other adapter.
                if (!sc.equals("s6")) throw new IllegalStateException("pgbulkinsert: only S6 is implemented");
                HikariConfig hc = new HikariConfig();
                hc.setJdbcUrl(Main.jdbcUrl(false));
                hc.setUsername(ui[0]); hc.setPassword(ui[1]);
                hc.setMaximumPoolSize(1); hc.setMinimumIdle(1);
                HikariDataSource ds = new HikariDataSource(hc);
                Connection c = ds.getConnection();
                c.setAutoCommit(false);
                de.bytefish.pgbulkinsert.PgBulkInsert<Long> bulk = new de.bytefish.pgbulkinsert.PgBulkInsert<>(new WideMapping());
                version = "pgbulkinsert 8.1.4 on pgjdbc " + org.postgresql.util.DriverInfo.DRIVER_VERSION;
                java.util.function.LongFunction<Pass> mk = n -> () -> {
                    try (java.sql.Statement st = c.createStatement()) { st.execute("TRUNCATE bench_ins"); }
                    c.commit();
                    bulk.saveAll(c.unwrap(org.postgresql.PGConnection.class),
                            java.util.stream.LongStream.rangeClosed(1, n).boxed());
                    c.commit();
                    return count(c);
                };
                one = mk.apply(bulkRows); warm = mk.apply(1000);
            }
            case "vertx-pg-client" -> {
                if (sc.equals("s6")) throw new IllegalStateException("vertx-pg-client has no COPY support");
                Vertx vertx = Vertx.vertx();
                PgConnectOptions co = PgConnectOptions.fromUri(url.replaceFirst("\\?.*$", ""));
                Pool p = PgBuilder.pool().with(new PoolOptions().setMaxSize(1)).connectingTo(co).using(vertx).build();
                Pass s4 = () -> {
                    var rs = p.preparedQuery(S4).execute().toCompletionStage().toCompletableFuture().get();
                    long rows = 0, sum = 0;
                    for (Row r : rs) {
                        rows++;
                        sum += r.getLong(0) + r.getInteger(1) + r.getInteger(2) + r.getLong(3)
                                + Double.doubleToRawLongBits(r.getDouble(4)) + Double.doubleToRawLongBits(r.getDouble(5))
                                + r.getString(6).length() + r.getString(7).length() + (r.getBoolean(8) ? 1 : 0) + r.getShort(9);
                    }
                    return new long[]{rows, sum};
                };
                version = "vertx-pg-client";
                if (sc.equals("s4")) { one = s4; warm = s4; }
                else {
                    java.util.function.LongFunction<Pass> mk = n -> () -> {
                        p.query("TRUNCATE bench_ins").execute().toCompletionStage().toCompletableFuture().get();
                        for (long start = 1; start <= n; start += 1000) {
                            List<Tuple> batch = new ArrayList<>();
                            for (long i = start; i < Math.min(n + 1, start + 1000); i++)
                                batch.add(Tuple.of(i, (int) (i % 100000), (int) ((i * 17) % 65536), i * 10007, i / 3.0, (i % 1000) * 0.25,
                                        "row-" + i, s2(i), i % 2 == 0, (short) (i % 1000)));
                            p.preparedQuery(INS).executeBatch(batch).toCompletionStage().toCompletableFuture().get();
                        }
                        var rs = p.query("SELECT count(*) FROM bench_ins").execute().toCompletionStage().toCompletableFuture().get();
                        return new long[]{rs.iterator().next().getLong(0), 0};
                    };
                    one = mk.apply(bulkRows); warm = mk.apply(1000);
                }
            }
            case "r2dbc-postgresql" -> {
                if (!sc.equals("s4")) throw new IllegalStateException("r2dbc-postgresql: only S4 is implemented");
                PostgresqlConnectionConfiguration cc = PostgresqlConnectionConfiguration.builder()
                        .host(u.getHost()).port(u.getPort() < 0 ? 5432 : u.getPort()).database(u.getPath().substring(1))
                        .username(ui[0]).password(ui[1]).build();
                io.r2dbc.spi.Connection c = new PostgresqlConnectionFactory(cc).create().block();
                Pass s4 = () -> {
                    AtomicLong rows = new AtomicLong(), sum = new AtomicLong();
                    Flux.from(c.createStatement(S4).execute()).flatMap(res -> res.map((row, md) -> {
                        sum.addAndGet(row.get(0, Long.class) + row.get(1, Integer.class) + row.get(2, Integer.class) + row.get(3, Long.class)
                                + Double.doubleToRawLongBits(row.get(4, Double.class)) + Double.doubleToRawLongBits(row.get(5, Double.class))
                                + row.get(6, String.class).length() + row.get(7, String.class).length() + (row.get(8, Boolean.class) ? 1 : 0) + row.get(9, Short.class));
                        rows.incrementAndGet();
                        return 1;
                    })).then().block();
                    return new long[]{rows.get(), sum.get()};
                };
                version = "r2dbc-postgresql 1.0";
                one = s4; warm = s4;
            }
            default -> throw new IllegalStateException("no bulk adapter for " + client);
        }
        warm.run();
        StringBuilder runs = new StringBuilder();
        for (int k = 0; k < repeats; k++) {
            double c0 = cpu(); long t0 = System.nanoTime();
            long[] r = one.run();
            double secs = (System.nanoTime() - t0) / 1e9;
            if (k > 0) runs.append(',');
            runs.append(String.format(java.util.Locale.ROOT, "{\"secs\":%.4f,\"cpu_s\":%.3f,\"rows\":%d,\"checksum\":%s}", secs, cpu() - c0, r[0], Long.toUnsignedString(r[1])));
        }
        return "{\"harness\":\"javaadapter\",\"bulk\":true,\"client\":\"" + client + "\",\"version\":\"" + version + "\",\"scenario\":\""
                + sc.toUpperCase() + "\",\"rows_target\":" + bulkRows + ",\"runs\":[" + runs + "],\"rss_kb\":" + rss() + "}";
    }

    /** The row recipe of S5 and S6 (bench/rustbench/src/bulk.rs) as a PgBulkInsert mapping over the row number. */
    static final class WideMapping extends de.bytefish.pgbulkinsert.mapping.AbstractMapping<Long> {
        WideMapping() {
            super("public", "bench_ins");
            mapLong("id", i -> i);
            mapInteger("a", i -> (int) (i % 100000));
            mapInteger("b", i -> (int) ((i * 17) % 65536));
            mapLong("c", i -> i * 10007);
            mapDouble("d", i -> i / 3.0);
            mapDouble("e", i -> (i % 1000) * 0.25);
            mapText("s1", i -> "row-" + i);
            mapText("s2", Bulk::s2);
            mapBoolean("flag", i -> i % 2 == 0);
            mapShort("f", i -> (short) (i % 1000));
        }
    }

    static long[] count(Connection c) throws Exception {
        try (java.sql.Statement st = c.createStatement(); ResultSet rs = st.executeQuery("SELECT count(*) FROM bench_ins")) {
            rs.next();
            long n = rs.getLong(1);
            c.commit();
            return new long[]{n, 0};
        }
    }
}
