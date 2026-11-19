# What simoom learnt that bears on `karez-rt`

_Reported 2026-11-13 by the simoom session (second-hand: I did not read the simoom files myself;
the paths below are the ones it named, in the simoom repository). Check each before relying on
it. Numbers are simoom's, from its own benchmarks on DigitalOcean c-4 VMs, and are not karez
measurements._

## Answer in one line

For a database client, thread per connection with sans-io protocol crates is the right model.
simoom's own client (`crates/simoom-client`) is the closer analogue to karez than its server.

## Facts reported

- **Server:** one event loop per core; completion-style interface; epoll default on Linux, io_uring
  opt-in (it did not beat epoll by the 10% the plan required), IOCP on Windows, kqueue on macOS.
  See simoom `docs/design/plan.md` sections 1, 2.2 and 2.3.
- **Client:** blocking client in `client.rs`, `pool.rs`, `transport.rs`. Multiplexing in `mux.rs`: the
  protocol engine sits under a briefly held lock with one slot and condvar per stream. The waiting
  thread reads the socket itself while one stream is open; from two streams up a pump thread reads
  for everyone and exits when idle, so an idle connection has no thread. Pipelining in
  `pipeline.rs` is opt-in, idempotent requests only, and resends unanswered requests on a new
  connection. DNS and connect run on a separate connector thread because the system resolver blocks.
- **Threads vs event loop (campaign 4, `bench/results/2026-11-12-campaign4/`):** the event-loop
  server was ahead by 22 to 28% in requests per second and used 33% less CPU per request and 5.8x
  less memory per idle connection; the threaded server was ahead by 9% on a new connection per
  request and used less memory at rest.
- **Surprises:** the VM's single receive queue capped every server at 160 to 180k requests per
  second; latency-based load shedding shed falsely and shipped off; close with unread input made
  the peer see a TCP reset (drain first); timers of closed connections must be cancelled; a handler
  wrongly treated as fast stalls a loop.
- **Not claimed by simoom:** the 2x gate was never shown (NIC ceiling); no numbers for larger
  machines or multi-queue NICs.

## What this means for karez (recommendations, not decisions)

Copy: sans-io protocol crates (already so in karez); no thread while idle, a pump thread only with
two or more requests in flight; pipelining only for requests that are safe to replay (for
PostgreSQL, only outside a transaction, and karez must carry that state itself); bounded queues
with a fast error; a default for every limit.

Avoid: latency-based shedding; DNS on the I/O thread; closing a socket with unread input
undrained; benchmarks that assume a VM's CPU and NIC topology.

Measure as the plan says: same machine, A/B, open-loop p99, publish the losses.
