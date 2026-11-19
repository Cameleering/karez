//! The decoder must give identical results however the byte stream is split.

use karez_chunk::rng::SplitMix64;
use karez_chunk::ChunkBuilder;
use karez_proto_pg::types::oid;
use karez_proto_pg::{BackendMessage, FieldDescription, FrameReader, ResultPlan, TxStatus};

fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn row_description(cols: &[(&str, u32)]) -> Vec<u8> {
    let mut b = (cols.len() as u16).to_be_bytes().to_vec();
    for (name, o) in cols {
        b.extend_from_slice(name.as_bytes());
        b.push(0);
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0i16.to_be_bytes());
        b.extend_from_slice(&o.to_be_bytes());
        b.extend_from_slice(&(-1i16).to_be_bytes());
        b.extend_from_slice(&(-1i32).to_be_bytes());
        b.extend_from_slice(&1i16.to_be_bytes());
    }
    b
}

fn data_row(rng: &mut SplitMix64) -> Vec<u8> {
    let mut b = 5u16.to_be_bytes().to_vec();
    let mut field = |v: Option<Vec<u8>>| match v {
        None => b.extend_from_slice(&(-1i32).to_be_bytes()),
        Some(v) => {
            b.extend_from_slice(&(v.len() as i32).to_be_bytes());
            b.extend_from_slice(&v);
        }
    };
    field(Some(rng.next_u32().to_be_bytes().to_vec()));
    field(if rng.chance(1, 4) {
        None
    } else {
        Some(rng.string(20).into_bytes())
    });
    field(Some(rng.next_u64().to_be_bytes().to_vec()));
    field(if rng.chance(1, 4) {
        None
    } else {
        {
            let n = rng.range(0, 30);
            Some(rng.bytes(n))
        }
    });
    field(Some(vec![u8::from(rng.chance(1, 2))]));
    b
}

fn server_stream() -> Vec<u8> {
    let mut rng = SplitMix64::new(77);
    let mut s = Vec::new();
    s.extend(msg(b'R', &[0, 0, 0, 0]));
    s.extend(msg(b'S', b"server_version\x0016.2\0"));
    s.extend(msg(b'S', b"client_encoding\0UTF8\0"));
    s.extend(msg(b'K', &[0, 0, 1, 2, 9, 9, 9, 9]));
    s.extend(msg(b'Z', b"I"));
    s.extend(msg(b'1', b""));
    s.extend(msg(b't', &[0, 1, 0, 0, 0, 23]));
    s.extend(msg(
        b'T',
        &row_description(&[
            ("a", oid::INT4),
            ("b", oid::TEXT),
            ("c", oid::INT8),
            ("d", oid::BYTEA),
            ("e", oid::BOOL),
        ]),
    ));
    s.extend(msg(b'2', b""));
    for _ in 0..40 {
        s.extend(msg(b'D', &data_row(&mut rng)));
    }
    s.extend(msg(b'N', b"SNOTICE\0C00000\0Mhello\0\0"));
    s.extend(msg(b'C', b"SELECT 40\0"));
    s.extend(msg(b'Z', b"T"));
    s.extend(msg(b'E', b"SERROR\0C42P01\0Mrelation does not exist\0\0"));
    s.extend(msg(b'Z', b"E"));
    s
}

/// Feed the chunks, parse every message, decode rows, and describe everything as strings.
fn run(chunks: &[&[u8]]) -> Vec<String> {
    let mut reader = FrameReader::default();
    let mut plan: Option<ResultPlan> = None;
    let mut builder: Option<ChunkBuilder> = None;
    let mut out = Vec::new();
    for c in chunks {
        reader.push(c);
        while let Some(frame) = reader.next_frame().unwrap() {
            let m = BackendMessage::parse(frame).unwrap();
            match &m {
                BackendMessage::RowDescription(d) => {
                    let p = ResultPlan::from_description(d);
                    builder = Some(ChunkBuilder::new(p.fields().to_vec()));
                    plan = Some(p);
                    out.push(format!("{m:?}"));
                }
                BackendMessage::DataRow(d) => {
                    let b = builder.as_mut().unwrap();
                    plan.as_ref().unwrap().decode_row(d.body, b).unwrap();
                    let r = b.chunk().row(b.rows() - 1).unwrap();
                    out.push(format!(
                        "row {:?} {:?} {:?} {:?} {:?}",
                        r.get::<Option<i32>>(0),
                        r.get::<Option<&str>>(1),
                        r.get::<Option<i64>>(2),
                        r.get::<Option<&[u8]>>(3),
                        r.get::<Option<bool>>(4)
                    ));
                }
                other => out.push(format!("{other:?}")),
            }
        }
    }
    assert_eq!(reader.buffered(), 0, "all bytes consumed");
    out
}

#[test]
fn every_single_split_point_gives_identical_results() {
    let stream = server_stream();
    let whole = run(&[&stream]);
    assert!(whole.iter().any(|l| l.contains("ReadyForQuery(Failed)")));
    assert_eq!(whole.iter().filter(|l| l.starts_with("row")).count(), 40);
    for split in 0..=stream.len() {
        assert_eq!(
            run(&[&stream[..split], &stream[split..]]),
            whole,
            "split at {split}"
        );
    }
}

#[test]
fn byte_at_a_time_and_random_chunking_give_identical_results() {
    let stream = server_stream();
    let whole = run(&[&stream]);
    let singles: Vec<&[u8]> = stream.chunks(1).collect();
    assert_eq!(run(&singles), whole);
    let mut rng = SplitMix64::new(4242);
    for _ in 0..300 {
        let mut parts: Vec<&[u8]> = Vec::new();
        let mut rest = &stream[..];
        while !rest.is_empty() {
            let n = rng.range(1, 40).min(rest.len());
            let (a, b) = rest.split_at(n);
            parts.push(a);
            rest = b;
        }
        assert_eq!(run(&parts), whole);
    }
}

#[test]
fn arbitrary_garbage_never_panics() {
    let mut rng = SplitMix64::new(1);
    for _ in 0..2000 {
        let n = rng.range(0, 60);
        let mut data = rng.bytes(n);
        // Give some inputs a plausible header so the parsers are reached.
        if rng.chance(2, 3) && data.len() >= 5 {
            let body = data.len() - 5;
            data[1..5].copy_from_slice(&((body + 4) as i32).to_be_bytes());
            data[0] = b"RSKZEN12 3ntTDCIsAv"[rng.range(0, 18)];
        }
        let mut r = FrameReader::new(1 << 20);
        r.push(&data);
        while let Ok(Some(f)) = r.next_frame() {
            let _ = BackendMessage::parse(f);
        }
    }
}

#[test]
fn ready_for_query_status_is_parsed() {
    let mut r = FrameReader::default();
    r.push(&msg(b'Z', b"T"));
    let m = BackendMessage::parse(r.next_frame().unwrap().unwrap()).unwrap();
    assert_eq!(m, BackendMessage::ReadyForQuery(TxStatus::InTransaction));
    let _ = FieldDescription {
        name: String::new(),
        table_oid: 0,
        column_attr: 0,
        type_oid: 0,
        type_size: 0,
        type_modifier: 0,
        format: 0,
    };
}
