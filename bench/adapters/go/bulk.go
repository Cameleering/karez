package main

// Bulk scenarios (S4 scan, S5 batch insert, S6 COPY) of the pgx adapter. Closed loop: one untimed
// warm-up pass, then `repeats` timed passes. See bench/rustbench/src/bulk.rs for the checksum of S4
// and the row recipe of S5 and S6.

import (
	"context"
	"encoding/json"
	"fmt"
	"math"
	"math/big"
	"os"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

const (
	s4SQL = "SELECT id, a, b, c, d, e, s1, s2, flag, f FROM bench_wide"
	cols  = "id, a, b, c, d, e, s1, s2, flag, f"
	insQL = "INSERT INTO bench_ins (" + cols + ") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
)

var mul, _ = new(big.Int).SetString("9E3779B97F4A7C15F39CC0605CEDC835", 16)
var m128 = new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), 128), big.NewInt(1))

func s2hex(i int64) string {
	v := new(big.Int).Mul(big.NewInt(i), mul)
	v.And(v, m128)
	return fmt.Sprintf("%032x", v)
}

func genRow(i int64) []any {
	return []any{i, int32(i % 100000), int32((i * 17) % 65536), i * 10007, float64(i) / 3.0, float64(i%1000) * 0.25,
		fmt.Sprintf("row-%d", i), s2hex(i), i%2 == 0, int16(i % 1000)}
}

func cpuNow() float64 {
	u, s, _ := procStats()
	return u + s
}

type pass struct {
	Secs     float64 `json:"secs"`
	CPU      float64 `json:"cpu_s"`
	Rows     uint64  `json:"rows"`
	Checksum uint64  `json:"checksum"`
}

func bulk(url, scenario string, repeats int, n int64) {
	ctx := context.Background()
	conn, err := pgx.Connect(ctx, url)
	if err != nil {
		die("connect: %v", err)
	}
	defer conn.Close(ctx)
	count := func() uint64 {
		var c int64
		if err := conn.QueryRow(ctx, "SELECT count(*) FROM bench_ins").Scan(&c); err != nil {
			die("count: %v", err)
		}
		return uint64(c)
	}
	var fn func(rows int64) (uint64, uint64)
	switch scenario {
	case "s4":
		fn = func(int64) (uint64, uint64) {
			rows, err := conn.Query(ctx, s4SQL)
			if err != nil {
				die("query: %v", err)
			}
			var r, sum uint64
			for rows.Next() {
				var id, c int64
				var a, b int32
				var d, e float64
				var s1, s2 string
				var flag bool
				var f int16
				if err := rows.Scan(&id, &a, &b, &c, &d, &e, &s1, &s2, &flag, &f); err != nil {
					die("scan: %v", err)
				}
				sum += uint64(id) + uint64(a) + uint64(b) + uint64(c) + math.Float64bits(d) + math.Float64bits(e) +
					uint64(len(s1)) + uint64(len(s2)) + b2u(flag) + uint64(f)
				r++
			}
			if rows.Err() != nil {
				die("rows: %v", rows.Err())
			}
			return r, sum
		}
	case "s5":
		fn = func(rows int64) (uint64, uint64) {
			if _, err := conn.Exec(ctx, "TRUNCATE bench_ins"); err != nil {
				die("truncate: %v", err)
			}
			tx, err := conn.Begin(ctx)
			if err != nil {
				die("begin: %v", err)
			}
			for start := int64(1); start <= rows; start += 1000 {
				b := &pgx.Batch{}
				for i := start; i < start+1000 && i <= rows; i++ {
					b.Queue(insQL, genRow(i)...)
				}
				br := tx.SendBatch(ctx, b)
				for i := 0; i < b.Len(); i++ {
					if _, err := br.Exec(); err != nil {
						die("batch: %v", err)
					}
				}
				if err := br.Close(); err != nil {
					die("batch close: %v", err)
				}
			}
			if err := tx.Commit(ctx); err != nil {
				die("commit: %v", err)
			}
			return count(), 0
		}
	case "s8":
		fn = func(int64) (uint64, uint64) {
			rows, err := conn.Query(ctx, "SELECT "+taxiCols+" FROM taxi_trips")
			if err != nil {
				die("query: %v", err)
			}
			var n, sum uint64
			for rows.Next() {
				var id int64
				var vendor, payment int16
				var pickup, dropoff time.Time
				var passengers, ratecode *int16
				var distance, fare, extra, tip, tolls, total float64
				var flag *string
				var pu, do int32
				var congestion *float64
				if err := rows.Scan(&id, &vendor, &pickup, &dropoff, &passengers, &distance, &ratecode, &flag, &pu, &do, &payment,
					&fare, &extra, &tip, &tolls, &total, &congestion); err != nil {
					die("scan: %v", err)
				}
				sum += uint64(id) + uint64(int64(vendor)) + uint64(pickup.UnixMicro()) + uint64(dropoff.UnixMicro()) + i16(passengers) +
					math.Float64bits(distance) + i16(ratecode)
				if flag != nil {
					sum += uint64(len(*flag))
				}
				sum += uint64(int64(pu)) + uint64(int64(do)) + uint64(int64(payment)) + math.Float64bits(fare) + math.Float64bits(extra) +
					math.Float64bits(tip) + math.Float64bits(tolls) + math.Float64bits(total)
				if congestion != nil {
					sum += math.Float64bits(*congestion)
				}
				n++
			}
			if rows.Err() != nil {
				die("rows: %v", rows.Err())
			}
			return n, sum
		}
	default: // s6: binary COPY through CopyFrom, pgx's documented fast path
		fn = func(rows int64) (uint64, uint64) {
			if _, err := conn.Exec(ctx, "TRUNCATE bench_ins"); err != nil {
				die("truncate: %v", err)
			}
			var i int64
			src := pgx.CopyFromFunc(func() ([]any, error) {
				i++
				if i > rows {
					return nil, nil
				}
				return genRow(i), nil
			})
			if _, err := conn.CopyFrom(ctx, pgx.Identifier{"bench_ins"}, strings.Split(cols, ", "), src); err != nil {
				die("copy: %v", err)
			}
			return count(), 0
		}
	}
	warm := n
	if scenario != "s4" && scenario != "s8" {
		warm = 1000
	}
	if scenario != "s8" {
		fn(warm)
	}
	var runs []pass
	for k := 0; k < repeats; k++ {
		c0, t0 := cpuNow(), time.Now()
		r, s := fn(n)
		runs = append(runs, pass{Secs: time.Since(t0).Seconds(), CPU: cpuNow() - c0, Rows: r, Checksum: s})
	}
	_, _, rss := procStats()
	out, _ := json.Marshal(map[string]any{"harness": "goadapter", "bulk": true, "client": "pgx", "version": "pgx v5",
		"scenario": strings.ToUpper(scenario), "rows_target": n, "runs": runs, "rss_kb": rss})
	fmt.Println(string(out))
	os.Exit(0)
}
