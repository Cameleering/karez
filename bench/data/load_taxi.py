#!/usr/bin/env python3
"""Load a real public dataset into the benchmark database: NYC TLC yellow taxi trip records
(https://www.nyc.gov/site/tlc/about/tlc-trip-record-data.page, public data published by the New York
City Taxi and Limousine Commission as monthly parquet files). About 3 million rows a month.

  load_taxi.py --url postgresql://bench:pw@host/bench --months 2024-01,2024-02,2024-03,2024-04 [--keep-files DIR]

Creates `taxi_trips` (17 columns of mixed types, NULLs where the source has them, primary key
`trip_id` numbered 1..N in file order, an index on `pu`) and writes the row count to `--rows-file`.
Needs pyarrow and psycopg (the benchmark virtualenv has both on the nodes).
"""
import argparse, os, sys, time, urllib.request

BASE = "https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_{m}.parquet"

DDL = """
DROP TABLE IF EXISTS taxi_trips;
CREATE TABLE taxi_trips (
    trip_id    int8 PRIMARY KEY,
    vendor     int2 NOT NULL,
    pickup     timestamp NOT NULL,
    dropoff    timestamp NOT NULL,
    passengers int2,
    distance   float8 NOT NULL,
    ratecode   int2,
    flag       text,
    pu         int4 NOT NULL,
    do_        int4 NOT NULL,
    payment    int2 NOT NULL,
    fare       float8 NOT NULL,
    extra      float8 NOT NULL,
    tip        float8 NOT NULL,
    tolls      float8 NOT NULL,
    total      float8 NOT NULL,
    congestion float8
);
"""
SRC = ["VendorID", "tpep_pickup_datetime", "tpep_dropoff_datetime", "passenger_count", "trip_distance", "RatecodeID",
       "store_and_fwd_flag", "PULocationID", "DOLocationID", "payment_type", "fare_amount", "extra", "tip_amount",
       "tolls_amount", "total_amount", "congestion_surcharge"]


def esc(v):
    if v is None:
        return "\\N"
    if isinstance(v, float):
        if v != v:
            return "\\N"
        return repr(v)
    if isinstance(v, str):
        return v.replace("\\", "\\\\").replace("\t", " ").replace("\n", " ")
    if hasattr(v, "isoformat"):
        return v.isoformat(sep=" ")
    return str(int(v)) if isinstance(v, (bool, int)) else str(v)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", required=True)
    ap.add_argument("--months", default="2024-01,2024-02,2024-03,2024-04")
    ap.add_argument("--keep-files", default="/tmp/taxi")
    ap.add_argument("--rows-file", default="/root/taxi_rows")
    a = ap.parse_args()
    import psycopg
    import pyarrow.parquet as pq
    os.makedirs(a.keep_files, exist_ok=True)
    conn = psycopg.connect(a.url, autocommit=True)
    conn.execute(DDL)
    n = 0
    for m in a.months.split(","):
        path = os.path.join(a.keep_files, f"yellow_{m}.parquet")
        if not os.path.exists(path):
            t = time.time()
            urllib.request.urlretrieve(BASE.format(m=m), path)
            print(f"{m}: downloaded {os.path.getsize(path) / 1e6:.0f} MB in {time.time() - t:.0f}s", file=sys.stderr)
        pf = pq.ParquetFile(path)
        t = time.time()
        with conn.cursor() as cur, cur.copy("COPY taxi_trips (trip_id, vendor, pickup, dropoff, passengers, distance, ratecode, flag, pu, do_, payment, fare, extra, tip, tolls, total, congestion) FROM STDIN") as cp:
            for batch in pf.iter_batches(batch_size=50000, columns=SRC):
                cols = [batch.column(i).to_pylist() for i in range(len(SRC))]
                lines = []
                for r in range(batch.num_rows):
                    n += 1
                    lines.append("\t".join([str(n)] + [esc(c[r]) for c in cols]) + "\n")
                cp.write("".join(lines))
        print(f"{m}: {pf.metadata.num_rows} rows loaded in {time.time() - t:.0f}s (total {n})", file=sys.stderr)
    conn.execute("CREATE INDEX taxi_trips_pu ON taxi_trips (pu)")
    conn.execute("ANALYZE taxi_trips")
    cur = conn.execute("SELECT count(*), min(pickup), max(pickup), count(passengers), count(flag), pg_size_pretty(pg_total_relation_size('taxi_trips')) FROM taxi_trips")
    print("taxi_trips:", cur.fetchone(), file=sys.stderr)
    open(a.rows_file, "w").write(str(n) + "\n")


if __name__ == "__main__":
    main()
