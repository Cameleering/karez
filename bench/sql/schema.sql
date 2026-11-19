-- Dataset of the karez benchmark. Identical on every server, for every client.
-- Run once per server with: psql -v ON_ERROR_STOP=1 -f schema.sql   (bench/run.py setup does it)

DROP TABLE IF EXISTS bench_item, bench_fortune, bench_wide, bench_ins CASCADE;

-- S2: point select by primary key, one 10-column mixed row. 100,000 rows.
CREATE TABLE bench_item (
    id    int8 PRIMARY KEY,
    k1    int4 NOT NULL,
    k2    int2 NOT NULL,
    flag  bool NOT NULL,
    score float8 NOT NULL,
    name  text NOT NULL,
    note  text NOT NULL,
    ref   int8 NOT NULL,
    tag   text NOT NULL,
    big   int8 NOT NULL
);
INSERT INTO bench_item
SELECT g, ((g * 7919) % 1000003)::int4, (g % 30000)::int2, g % 3 = 0, g / 7.0,
       'item-' || g, repeat('note ', 8) || g, (g * 31) % 100000, 'tag' || (g % 97),
       g * 1000003
FROM generate_series(1::int8, 100000) AS g;

-- S3: Fortunes-like, 12 rows with a string column (as in TechEmpower).
CREATE TABLE bench_fortune (id int4 PRIMARY KEY, message text NOT NULL);
INSERT INTO bench_fortune VALUES
 (1,  'fortune: favourable winds'), (2,  'A computer program does what you write, not what you mean.'),
 (3,  'Any program that runs right is obsolete.'), (4,  'Premature optimisation is the root of all evil.'),
 (5,  'Measure twice, cut once.'), (6,  'There is no place like 127.0.0.1.'),
 (7,  'The cheapest, fastest and most reliable components are those that are not there.'),
 (8,  'A bad random number generator: 1, 1, 1, 1, 1, 4.33e+67, 1, 1, 1'),
 (9,  'フレームワークのベンチマーク'), (10, 'Be liberal in what you accept, conservative in what you send.'),
 (11, 'Cats are not a safe type for hash keys.'), (12, 'Latency is not throughput.');

-- S4: scan of 1,000,000 rows x 10 columns.
CREATE TABLE bench_wide (
    id  int8 NOT NULL, a int4 NOT NULL, b int4 NOT NULL, c int8 NOT NULL, d float8 NOT NULL,
    e   float8 NOT NULL, s1 text NOT NULL, s2 text NOT NULL, flag bool NOT NULL, f int2 NOT NULL
);
INSERT INTO bench_wide
SELECT g, (g % 100000)::int4, ((g * 17) % 65536)::int4, g * 10007, g / 3.0, (g % 1000) * 0.25,
       'row-' || g, md5(g::text), g % 2 = 0, (g % 1000)::int2
FROM generate_series(1::int8, 1000000) AS g;

-- S5 and S6: insert targets, same shape as bench_wide, empty. Truncated before every run.
CREATE TABLE bench_ins (LIKE bench_wide);

ANALYZE;
