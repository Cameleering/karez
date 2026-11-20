//! The sans-io engine, driven by scripted server bytes (no socket, no server).

use karez_chunk::rng::SplitMix64;
use karez_proto_pg::backend::FieldDescription;
use karez_proto_pg::{ResultPlan, TxStatus, Value};
use karez_rt::engine::{Class, Done, Engine, Event, RawPrepared};
use karez_rt::Error;
use std::collections::HashMap;
use std::sync::Arc;

fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn engine() -> Engine {
    Engine::new(1 << 20, 8, 1 << 20, TxStatus::Idle, HashMap::new())
}

fn ready() -> Vec<u8> {
    msg(b'Z', b"I")
}

fn int_row(i: i32) -> Vec<u8> {
    let mut b = 1u16.to_be_bytes().to_vec();
    b.extend_from_slice(&4i32.to_be_bytes());
    b.extend_from_slice(&i.to_be_bytes());
    msg(b'D', &b)
}

fn desc_int() -> FieldDescription {
    FieldDescription {
        name: "n".into(),
        table_oid: 0,
        column_attr: 0,
        type_oid: 23,
        type_size: 4,
        type_modifier: -1,
        format: 1,
    }
}

/// Replies for Parse + Describe of a statement `SELECT n` with one int4 parameter and column.
fn prepare_reply() -> Vec<u8> {
    let mut v = msg(b'1', b"");
    v.extend(msg(b't', &[0, 1, 0, 0, 0, 23]));
    let mut t = 1u16.to_be_bytes().to_vec();
    t.extend_from_slice(b"n\0");
    t.extend_from_slice(&0u32.to_be_bytes());
    t.extend_from_slice(&0i16.to_be_bytes());
    t.extend_from_slice(&23u32.to_be_bytes());
    t.extend_from_slice(&4i16.to_be_bytes());
    t.extend_from_slice(&(-1i32).to_be_bytes());
    t.extend_from_slice(&1i16.to_be_bytes());
    v.extend(msg(b'T', &t));
    v.extend(ready());
    v
}

fn prepared(e: &mut Engine) -> Arc<karez_rt::engine::Prepared> {
    let id = e.prepare("SELECT $1::int", &[23]).unwrap();
    let _ = e.take_output();
    e.feed(&prepare_reply()).unwrap();
    let Some(Event::Done { id: got, result }) = e.poll_event() else {
        panic!("expected completion")
    };
    assert_eq!(got, id);
    let done: Done = result.unwrap();
    let raw: RawPrepared = done.prepared.unwrap();
    assert_eq!(raw.param_oids, vec![23]);
    let plan = ResultPlan::from_description(&raw.fields);
    assert_eq!(raw.fields.len(), 1);
    assert_eq!(raw.fields[0].type_oid, desc_int().type_oid);
    e.install("SELECT $1::int", &[23], raw, plan)
}

/// A server answer to one Bind/Execute/Sync of `rows` rows.
fn run_reply(rows: i32) -> Vec<u8> {
    let mut v = msg(b'2', b"");
    for i in 0..rows {
        v.extend(int_row(i));
    }
    v.extend(msg(b'C', format!("SELECT {rows}\0").as_bytes()));
    v.extend(ready());
    v
}

fn drain(e: &mut Engine) -> Vec<Event> {
    std::iter::from_fn(|| e.poll_event()).collect()
}

#[test]
fn prepare_and_collect_with_any_split_gives_the_same_events() {
    let reply = run_reply(2500);
    let mut results = Vec::new();
    let mut rng = SplitMix64::new(9);
    for mode in 0..4 {
        let mut e = engine();
        let stmt = prepared(&mut e);
        let id = e.run(&stmt, &[Value::i32(1)], Class::Collect).unwrap();
        let out = e.take_output();
        assert!(!out.is_empty());
        match mode {
            0 => e.feed(&reply).unwrap(),
            1 => reply.chunks(1).for_each(|b| e.feed(b).unwrap()),
            2 => reply.chunks(997).for_each(|b| e.feed(b).unwrap()),
            _ => {
                let mut at = 0;
                while at < reply.len() {
                    let n = rng.range(1, 5000).min(reply.len() - at);
                    e.feed(&reply[at..at + n]).unwrap();
                    at += n;
                }
            }
        }
        let evs = drain(&mut e);
        assert_eq!(evs.len(), 1);
        let Event::Done { id: got, result } = &evs[0] else {
            panic!()
        };
        assert_eq!(*got, id);
        let done = result.as_ref().unwrap();
        assert_eq!(done.rows, 2500);
        let sizes: Vec<usize> = done.chunks.iter().map(|c| c.rows()).collect();
        let sum: i64 = done
            .chunks
            .iter()
            .flat_map(|c| {
                (0..c.rows()).map(move |r| c.row(r).unwrap().get::<i32>(0).unwrap() as i64)
            })
            .sum();
        assert_eq!(sum, (0..2500i64).sum::<i64>());
        results.push((sizes, e.in_flight(), e.tx_status()));
    }
    assert!(results.windows(2).all(|w| w[0] == w[1]), "{results:?}");
    assert_eq!(results[0].0, vec![1024, 1024, 452]);
    assert_eq!(results[0].1, 0);
}

#[test]
fn stream_delivers_chunks_before_completion_and_exec_discards_rows() {
    let mut e = engine();
    let stmt = prepared(&mut e);
    let a = e.run(&stmt, &[Value::i32(1)], Class::Stream).unwrap();
    let b = e.run(&stmt, &[Value::i32(1)], Class::Exec).unwrap();
    let _ = e.take_output();
    let mut reply = run_reply(1500);
    reply.extend(run_reply(5000));
    e.feed(&reply).unwrap();
    let evs = drain(&mut e);
    let chunks: Vec<usize> = evs
        .iter()
        .filter_map(|v| match v {
            Event::Chunk { id, chunk } if *id == a => Some(chunk.rows()),
            _ => None,
        })
        .collect();
    assert_eq!(chunks, vec![1024, 476]);
    let done: Vec<_> = evs
        .iter()
        .filter_map(|v| match v {
            Event::Done { id, result } => Some((*id, result.as_ref().unwrap().rows)),
            _ => None,
        })
        .collect();
    assert_eq!(done, vec![(a, 1500), (b, 5000)]);
}

#[test]
fn an_error_in_a_group_skips_the_rest_and_later_commands_still_run() {
    let mut e = engine();
    let stmt = prepared(&mut e);
    let p = [Value::i32(1)];
    let ids = e
        .run_group(&[
            (&stmt, &p[..], Class::Exec),
            (&stmt, &p[..], Class::Exec),
            (&stmt, &p[..], Class::Exec),
        ])
        .unwrap();
    let after = e.run(&stmt, &p, Class::Exec).unwrap();
    let _ = e.take_output();
    // First member succeeds, the second fails (the server skips to Sync), then the next command.
    let mut reply = msg(b'2', b"");
    reply.extend(msg(b'C', b"SELECT 1\0"));
    reply.extend(msg(b'2', b""));
    reply.extend(msg(b'E', b"SERROR\0C22012\0Mdivision by zero\0\0"));
    reply.extend(ready());
    reply.extend(run_reply(0));
    e.feed(&reply).unwrap();
    let evs = drain(&mut e);
    let r: Vec<(u64, String)> = evs
        .into_iter()
        .map(|v| match v {
            Event::Done { id, result } => (
                id,
                match result {
                    Ok(d) => format!("ok {}", d.rows),
                    Err(Error::Skipped) => "skipped".into(),
                    Err(e) => format!("err {}", e.sqlstate().unwrap_or("?")),
                },
            ),
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        r,
        vec![
            (ids[0], "ok 1".to_string()),
            (ids[1], "err 22012".to_string()),
            (ids[2], "skipped".to_string()),
            (after, "ok 0".to_string()),
        ]
    );
    assert_eq!(e.in_flight(), 0);
}

#[test]
fn a_lost_connection_says_which_commands_were_never_sent() {
    let mut e = engine();
    let stmt = prepared(&mut e);
    let p = [Value::i32(1)];
    let sent = e.run(&stmt, &p, Class::Exec).unwrap();
    let _written = e.take_output();
    let queued = e.run(&stmt, &p, Class::Exec).unwrap();
    assert!(e.has_output());
    e.fail_all("socket closed");
    let evs = drain(&mut e);
    assert_eq!(evs.len(), 2);
    for v in evs {
        let Event::Done { id, result } = v else {
            panic!()
        };
        let err = result.unwrap_err();
        if id == sent {
            assert!(!err.retry_safe(), "{err}");
        } else {
            assert_eq!(id, queued);
            assert!(err.retry_safe(), "{err}");
        }
    }
    assert!(!e.has_output());
}

#[test]
fn bad_parameters_queue_nothing_and_a_stale_plan_is_dropped_from_the_cache() {
    let mut e = engine();
    let stmt = prepared(&mut e);
    assert!(matches!(
        e.run(&stmt, &[], Class::Exec),
        Err(Error::Pg(karez_pg::Error::Usage(_)))
    ));
    assert!(matches!(
        e.run(&stmt, &[Value::text("x")], Class::Exec),
        Err(Error::Pg(karez_pg::Error::Usage(_)))
    ));
    assert!(!e.has_output() && e.in_flight() == 0);
    assert!(e.lookup("SELECT $1::int", &[23]).is_some());
    // The server rejects a cached plan whose result type changed: the entry must go.
    let _ = e.run(&stmt, &[Value::i32(1)], Class::Exec).unwrap();
    let _ = e.take_output();
    let mut reply = msg(b'2', b"");
    reply.extend(msg(
        b'E',
        b"SERROR\0C0A000\0Mcached plan must not change result type\0\0",
    ));
    reply.extend(ready());
    e.feed(&reply).unwrap();
    assert!(e.lookup("SELECT $1::int", &[23]).is_none());
}

#[test]
fn malformed_server_bytes_are_an_error_not_a_panic() {
    let mut rng = SplitMix64::new(3);
    for _ in 0..500 {
        let mut e = engine();
        let stmt = prepared(&mut e);
        let _ = e.run(&stmt, &[Value::i32(1)], Class::Collect);
        let _ = e.take_output();
        let n = rng.range(0, 200);
        let junk: Vec<u8> = (0..n).map(|_| rng.range(0, 256) as u8).collect();
        let _ = e.feed(&junk);
        e.fail_all("test");
        drain(&mut e);
    }
}

/// What the engine writes for runs of the statement with these parameters, with nothing taken back.
fn bytes_of(params: &[i32]) -> Vec<u8> {
    let mut e = engine();
    let stmt = prepared(&mut e);
    for p in params {
        e.run(&stmt, &[Value::i32(*p)], Class::Exec).unwrap();
    }
    e.take_output()
}

#[test]
fn an_unwritten_command_can_be_taken_back_and_a_written_one_cannot() {
    let mut e = engine();
    let stmt = prepared(&mut e);
    let ids: Vec<_> = (1..=4)
        .map(|p| e.run(&stmt, &[Value::i32(p)], Class::Collect).unwrap())
        .collect();
    // the middle, the first and the last: what is left is exactly what was never taken back
    assert!(e.cancel_unsent(ids[1]));
    assert!(e.cancel_unsent(ids[3]));
    assert_eq!(e.in_flight(), 2);
    assert!(!e.cancel_unsent(ids[1]), "already gone");
    assert!(!e.cancel_unsent(9999));
    let out = e.take_output();
    assert_eq!(out, bytes_of(&[1, 3]), "the bytes of 2 and 4 are not sent");
    // written: no longer cancellable; the answers of the other two arrive in order
    assert!(!e.cancel_unsent(ids[0]));
    e.feed(&run_reply(1)).unwrap();
    e.feed(&run_reply(2)).unwrap();
    let evs = drain(&mut e);
    let got: Vec<_> = evs
        .iter()
        .map(|ev| match ev {
            Event::Done { id, result } => (*id, result.as_ref().unwrap().rows),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(got, vec![(ids[0], 1), (ids[2], 2)]);
    assert_eq!(e.in_flight(), 0);
}

#[test]
fn only_a_plain_run_in_a_group_of_its_own_can_be_taken_back() {
    let mut e = engine();
    let stmt = prepared(&mut e);
    let first = e.run(&stmt, &[Value::i32(1)], Class::Exec).unwrap();
    let group = e
        .run_group(&[
            (&stmt, &[Value::i32(2)][..], Class::Exec),
            (&stmt, &[Value::i32(3)][..], Class::Exec),
        ])
        .unwrap();
    let last = e.run(&stmt, &[Value::i32(4)], Class::Exec).unwrap();
    assert!(!e.cancel_unsent(group[0]), "a group shares one Sync");
    assert!(!e.cancel_unsent(group[1]), "a group shares one Sync");
    assert!(e.cancel_unsent(last), "its own group, after a group");
    assert!(e.cancel_unsent(first));
    let out = e.take_output();
    let mut want = engine();
    let s2 = prepared(&mut want);
    want.run_group(&[
        (&s2, &[Value::i32(2)][..], Class::Exec),
        (&s2, &[Value::i32(3)][..], Class::Exec),
    ])
    .unwrap();
    assert_eq!(out, want.take_output());
    // a prepare is never taken back: the statement cache depends on its answer
    let prep = e.prepare("SELECT $1::int + 1", &[23]).unwrap();
    assert!(!e.cancel_unsent(prep));
}
