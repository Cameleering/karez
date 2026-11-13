# karez-rt: design

_2026-11-13. Design only: nothing in this note is built. Status words: **planned** or **design
target**. Owner decision (2026-11-13): a small design with no async runtime, informed by simoom._

Sources. Our code: the blocking client in `crates/karez-pg` (the sans-io protocol in
`crates/karez-proto-pg`, 108 tests, real-server runs on PostgreSQL 14.25, 16.15, 17.12 `[measured
here]`, 2026-11-13). The plan: [plan.md](plan.md) sections 1.2, 2.3, 6. simoom's lessons:
[../research/simoom-io-learnings.md](../research/simoom-io-learnings.md), `[reported by the simoom
session, not checked, 2026-11-13]`. No number below is a karez measurement.

## 1. Goal and non-goals

Goal: let many threads, or many futures on any executor, share one PostgreSQL connection with
**pipelining**, using only `std`, with no thread on an idle connection, bounded memory, and an
error for every limit. The blocking one-caller path keeps its current shape: the caller's thread
drives the socket, with no hand-off (plan.md 2.3).

Non-goals: an event loop, `epoll`/`IOCP`, `io_uring`, a runtime, MySQL and TDS (their protocols
do not pipeline; they reuse the worker without pipelining), latency-based load shedding.

## 2. Why threads, not an event loop

- A database client holds tens of connections, not tens of thousands. simoom's own data, `[reported
  by the simoom session]`: a threaded server beat its event-loop server on connect-per-request
  (+9%) and on memory at rest; the event loop won on CPU per request (-33%) and on memory per
  idle connection (5.8x). The second pair matters at thousands of connections, which a pool never has.
- `std` has no `poll`/`select`, and the core has no `unsafe` and no dependencies, so a
  readiness loop cannot be written in the core. A blocking read on a thread is the only way.
- The sans-io split is kept: the protocol crates see bytes and return events and bytes.

## 3. Layers

```text
karez-proto-pg   bytes in, messages out; encoders; SCRAM; ResultPlan          (exists)
karez-pg         Engine: sans-io session (statement cache, command FIFO,      (to extract)
                 decoding into chunks). Connection = Engine + blocking socket (exists, refactored)
karez-rt         Shared connection: pump thread, slots, futures, pipelining   (planned)
karez-pool       fair pool over Connection and Shared connection              (planned)
```

**Phase 0 refactor.** Today `Connection` mixes protocol logic and blocking reads (`Io::next_msg`
loops on the socket). Extract an `Engine` that never touches a socket:

- `engine.submit(Command) -> CommandId` queues a command and appends its bytes to `engine.out`.
- `engine.feed(&[u8])` takes bytes from the server; `engine.poll() -> Option<Event>` returns
  `Event::Row/Chunk/Done/Error(CommandId, ..)`, `Notice`, `Notification`.
- `engine.out()` is the pending write buffer. All queued commands are in it: one `write`, one
  TCP segment, one TLS record (plan.md 2.3 "write coalescing").
- Behaviour must not change: all 108 tests stay green, and the existing split-invariance tests
  are extended to the engine (same server bytes, cut anywhere, same events).

## 4. Threading: no thread while idle

Copied from simoom's `mux.rs` `[reported]`:

| in flight | who reads the socket |
|---|---|
| 0 | nobody; no thread exists |
| 1, caller blocks | the calling thread (leader), exactly as `Connection` does today |
| 2 or more, or any async caller | a **pump thread**, started on demand; it exits when in flight reaches 0 |

State is one `Mutex<Shared>` (the engine plus the socket's write half) held only briefly, and one
**slot** per command: a small `Mutex` + `Condvar` + `Waker`. The reader never holds the lock while
blocked in `read`: it reads into its own buffer, then locks, feeds the engine, drains events into
slots and unlocks. A connection that listens (`LISTEN`) keeps its pump until closed.

**Why a reader thread at all.** Writing a large batch while nobody reads can deadlock: the server
blocks writing results into a full socket buffer, we block writing the rest of the batch. With a
pump thread reading, a blocked writer cannot stall the reader. The caller who writes uses the
socket's write half (`try_clone`); the pump owns the read half.

> **Step 0a, done in the code (2026-11-13).** `Connection::execute_many` used to write its whole
> batch before it read anything, which can deadlock when both socket buffers fill. A batch above
> 64 KiB is now written by a scoped helper thread while the caller reads. The hang itself could not
> be reproduced on the Windows development machine (see the results note), so the fix is by
> reasoning; verify it on Linux. The pump thread of step 1 makes the helper thread unnecessary.

## 5. Commands, pipelining and ordering

PostgreSQL answers commands in the order sent; an error skips to the next `Sync`
(plan.md 2.3).

- **Implicit pipelining:** each command from each thread is its own Bind/Execute/**Sync** group, so
  one failure affects only that command (inside an explicit transaction it still aborts the
  transaction, as PostgreSQL does). Everything queued in one turn goes out in one `write`.
- **Explicit `Pipeline`:** one `Sync` for the batch, so it is atomic (this is `execute_many`
  today, generalised to any statements).
- **Three result classes**, chosen by the caller:
  - `Exec`: only the row count is kept.
  - `Collect`: chunks are buffered, up to `max_collect_bytes` (default 64 MiB), then the command
    fails with an error and its rest is discarded (the connection stays in sync).
  - `Stream`: chunks are delivered as they arrive, in a queue of at most 4 chunks. When it is full
    the reader stops reading the socket. Because the protocol is in order, **a slow stream stalls
    every command behind it on that connection**. Therefore a stream **leases the connection**:
    commands submitted meanwhile wait in the submit queue (bounded, section 7). This is the
    price of in-order delivery and it is stated in the docs.
- Deadlock rule: a thread must not wait on a later command while it holds an unread full stream
  from an earlier one. The API makes `Stream` borrow the handle, so the compiler enforces it.

## 6. Failure, cancellation, replay

- **Not sent versus sent.** Every error carries `retry_safe()`: true when the command never
  reached the socket (still queued, or the connection was already down), false when it was written
  and the outcome is unknown. A dead connection fails all in-flight commands with
  `Error::Closed { retry_safe }` each.
- **Replay (owner decision 2026-11-13):** the pool moves a command to a new connection only if it
  was **never written** to the dead one (`retry_safe()` is true). A command that was written is
  never resent automatically, because a written `INSERT` may have committed; the caller gets the
  error with `retry_safe() == false` and decides. simoom also resends unanswered idempotent
  requests `[reported]`; karez does not, for that reason.
- **Dropping a future or handle** cannot desynchronise the stream, because the engine owns the
  socket state. A command still queued is removed and never runs. One in flight has its results
  discarded; it is cancelled with the `CancelToken` only if `cancel_on_drop` is set (default off),
  and only while it is the head of the queue, because a late cancel can hit the next command.
- **Deadlines:** each command may carry a deadline. Expiry cancels it if it is the head, fails it
  with a timeout error, and drains it. A connection that cannot be drained within the grace period
  is closed.
- **Close:** `close(grace)` stops accepting commands, waits for those in flight, sends Terminate,
  half-closes the write side and reads to EOF (bounded) before closing, so the server never sees a
  reset because of unread input (simoom bug 20 `[reported]`). `Drop` does the same without waiting.

## 7. Limits (each has a default; none is unbounded)

| limit | default | when exceeded |
|---|---|---|
| submit queue, commands | 1,024 | `Error::QueueFull` at once (`try_submit`), or wait up to a timeout |
| submit queue, bytes | 16 MiB | same |
| message size | 64 MiB (exists) | protocol error, connection closed |
| `Collect` buffer | 64 MiB | that command fails, connection stays usable |
| `Stream` queue | 4 chunks | reader pauses (backpressure) |
| statement cache | 256 (exists) | LRU eviction |
| close grace | 5 s | socket closed |
| connect, read timeouts | exist | I/O error |

Admission is by queue size only. A latency-based shed is **not** built: it shed falsely at 50%
load in simoom and never helped overload `[reported]`.

## 8. Async without a runtime

`std::future::Future` and `std::task::Waker` need no runtime. A command's slot holds a `Waker`;
the pump completes the slot and calls `wake()`. `submit_async(cmd)` returns a `CommandFuture`,
`Stream::next_chunk()` returns a `NextChunk` future. They work on tokio, smol, async-std or a
hand-written `block_on`. A pump thread serves every async caller (section 4), so an async caller
never blocks an executor thread on I/O.

DNS and connect for async callers run on a short-lived connector thread, because the system
resolver blocks (simoom `rt.rs` `[reported]`); a blocking caller resolves on its own thread.

## 9. Pool (`karez-pool`)

FIFO and fair: waiters are served in arrival order, with a bounded wait and an error on timeout.
The `Pool` now in `karez-pg` is not fair (a `Condvar`) and is the starting point. Two modes:
exclusive checkout (the blocking default, no hand-off) and shared connections (pipelined, the
planned default for async). A pooled connection returned inside a transaction is rolled back (exists).

## 10. TLS

The planned `rustls` feature fits the Engine: `rustls::ClientConnection` is itself sans-io (bytes
in, bytes out), so the Engine's `feed` and `out` wrap it and nothing else changes. Decided
2026-11-13: an optional feature, after an issue discusses the dependency.

## 11. Tests

- Engine: split invariance for events; scripted-server tests for each failure in section 6.
- Mock server with **delayed reads** to prove there is no write/read deadlock (the Phase 0 issue
  gets a regression test first).
- Real server, 14.25, 16.15, 17.12: 8 threads and 8 futures on one connection; pipelined
  transactions; backend killed mid-pipeline (`retry_safe` is right for every command); future
  dropped at every state; slow stream blocks others as documented; cancel and deadline races;
  `LISTEN` while pipelining; close with unread input and the server sees EOF, not a reset.
- Every test keeps the loud skip rule (`KAREZ_REQUIRE_SERVER=1`).

## 12. Measuring it (before any claim)

Per plan.md section 6 and simoom's method `[reported]`: same machine, A/B, **open-loop p99**
(coordinated omission corrected), pipelined and unpipelined reported separately, losses published.
Baselines: tokio-postgres per-core pipelined, zero-postgres, wtx, pgJDBC tuned, asyncpg, libpq.
Run on Linux VMs: simoom's Windows loopback experiment gave no usable signal `[reported]`, so the
go/no-go does not run on Windows. Pin loop and pump threads away from the NIC's receive CPU and
record the VM's CPU and NIC topology with every run. Targets are the plan's (S2 within 5% of the
fastest; pipelined within 5% of the best pipelined client); they are targets, not results.

## 13. Build order and exit criteria

Status as built on 2026-11-13 (branch `claude/m0-spike-1bacb7`). Deviations from the design above are
listed under the table.

| step | what | state |
|---|---|---|
| 0a | fix the `execute_many` write/read deadlock | **done**; not yet verified on Linux (the hang never reproduced on Windows) |
| 0b | sans-io `Engine` | **done** in `karez-rt` (6 tests with scripted bytes, split invariance, malformed input); `Connection` is **not** rebuilt on it |
| 1 | shared connection, pump, slots, limits, close | **done** (11 real-server tests on 14.25, 16.15, 17.12) |
| 2 | implicit pipelining, `Pipeline`, stream lease, `retry_safe` | **done**, with the deviations below |
| 3 | `Future` API | **done**, tested on a hand-written `block_on`; **not** yet tested on tokio |
| 4 | fair pool | **done** in `karez-pg` (first come, first served), not in a separate `karez-pool` crate; no pool of shared connections yet |
| 5 | benchmark harness and baselines | **not done**; only a smoke run (see the results note) |

Deviations and gaps:

- `Connection` keeps its own blocking protocol code, so prepare, decode and error handling now exist twice
  (`karez-pg` and the engine). Rebuilding `Connection` on the engine is optional follow-up; the tests of both
  protect them.
- The pump thread serves **every** command, including a lone blocking caller. The leader optimisation (the
  caller reads the socket itself when one command is in flight) is not built. In the smoke run one-at-a-time
  through `SharedConnection` is slower than through `Connection` (see the results note).
- A dropped `Pending` or `Stream` discards its answer when it arrives, but a command still queued is **not**
  removed: it runs. The design said it would be removed.
- `LISTEN` notifications are read only while commands are in flight; an idle shared connection has no reader.
- Statement-cache eviction and duplicate prepares queue a Close; a stale plan (0A000, 26000) drops the entry and
  the error is returned (no retry).
- The first use of a statement blocks the caller for one round trip, async callers included.
- `Pipeline` takes owned values of the nine basic types only (`OwnedValue`).

## 14. Risks and open questions

- **No `poll` in `std`:** a pump thread blocks in `read`. To stop it, another thread calls
  `TcpStream::shutdown`; the cost is one thread per busy connection. Acceptable at pool sizes of
  tens; revisit only if measurements say otherwise.
- **Head-of-line blocking** behind a slow `Stream` is inherent to in-order protocols; the lease
  makes it explicit (decided 2026-11-13: lease and document, no automatic second connection).
- **Replay** is limited to commands never written (decided); anything else is the caller's call.
- **Windows:** thread and socket behaviour differ (timeouts report `TimedOut`, not `WouldBlock`;
  already handled in `wait_for_notification`). Test on both.
