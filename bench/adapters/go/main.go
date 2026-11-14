// Go adapter (pgx v5) of the karez benchmark. Same flags and output as rustbench (bench/README.md).
//
// Adapter rules: connections open and statements prepared before the clock starts; every column of
// every row is read in its native Go type and folded into the checksum; row count (and for S2 the
// id) is checked; latency is measured from the intended start.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"math/bits"
	"os"
	"runtime"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

var sqlOf = map[string]string{
	"s1": "SELECT 1",
	"s2": "SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = $1",
	"s3": "SELECT id, message FROM bench_fortune",
}
var rowsOf = map[string]int{"s1": 1, "s2": 1, "s3": 12}

const buckets = 64 * 60

func hidx(v uint64) int {
	if v < 128 {
		return int(v)
	}
	msb := uint64(63 - bits.LeadingZeros64(v))
	shift := msb - 6
	return int((shift << 6) + (v >> shift))
}

type shared struct {
	t0                  time.Time
	warmNs, endNs       uint64
	hist, svc           [buckets]atomic.Uint64
	ok, errs, dropped   atomic.Uint64
	sent, checksum      atomic.Uint64
	firstErr            atomic.Value
	scenario            string
	rows                int64
	rate                float64
	conns, pool         int
	perConn             bool
	url                 string
}

func (s *shared) now() uint64 { return uint64(time.Since(s.t0).Nanoseconds()) }

func (s *shared) record(intended, start, end uint64, err error, sum uint64) {
	if intended < s.warmNs || intended >= s.endNs {
		return
	}
	if err != nil {
		s.errs.Add(1)
		s.firstErr.CompareAndSwap(nil, err.Error())
		return
	}
	i, j := hidx(end-min(end, intended)), hidx(end-min(end, start))
	if i >= buckets {
		i = buckets - 1
	}
	if j >= buckets {
		j = buckets - 1
	}
	s.hist[i].Add(1)
	s.svc[j].Add(1)
	s.ok.Add(1)
	s.checksum.Add(sum)
}

func (s *shared) drop(intended uint64) {
	if intended >= s.warmNs && intended < s.endNs {
		s.dropped.Add(1)
	}
}

type rng uint64

func (r *rng) next(n int64) int64 {
	x := uint64(*r)
	x ^= x >> 12
	x ^= x << 25
	x ^= x >> 27
	*r = rng(x)
	return int64((x*0x2545F4914F6CDD1D)>>11)%n + 1
}

// foldRows reads every column in its native type.
func fold(s *shared, rows pgx.Rows, want int64) (uint64, error) {
	n := 0
	var sum uint64
	for rows.Next() {
		n++
		switch s.scenario {
		case "s1":
			var v int32
			if err := rows.Scan(&v); err != nil {
				return 0, err
			}
			sum += uint64(v)
		case "s2":
			var id, ref, big int64
			var k1 int32
			var k2 int16
			var flag bool
			var score float64
			var name, note, tag string
			if err := rows.Scan(&id, &k1, &k2, &flag, &score, &name, &note, &ref, &tag, &big); err != nil {
				return 0, err
			}
			if id != want {
				return 0, fmt.Errorf("S2 returned id %d, wanted %d", id, want)
			}
			sum += uint64(id) + uint64(k1) + uint64(k2) + b2u(flag) + uint64(int64(score)) + uint64(len(name)) +
				uint64(len(note)) + uint64(ref) + uint64(len(tag)) + uint64(big)
		default:
			var id int32
			var msg string
			if err := rows.Scan(&id, &msg); err != nil {
				return 0, err
			}
			sum += uint64(id) + uint64(len(msg))
		}
	}
	if err := rows.Err(); err != nil {
		return 0, err
	}
	if n != rowsOf[s.scenario] {
		return 0, fmt.Errorf("%d rows, wanted %d", n, rowsOf[s.scenario])
	}
	return sum, nil
}

func b2u(b bool) uint64 {
	if b {
		return 1
	}
	return 0
}

func procStats() (user, sys float64, rssKB uint64) {
	if b, err := os.ReadFile("/proc/self/stat"); err == nil {
		s := string(b)
		if i := strings.LastIndex(s, ")"); i >= 0 {
			f := strings.Fields(s[i+1:])
			if len(f) > 12 {
				var u, st float64
				fmt.Sscan(f[11], &u)
				fmt.Sscan(f[12], &st)
				user, sys = u/100, st/100
			}
		}
	}
	if b, err := os.ReadFile("/proc/self/status"); err == nil {
		for _, l := range strings.Split(string(b), "\n") {
			if strings.HasPrefix(l, "VmHWM:") {
				fmt.Sscan(strings.Fields(l)[1], &rssKB)
			}
		}
	}
	return
}

func die(format string, a ...any) {
	fmt.Fprintf(os.Stderr, "goadapter: "+format+"\n", a...)
	os.Exit(2)
}

func main() {
	client := flag.String("client", "pgx", "")
	url := flag.String("url", "", "")
	scenario := flag.String("scenario", "s1", "")
	mode := flag.String("mode", "unpipelined", "")
	conns := flag.Int("conns", 1, "")
	pool := flag.Int("pool", 0, "")
	rate := flag.Float64("rate", 1000, "")
	duration := flag.Float64("duration", 10, "")
	warmup := flag.Float64("warmup", 2, "")
	dispatch := flag.String("dispatch", "shared", "")
	rows := flag.Int64("rows", 100000, "")
	repeats := flag.Int("repeats", 5, "")
	bulkRows := flag.Int64("bulk-rows", 100000, "")
	_ = flag.Int("rt-threads", 4, "")
	_ = flag.String("root-cert", "", "")
	flag.Parse()
	if *mode == "pipelined" {
		die("pgx: unpipelined only")
	}
	if *client != "pgx" {
		die("unknown client %s (pgx)", *client)
	}
	if *scenario == "s4" || *scenario == "s5" || *scenario == "s6" {
		bulk(*url, *scenario, *repeats, *bulkRows)
	}
	sh := &shared{
		warmNs: uint64(*warmup * 1e9), endNs: uint64((*warmup + *duration) * 1e9), scenario: *scenario,
		rows: *rows, rate: *rate, conns: *conns, pool: *pool, perConn: *dispatch == "perconn", url: *url,
	}
	ctx := context.Background()
	sql := sqlOf[*scenario]
	query := func(q interface {
		Query(context.Context, string, ...any) (pgx.Rows, error)
	}, id int64) (pgx.Rows, error) {
		if *scenario == "s2" {
			return q.Query(ctx, sql, id)
		}
		return q.Query(ctx, sql)
	}
	exec := func(q interface {
		Query(context.Context, string, ...any) (pgx.Rows, error)
	}, id int64) (uint64, error) {
		r, err := query(q, id)
		if err != nil {
			return 0, err
		}
		defer r.Close()
		return fold(sh, r, id)
	}

	// workers
	var runOne []func(id int64) (uint64, error)
	var closers []func()
	if *pool > 0 {
		p, err := pgxpool.New(ctx, *url+poolParam(*url, *pool))
		if err != nil {
			die("pool: %v", err)
		}
		// open and warm every pool connection before the clock starts
		held := make([]*pgxpool.Conn, 0, *pool)
		for i := 0; i < *pool; i++ {
			c, err := p.Acquire(ctx)
			if err != nil {
				die("pool: %v", err)
			}
			if _, err := exec(c, 1); err != nil {
				die("warm: %v", err)
			}
			held = append(held, c)
		}
		for _, c := range held {
			c.Release()
		}
		for i := 0; i < *conns; i++ {
			runOne = append(runOne, func(id int64) (uint64, error) { return exec(p, id) })
		}
		closers = append(closers, p.Close)
	} else {
		for i := 0; i < *conns; i++ {
			c, err := pgx.Connect(ctx, *url)
			if err != nil {
				die("connect: %v", err)
			}
			if _, err := exec(c, 1); err != nil {
				die("warm: %v", err)
			}
			cc := c
			runOne = append(runOne, func(id int64) (uint64, error) { return exec(cc, id) })
			closers = append(closers, func() { cc.Close(ctx) })
		}
	}

	nq := 1
	if sh.perConn {
		nq = len(runOne)
	}
	qs := make([]chan uint64, nq)
	for i := range qs {
		qs[i] = make(chan uint64, 1<<20)
	}
	sh.t0 = time.Now()
	fmt.Fprintln(os.Stderr, "T0")
	var wg sync.WaitGroup
	for w, run := range runOne {
		wg.Add(1)
		go func(w int, run func(int64) (uint64, error)) {
			defer wg.Done()
			r := rng(uint64(w+1) * 0x9E3779B97F4A7C15 | 1)
			for {
				var intended uint64
				if sh.rate == 0 {
					intended = sh.now()
					if intended >= sh.endNs {
						return
					}
				} else {
					v, ok := <-qs[w%nq]
					if !ok {
						return
					}
					intended = v
				}
				start := sh.now()
				if start > sh.endNs+5_000_000_000 {
					sh.drop(intended)
					continue
				}
				var id int64
				if sh.scenario == "s2" {
					id = r.next(sh.rows)
				}
				sum, err := run(id)
				sh.record(intended, start, sh.now(), err, sum)
			}
		}(w, run)
	}
	if sh.rate == 0 {
		time.Sleep(time.Duration(sh.endNs-sh.now()) * time.Nanosecond)
	} else {
		period := 1e9 / sh.rate
		for k := uint64(0); ; k++ {
			intended := uint64(float64(k) * period)
			if intended >= sh.endNs {
				break
			}
			if now := sh.now(); intended > now {
				time.Sleep(time.Duration(intended-now) * time.Nanosecond)
			}
			if intended >= sh.warmNs {
				sh.sent.Add(1)
			}
			qs[int(k)%nq] <- intended
		}
		for _, q := range qs {
			close(q)
		}
	}
	wg.Wait()
	wall := time.Since(sh.t0).Seconds()
	for _, c := range closers {
		c()
	}
	user, sys, rss := procStats()
	pairs := func(h *[buckets]atomic.Uint64) [][2]uint64 {
		var o [][2]uint64
		for i := range h {
			if c := h[i].Load(); c > 0 {
				o = append(o, [2]uint64{uint64(i), c})
			}
		}
		sort.Slice(o, func(a, b int) bool { return o[a][0] < o[b][0] })
		return o
	}
	fe, _ := sh.firstErr.Load().(string)
	var feJSON any
	if fe != "" {
		feJSON = fe
	}
	out, _ := json.Marshal(map[string]any{
		"harness": "goadapter", "client": "pgx", "version": "pgx v5 (" + runtime.Version() + ")",
		"scenario": strings.ToUpper(*scenario), "mode": "Unpipelined", "conns": *conns, "pool": *pool,
		"rate": *rate, "duration_s": *duration, "warmup_s": *warmup, "dispatch": *dispatch,
		"rt_threads": runtime.GOMAXPROCS(0), "sent": sh.sent.Load(), "ok": sh.ok.Load(), "errors": sh.errs.Load(),
		"dropped": sh.dropped.Load(), "checksum": sh.checksum.Load(), "first_error": feJSON, "wall_s": wall,
		"cpu_user_s": user, "cpu_sys_s": sys, "rss_kb": rss, "hist": pairs(&sh.hist), "service_hist": pairs(&sh.svc),
	})
	fmt.Println(string(out))
}

func poolParam(url string, n int) string {
	sep := "?"
	if strings.Contains(url, "?") {
		sep = "&"
	}
	return fmt.Sprintf("%spool_max_conns=%d&pool_min_conns=%d", sep, n, n)
}
