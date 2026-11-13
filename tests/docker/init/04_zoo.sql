-- Awkward values on purpose, one small table per theme. Rows are selected by id in the tests.
SET ROLE karez_scram;

CREATE TABLE ints_zoo (id int PRIMARY KEY, i2 int2, i4 int4, i8 int8);
INSERT INTO ints_zoo VALUES
  (1, -32768, -2147483648, -9223372036854775808),
  (2,  32767,  2147483647,  9223372036854775807),
  (3, 0, 0, 0),
  (4, NULL, NULL, NULL);

CREATE TABLE floats_zoo (id int PRIMARY KEY, f4 float4, f8 float8);
INSERT INTO floats_zoo VALUES
  (1, 'NaN', 'NaN'),
  (2, 'Infinity', 'Infinity'),
  (3, '-Infinity', '-Infinity'),
  (4, '-0', '-0'),
  (5, '1.4e-45', '4.9e-324'),                          -- smallest subnormals
  (6, '3.4028235e38', '1.7976931348623157e308'),        -- largest finite
  (7, '-3.4028235e38', '-1.7976931348623157e308'),
  (8, NULL, NULL);

CREATE TABLE text_zoo (id int PRIMARY KEY, t text, v varchar(40), c char(8));
INSERT INTO text_zoo VALUES
  (1, '', '', ''),
  (2, repeat('x', 1048576), 'short', 'abc'),
  (3, 'héllo wörld, 日本語, Ελληνικά', 'ünï', 'çé'),
  (4, '🎉🚀 emoji 👩‍👩‍👧‍👦', '😀', '😀'),
  (5, E'it''s "quoted" \\ back\nnew\r\nline\ttab', 'a''b"c\d', 'x'),
  (6, NULL, NULL, NULL);

CREATE TABLE time_zoo (id int PRIMARY KEY, d date, ts timestamp, tstz timestamptz);
INSERT INTO time_zoo VALUES
  (1, '1900-01-01', '1900-01-01 00:00:00', '1900-01-01 00:00:00+00'),
  (2, '1999-12-31', '1999-12-31 23:59:59.999999', '1999-12-31 23:59:59.999999+00'),
  (3, '2000-01-01', '2000-01-01 00:00:00', '2000-01-01 00:00:00+00'),
  (4, '1970-01-01', '1970-01-01 00:00:00', '1970-01-01 00:00:00+00'),
  (5, '2100-12-31', '2100-12-31 12:34:56.789012', '2100-12-31 12:34:56.789012+00'),
  (6, '4713-01-01 BC', '4713-01-01 00:00:00 BC', '4713-01-01 00:00:00+00 BC'),   -- minimum
  (7, 'infinity', 'infinity', 'infinity'),
  (8, '-infinity', '-infinity', '-infinity'),
  -- US daylight-saving change on 2024-03-10: 06:59:59 UTC is 01:59:59 EST, 07:00:00 UTC is 03:00:00 EDT
  (9,  '2024-03-10', '2024-03-10 01:59:59', '2024-03-10 06:59:59+00'),
  (10, '2024-03-10', '2024-03-10 03:00:00', '2024-03-10 07:00:00+00'),
  (11, NULL, NULL, NULL),
  -- Largest values PostgreSQL accepts. The date fits a 32-bit Unix day count. karez stores Unix-epoch
  -- microseconds, which cannot hold the last ~30 years of PostgreSQL's timestamp range; the tests
  -- expect a clear error for row 99, not a wrong value.
  (98, '5874897-12-31', NULL, NULL),
  (99, NULL, '294276-12-31 23:59:59.999999', '294276-12-31 23:59:59.999999+00'),
  -- A large year that does fit:
  (97, NULL, '294000-01-01 00:00:00', '294000-01-01 00:00:00+00');

CREATE TABLE bytea_zoo (id int PRIMARY KEY, b bytea);
INSERT INTO bytea_zoo VALUES
  (1, ''::bytea),
  (2, '\x00'::bytea),
  (3, decode(repeat('00ff10', 349525), 'hex')),          -- 1,048,575 bytes, just under 1 MiB
  (4, decode(repeat('00ff', 2621440), 'hex')),           -- 5 MiB
  (5, NULL);

CREATE TABLE misc_zoo (id int PRIMARY KEY, u uuid, flag bool);
INSERT INTO misc_zoo VALUES
  (1, '00000000-0000-0000-0000-000000000000', true),
  (2, 'ffffffff-ffff-ffff-ffff-ffffffffffff', false),
  (3, '123e4567-e89b-12d3-a456-426614174000', NULL),
  (4, NULL, NULL);

-- One column always NULL, one never NULL, many rows.
CREATE TABLE null_zoo (id int PRIMARY KEY, always_null int4, never_null int4 NOT NULL,
                       half_null text);
INSERT INTO null_zoo
SELECT i, NULL, i, CASE WHEN i % 2 = 0 THEN 'v' || i END FROM generate_series(1, 5000) AS i;

-- n..ip have decoders (rendered to text); mny..ma do not and must produce a clear error that
-- names the type.
CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy');
CREATE TABLE unsupported_zoo (id int PRIMARY KEY, n numeric, j jsonb, arr int4[], iv interval,
                              m mood, ip inet, mny money, pt point, rng int4range,
                              tsv tsvector, ma mood[]);
INSERT INTO unsupported_zoo VALUES
  (1, 12345678901234567890.123456789, '{"a": [1, 2, {"b": null}]}', '{1,2,NULL,4}',
   '1 year 2 mons 3 days 04:05:06.789', 'happy', '192.168.1.0/24',
   '12.34', '(1,2)', '[1,10)', 'a:1 b:2', '{sad,happy}');

-- The healthcheck waits for this table: it exists only after the whole seed has loaded.
RESET ROLE;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO karez_clear, karez_trust;
ANALYZE;
CREATE TABLE seed_done (done int);
INSERT INTO seed_done VALUES (1);
