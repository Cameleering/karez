/* libpq adapter of the karez benchmark: the C ceiling. Same flags and output as rustbench
 * (bench/README.md). One thread and one connection per worker; statements prepared once; results
 * requested in binary format and decoded to native types; every column is folded into the
 * checksum. The pool of scenario S8 is a plain mutex-guarded list of connections (libpq has none).
 *
 * Build: cc -O2 -o libpq_adapter libpq_adapter.c -I$(pg_config --includedir) -L$(pg_config --libdir) -lpq -lpthread -lm
 */
#define _GNU_SOURCE
#include <libpq-fe.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <arpa/inet.h>

#define BUCKETS (64 * 60)

static const char *SQL1 = "SELECT 1";
static const char *SQL2 = "SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = $1";
static const char *SQL3 = "SELECT id, message FROM bench_fortune";

typedef struct { uint64_t *v; size_t cap, head, tail; pthread_mutex_t m; pthread_cond_t c; int closed; } queue_t;

static const char *client = "libpq", *url, *scenario = "s1";
static int conns = 1, pool_n = 0, per_conn = 0;
static double rate = 1000, duration = 10, warmup = 2;
static long rows_n = 100000;
static int repeats = 5;
static long bulk_rows_arg = 100000;
static uint64_t warm_ns, end_ns;
static struct timespec t0;
static _Atomic uint64_t hist[BUCKETS], svc[BUCKETS], okc, errc, dropc, sentc, checksum;
static char first_err[512];
static pthread_mutex_t err_m = PTHREAD_MUTEX_INITIALIZER;
static queue_t *queues;
static int nq;
static const char *sql;
static int expect_rows;

/* the pool for S8 */
static PGconn **pool_conns;
static int pool_free_n;
static pthread_mutex_t pool_m = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t pool_c = PTHREAD_COND_INITIALIZER;

static uint64_t now_ns(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)(t.tv_sec - t0.tv_sec) * 1000000000ull + (uint64_t)(t.tv_nsec - t0.tv_nsec);
}
static void sleep_ns(uint64_t ns) {
    struct timespec t = {(time_t)(ns / 1000000000ull), (long)(ns % 1000000000ull)};
    nanosleep(&t, NULL);
}
static int hidx(uint64_t v) {
    if (v < 128) return (int)v;
    int msb = 63 - __builtin_clzll(v), shift = msb - 6;
    int i = (shift << 6) + (int)(v >> shift);
    return i >= BUCKETS ? BUCKETS - 1 : i;
}
static void die(const char *m) { fprintf(stderr, "libpq_adapter: %s\n", m); exit(2); }

static void record(uint64_t intended, uint64_t start, uint64_t end, const char *err, uint64_t sum) {
    if (intended < warm_ns || intended >= end_ns) return;
    if (err) {
        errc++;
        pthread_mutex_lock(&err_m);
        if (!first_err[0]) snprintf(first_err, sizeof first_err, "%s", err);
        pthread_mutex_unlock(&err_m);
        return;
    }
    hist[hidx(end > intended ? end - intended : 0)]++;
    svc[hidx(end > start ? end - start : 0)]++;
    okc++;
    checksum += sum;
}

static uint64_t be64(const char *p) { uint64_t v; memcpy(&v, p, 8); return __builtin_bswap64(v); }
static uint32_t be32(const char *p) { uint32_t v; memcpy(&v, p, 4); return ntohl(v); }
static uint16_t be16(const char *p) { uint16_t v; memcpy(&v, p, 2); return ntohs(v); }

/* run one query on c and fold the result; returns NULL or an error message */
static const char *run_query(PGconn *c, long id, uint64_t *sum_out, char *errbuf, size_t errlen) {
    PGresult *r;
    if (!strcmp(scenario, "s2")) {
        uint64_t be = __builtin_bswap64((uint64_t)id);
        const char *vals[1] = {(const char *)&be};
        int lens[1] = {8}, fmts[1] = {1};
        r = PQexecPrepared(c, "q", 1, vals, lens, fmts, 1);
    } else {
        r = PQexecPrepared(c, "q", 0, NULL, NULL, NULL, 1);
    }
    if (PQresultStatus(r) != PGRES_TUPLES_OK) {
        snprintf(errbuf, errlen, "%s", PQerrorMessage(c));
        PQclear(r);
        return errbuf;
    }
    int n = PQntuples(r);
    if (n != expect_rows) { snprintf(errbuf, errlen, "%d rows, wanted %d", n, expect_rows); PQclear(r); return errbuf; }
    uint64_t sum = 0;
    for (int i = 0; i < n; i++) {
        if (!strcmp(scenario, "s1")) sum += be32(PQgetvalue(r, i, 0));
        else if (!strcmp(scenario, "s2")) {
            long rid = (long)be64(PQgetvalue(r, i, 0));
            if (rid != id) { snprintf(errbuf, errlen, "S2 returned id %ld, wanted %ld", rid, id); PQclear(r); return errbuf; }
            double score; uint64_t sb = be64(PQgetvalue(r, i, 4)); memcpy(&score, &sb, 8);
            sum += (uint64_t)rid + be32(PQgetvalue(r, i, 1)) + be16(PQgetvalue(r, i, 2)) + (uint8_t)PQgetvalue(r, i, 3)[0]
                 + (uint64_t)(int64_t)score + (uint64_t)PQgetlength(r, i, 5) + (uint64_t)PQgetlength(r, i, 6)
                 + be64(PQgetvalue(r, i, 7)) + (uint64_t)PQgetlength(r, i, 8) + be64(PQgetvalue(r, i, 9));
        } else sum += be32(PQgetvalue(r, i, 0)) + (uint64_t)PQgetlength(r, i, 1);
    }
    PQclear(r);
    *sum_out = sum;
    return NULL;
}

static PGconn *open_conn(void) {
    PGconn *c = PQconnectdb(url);
    if (PQstatus(c) != CONNECTION_OK) { fprintf(stderr, "libpq_adapter: connect: %s", PQerrorMessage(c)); exit(2); }
    PGresult *r = PQprepare(c, "q", sql, 0, NULL);
    if (PQresultStatus(r) != PGRES_COMMAND_OK) { fprintf(stderr, "libpq_adapter: prepare: %s", PQerrorMessage(c)); exit(2); }
    PQclear(r);
    return c;
}

static int64_t q_pop(queue_t *q) {
    pthread_mutex_lock(&q->m);
    while (q->head == q->tail && !q->closed) pthread_cond_wait(&q->c, &q->m);
    int64_t v = -1;
    if (q->head != q->tail) v = (int64_t)q->v[q->head++];
    pthread_mutex_unlock(&q->m);
    return v;
}
static void q_push(queue_t *q, uint64_t v) {
    pthread_mutex_lock(&q->m);
    if (q->tail < q->cap) q->v[q->tail++] = v;
    pthread_mutex_unlock(&q->m);
    pthread_cond_signal(&q->c);
}

static pthread_barrier_t b1, b2;

static void *worker_run(void *arg) {
    long w = (long)arg;
    uint64_t rng = ((uint64_t)(w + 1) * 0x9E3779B97F4A7C15ull) | 1;
    PGconn *own = pool_n > 0 ? NULL : open_conn();
    pthread_barrier_wait(&b1);
    pthread_barrier_wait(&b2);
    char eb[512];
    for (;;) {
        int64_t intended;
        if (rate == 0) { intended = (int64_t)now_ns(); if ((uint64_t)intended >= end_ns) break; }
        else { intended = q_pop(&queues[w % nq]); if (intended < 0) break; }
        uint64_t start = now_ns();
        if (start > end_ns + 5000000000ull) { if ((uint64_t)intended >= warm_ns && (uint64_t)intended < end_ns) dropc++; continue; }
        long id = 0;
        if (!strcmp(scenario, "s2")) {
            uint64_t x = rng; x ^= x >> 12; x ^= x << 25; x ^= x >> 27; rng = x;
            id = (long)((x * 0x2545F4914F6CDD1Dull) >> 11) % rows_n + 1;
        }
        PGconn *c = own;
        int slot = -1;
        if (pool_n > 0) {
            pthread_mutex_lock(&pool_m);
            while (pool_free_n == 0) pthread_cond_wait(&pool_c, &pool_m);
            slot = --pool_free_n;
            c = pool_conns[slot];
            pthread_mutex_unlock(&pool_m);
        }
        uint64_t sum = 0;
        const char *err = run_query(c, id, &sum, eb, sizeof eb);
        if (pool_n > 0) {
            pthread_mutex_lock(&pool_m);
            pool_conns[pool_free_n++] = c;
            pthread_mutex_unlock(&pool_m);
            pthread_cond_signal(&pool_c);
        }
        record((uint64_t)intended, start, now_ns(), err, sum);
    }
    if (own) PQfinish(own);
    return NULL;
}

static void print_hist(_Atomic uint64_t *h) {
    int first = 1;
    putchar('[');
    for (int i = 0; i < BUCKETS; i++) {
        uint64_t c = h[i];
        if (c) { printf("%s[%d,%llu]", first ? "" : ",", i, (unsigned long long)c); first = 0; }
    }
    putchar(']');
}

/* ---- bulk scenarios (S4 scan, S5 batch insert, S6 COPY): closed loop, see rustbench/src/bulk.rs ---- */
static double cpu_now(void) {
    double user = 0, sys = 0;
    FILE *f = fopen("/proc/self/stat", "r");
    if (f) {
        char buf[2048]; size_t n = fread(buf, 1, sizeof buf - 1, f); buf[n] = 0; fclose(f);
        char *p = strrchr(buf, ')');
        if (p) { unsigned long ut = 0, st = 0; sscanf(p + 2, "%*c %*d %*d %*d %*d %*d %*u %*u %*u %*u %*u %lu %lu", &ut, &st); user = ut / 100.0; sys = st / 100.0; }
    }
    return user + sys;
}
static double secs_now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9; }

static void s2_hex(long i, char *out) {
    __uint128_t mul = ((__uint128_t)0x9E3779B97F4A7C15ull << 64) | 0xF39CC0605CEDC835ull;
    __uint128_t v = (__uint128_t)(unsigned long)i * mul;
    snprintf(out, 33, "%016llx%016llx", (unsigned long long)(v >> 64), (unsigned long long)v);
}

static void bulk_die(PGconn *c, const char *what) { fprintf(stderr, "libpq_adapter: %s: %s", what, PQerrorMessage(c)); exit(2); }

static uint64_t bulk_count(PGconn *c) {
    PGresult *r = PQexec(c, "SELECT count(*) FROM bench_ins");
    uint64_t n = (uint64_t)atoll(PQgetvalue(r, 0, 0));
    PQclear(r);
    return n;
}

static void put64(char *p, uint64_t v) { v = __builtin_bswap64(v); memcpy(p, &v, 8); }
static void put32(char *p, uint32_t v) { v = htonl(v); memcpy(p, &v, 4); }
static void put16(char *p, uint16_t v) { v = htons(v); memcpy(p, &v, 2); }

static void bulk_pass(PGconn *c, const char *sc, long rows, uint64_t *out_rows, uint64_t *out_sum) {
    *out_rows = 0; *out_sum = 0;
    if (!strcmp(sc, "s4")) {
        PGresult *r = PQexecPrepared(c, "scan", 0, NULL, NULL, NULL, 1);
        if (PQresultStatus(r) != PGRES_TUPLES_OK) bulk_die(c, "scan");
        int n = PQntuples(r);
        uint64_t sum = 0;
        for (int i = 0; i < n; i++) {
            double d, e; uint64_t db = be64(PQgetvalue(r, i, 4)), eb = be64(PQgetvalue(r, i, 5));
            memcpy(&d, &db, 8); memcpy(&e, &eb, 8);
            sum += be64(PQgetvalue(r, i, 0)) + (uint32_t)be32(PQgetvalue(r, i, 1)) + (uint32_t)be32(PQgetvalue(r, i, 2)) + be64(PQgetvalue(r, i, 3))
                 + db + eb + (uint64_t)PQgetlength(r, i, 6) + (uint64_t)PQgetlength(r, i, 7) + (uint8_t)PQgetvalue(r, i, 8)[0] + be16(PQgetvalue(r, i, 9));
            (void)d; (void)e;
        }
        /* a, b are int4 (non-negative here), f is int2 (non-negative): the unsigned sums equal the signed ones */
        *out_rows = (uint64_t)n; *out_sum = sum;
        PQclear(r);
    } else if (!strcmp(sc, "s5")) {
        PQclear(PQexec(c, "TRUNCATE bench_ins"));
        PQclear(PQexec(c, "BEGIN"));
        if (PQenterPipelineMode(c) != 1) bulk_die(c, "pipeline");
        for (long start = 1; start <= rows; start += 1000) {
            long end = start + 999 < rows ? start + 999 : rows;
            for (long i = start; i <= end; i++) {
                char b_id[8], b_a[4], b_b[4], b_c[8], b_d[8], b_e[8], b_f[2], b_flag[1], hex[33], s1[32];
                put64(b_id, (uint64_t)i); put32(b_a, (uint32_t)(i % 100000)); put32(b_b, (uint32_t)((i * 17) % 65536)); put64(b_c, (uint64_t)(i * 10007));
                double d = (double)i / 3.0, e = (double)(i % 1000) * 0.25; uint64_t db, eb; memcpy(&db, &d, 8); memcpy(&eb, &e, 8);
                put64(b_d, db); put64(b_e, eb); put16(b_f, (uint16_t)(i % 1000)); b_flag[0] = (i % 2 == 0);
                s2_hex(i, hex); snprintf(s1, sizeof s1, "row-%ld", i);
                const char *vals[10] = {b_id, b_a, b_b, b_c, b_d, b_e, s1, hex, b_flag, b_f};
                int lens[10] = {8, 4, 4, 8, 8, 8, (int)strlen(s1), 32, 1, 2};
                int fmts[10] = {1, 1, 1, 1, 1, 1, 0, 0, 1, 1};
                if (PQsendQueryPrepared(c, "ins", 10, vals, lens, fmts, 0) != 1) bulk_die(c, "send");
            }
            PQpipelineSync(c);
            for (;;) {
                PGresult *r = PQgetResult(c);
                if (!r) continue;
                ExecStatusType st = PQresultStatus(r);
                PQclear(r);
                if (st == PGRES_PIPELINE_SYNC) break;
                if (st == PGRES_FATAL_ERROR || st == PGRES_PIPELINE_ABORTED) bulk_die(c, "insert");
                /* each command ends with a NULL result */
                while ((r = PQgetResult(c)) != NULL) PQclear(r);
            }
        }
        PQexitPipelineMode(c);
        PQclear(PQexec(c, "COMMIT"));
        *out_rows = bulk_count(c);
    } else {
        PQclear(PQexec(c, "TRUNCATE bench_ins"));
        PGresult *r = PQexec(c, "COPY bench_ins (id, a, b, c, d, e, s1, s2, flag, f) FROM STDIN");
        if (PQresultStatus(r) != PGRES_COPY_IN) bulk_die(c, "copy");
        PQclear(r);
        char *buf = malloc(80 * 1024); size_t len = 0;
        for (long i = 1; i <= rows; i++) {
            char hex[33]; s2_hex(i, hex);
            len += (size_t)snprintf(buf + len, 512, "%ld\t%ld\t%ld\t%ld\t%.17g\t%.17g\trow-%ld\t%s\t%c\t%ld\n", i, i % 100000, (i * 17) % 65536, i * 10007,
                                    (double)i / 3.0, (double)(i % 1000) * 0.25, i, hex, i % 2 == 0 ? 't' : 'f', i % 1000);
            if (len >= 64 * 1024 || i == rows) { if (PQputCopyData(c, buf, (int)len) != 1) bulk_die(c, "copy data"); len = 0; }
        }
        free(buf);
        if (PQputCopyEnd(c, NULL) != 1) bulk_die(c, "copy end");
        r = PQgetResult(c);
        if (PQresultStatus(r) != PGRES_COMMAND_OK) bulk_die(c, "copy result");
        PQclear(r);
        *out_rows = bulk_count(c);
    }
}

static void bulk_main(const char *sc, int repeats, long bulk_rows) {
    PGconn *c = PQconnectdb(url);
    if (PQstatus(c) != CONNECTION_OK) bulk_die(c, "connect");
    PGresult *r = PQprepare(c, "scan", "SELECT id, a, b, c, d, e, s1, s2, flag, f FROM bench_wide", 0, NULL); PQclear(r);
    r = PQprepare(c, "ins", "INSERT INTO bench_ins (id, a, b, c, d, e, s1, s2, flag, f) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)", 0, NULL);
    if (PQresultStatus(r) != PGRES_COMMAND_OK) bulk_die(c, "prepare");
    PQclear(r);
    uint64_t nr, sum;
    bulk_pass(c, sc, !strcmp(sc, "s4") ? 0 : 1000, &nr, &sum);
    printf("{\"harness\":\"libpq_adapter\",\"bulk\":true,\"client\":\"libpq\",\"version\":\"libpq %d\",\"scenario\":\"%c%c\",\"rows_target\":%ld,\"runs\":[", PQlibVersion(), sc[0] - 32, sc[1], bulk_rows);
    for (int k = 0; k < repeats; k++) {
        double c0 = cpu_now(), t0 = secs_now();
        bulk_pass(c, sc, bulk_rows, &nr, &sum);
        printf("%s{\"secs\":%.4f,\"cpu_s\":%.3f,\"rows\":%llu,\"checksum\":%llu}", k ? "," : "", secs_now() - t0, cpu_now() - c0, (unsigned long long)nr, (unsigned long long)sum);
    }
    double rss = 0; FILE *f = fopen("/proc/self/status", "r");
    if (f) { char l[256]; while (fgets(l, sizeof l, f)) if (!strncmp(l, "VmHWM:", 6)) rss = atof(l + 6); fclose(f); }
    printf("],\"rss_kb\":%.0f}\n", rss);
    PQfinish(c);
    exit(0);
}

int main(int argc, char **argv) {
    for (int i = 1; i + 1 < argc; i += 2) {
        const char *k = argv[i] + 2, *v = argv[i + 1];
        if (!strcmp(k, "client")) client = v;
        else if (!strcmp(k, "url")) url = v;
        else if (!strcmp(k, "scenario")) scenario = v;
        else if (!strcmp(k, "mode")) { if (!strcmp(v, "pipelined")) die("libpq: unpipelined only"); }
        else if (!strcmp(k, "conns")) conns = atoi(v);
        else if (!strcmp(k, "pool")) pool_n = atoi(v);
        else if (!strcmp(k, "rate")) rate = atof(v);
        else if (!strcmp(k, "duration")) duration = atof(v);
        else if (!strcmp(k, "warmup")) warmup = atof(v);
        else if (!strcmp(k, "rows")) rows_n = atol(v);
        else if (!strcmp(k, "dispatch")) per_conn = !strcmp(v, "perconn");
        else if (!strcmp(k, "repeats")) repeats = atoi(v);
        else if (!strcmp(k, "bulk-rows")) bulk_rows_arg = atol(v);
    }
    if (!url) die("--url is required");
    if (!strcmp(scenario, "s4") || !strcmp(scenario, "s5") || !strcmp(scenario, "s6")) bulk_main(scenario, repeats, bulk_rows_arg);
    sql = !strcmp(scenario, "s1") ? SQL1 : !strcmp(scenario, "s2") ? SQL2 : SQL3;
    expect_rows = !strcmp(scenario, "s3") ? 12 : 1;
    warm_ns = (uint64_t)(warmup * 1e9);
    end_ns = (uint64_t)((warmup + duration) * 1e9);
    nq = per_conn ? conns : 1;
    queues = calloc((size_t)nq, sizeof(queue_t));
    size_t total = rate > 0 ? (size_t)(rate * (warmup + duration) / nq) + 1024 : 1;
    if (total > 200000000u) die("rate x duration too large for the queue");
    for (int i = 0; i < nq; i++) {
        queues[i].v = malloc(total * sizeof(uint64_t));
        queues[i].cap = total;
        pthread_mutex_init(&queues[i].m, NULL);
        pthread_cond_init(&queues[i].c, NULL);
    }
    if (pool_n > 0) {
        pool_conns = calloc((size_t)pool_n, sizeof(PGconn *));
        for (int i = 0; i < pool_n; i++) pool_conns[i] = open_conn();
        pool_free_n = pool_n;
    }
    pthread_barrier_init(&b1, NULL, (unsigned)conns + 1);
    pthread_barrier_init(&b2, NULL, (unsigned)conns + 1);
    pthread_t *th = calloc((size_t)conns, sizeof(pthread_t));
    for (long i = 0; i < conns; i++) pthread_create(&th[i], NULL, worker_run, (void *)i);
    pthread_barrier_wait(&b1);
    clock_gettime(CLOCK_MONOTONIC, &t0);
    fprintf(stderr, "T0\n");
    pthread_barrier_wait(&b2);
    if (rate == 0) sleep_ns(end_ns - now_ns());
    else {
        double period = 1e9 / rate;
        for (uint64_t k = 0;; k++) {
            uint64_t intended = (uint64_t)((double)k * period);
            if (intended >= end_ns) break;
            uint64_t n = now_ns();
            if (intended > n) sleep_ns(intended - n);
            if (intended >= warm_ns) sentc++;
            q_push(&queues[k % (uint64_t)nq], intended);
        }
        for (int i = 0; i < nq; i++) { pthread_mutex_lock(&queues[i].m); queues[i].closed = 1; pthread_mutex_unlock(&queues[i].m); pthread_cond_broadcast(&queues[i].c); }
    }
    for (int i = 0; i < conns; i++) pthread_join(th[i], NULL);
    double wall = now_ns() / 1e9;
    double user = 0, sys = 0, rss = 0;
    FILE *f = fopen("/proc/self/stat", "r");
    if (f) {
        char buf[2048]; size_t n = fread(buf, 1, sizeof buf - 1, f); buf[n] = 0; fclose(f);
        char *p = strrchr(buf, ')');
        if (p) { unsigned long ut = 0, st = 0; sscanf(p + 2, "%*c %*d %*d %*d %*d %*d %*u %*u %*u %*u %*u %lu %lu", &ut, &st); user = ut / 100.0; sys = st / 100.0; }
    }
    f = fopen("/proc/self/status", "r");
    if (f) { char l[256]; while (fgets(l, sizeof l, f)) if (!strncmp(l, "VmHWM:", 6)) rss = atof(l + 6); fclose(f); }
    int ver = PQlibVersion();
    printf("{\"harness\":\"libpq_adapter\",\"client\":\"libpq\",\"version\":\"libpq %d.%d\",\"scenario\":\"%c%c\",\"mode\":\"Unpipelined\","
           "\"conns\":%d,\"pool\":%d,\"rate\":%g,\"duration_s\":%g,\"warmup_s\":%g,\"dispatch\":\"%s\",\"rt_threads\":%d,"
           "\"sent\":%llu,\"ok\":%llu,\"errors\":%llu,\"dropped\":%llu,\"checksum\":%llu,\"first_error\":",
           ver / 10000, ver % 10000, scenario[0] - 32, scenario[1], conns, pool_n, rate, duration, warmup, per_conn ? "perconn" : "shared", conns,
           (unsigned long long)sentc, (unsigned long long)okc, (unsigned long long)errc, (unsigned long long)dropc, (unsigned long long)checksum);
    if (first_err[0]) { putchar('"'); for (char *p = first_err; *p; p++) putchar(*p == '"' || *p == '\n' || *p == '\\' ? ' ' : *p); putchar('"'); }
    else printf("null");
    printf(",\"wall_s\":%.3f,\"cpu_user_s\":%.2f,\"cpu_sys_s\":%.2f,\"rss_kb\":%.0f,\"hist\":", wall, user, sys, rss);
    print_hist(hist);
    printf(",\"service_hist\":");
    print_hist(svc);
    printf("}\n");
    return 0;
}
