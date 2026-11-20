//! Mutation fuzzing of everything that reads bytes from a server: framing, message parsing,
//! binary row decoding into chunks, SCRAM message handling and base64.
//!
//! No dependency: a corpus of valid server streams is mutated with the in-repo PRNG. The test
//! passes when no input makes any of the parsers panic, loop or buffer past the size limit;
//! errors are expected and fine. A failure prints the seed, the iteration and the input as hex,
//! so it can be replayed with `KAREZ_FUZZ_SEED`.
//!
//! * `KAREZ_FUZZ_ITERS`: mutated streams per test (default 20,000).
//! * `KAREZ_FUZZ_SEED`: first seed (default 1); each iteration uses `seed + iteration`.
//!
//! This is the std-only floor of the M1 fuzz target (plan section 7.3). It does not replace
//! coverage-guided fuzzing (cargo-fuzz or AFL, kept outside the dependency-free crates).

use karez_chunk::rng::SplitMix64;
use karez_chunk::ChunkBuilder;
use karez_proto_pg::types::oid;
use karez_proto_pg::{
    base64, scram::ScramClient, BackendMessage, DataRow, FieldDescription, FrameReader, ResultPlan,
};
use std::panic::{catch_unwind, AssertUnwindSafe};

const MAX_MESSAGE: usize = 1 << 20;

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

/// Every type OID the decoder knows, in one RowDescription.
const OIDS: &[u32] = &[
    oid::BOOL,
    oid::BYTEA,
    oid::NAME,
    oid::INT8,
    oid::INT2,
    oid::INT4,
    oid::TEXT,
    oid::FLOAT4,
    oid::FLOAT8,
    oid::BPCHAR,
    oid::VARCHAR,
    oid::DATE,
    oid::TIMESTAMP,
    oid::TIMESTAMPTZ,
    oid::UUID,
    oid::CHAR,
    oid::OID,
    oid::JSON,
    oid::XML,
    oid::CIDR,
    oid::MACADDR8,
    oid::MACADDR,
    oid::INET,
    oid::TIME,
    oid::INTERVAL,
    oid::TIMETZ,
    oid::BIT,
    oid::VARBIT,
    oid::NUMERIC,
    oid::JSONB,
    oid::NUMERIC, // the second numeric has a type modifier, numeric(12,2): a Decimal128 column
    4242,         // unknown: text fallback
];

/// The type modifier of column `i`: `numeric(12,2)` for the second numeric, none for the rest.
fn typmod(i: usize) -> i32 {
    if i == OIDS.len() - 2 {
        ((12 << 16) | 2) + 4
    } else {
        -1
    }
}

fn row_description() -> Vec<u8> {
    let mut b = (OIDS.len() as u16).to_be_bytes().to_vec();
    for (i, o) in OIDS.iter().enumerate() {
        b.extend_from_slice(format!("c{i}").as_bytes());
        b.push(0);
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0i16.to_be_bytes());
        b.extend_from_slice(&o.to_be_bytes());
        b.extend_from_slice(&(-1i16).to_be_bytes());
        b.extend_from_slice(&typmod(i).to_be_bytes());
        // unknown types come in text format (0), the others in binary (1)
        b.extend_from_slice(&i16::from(*o != 4242).to_be_bytes());
    }
    b
}

/// A plausible binary value for a type, so mutations start near valid input.
fn value_for(o: u32, rng: &mut SplitMix64) -> Option<Vec<u8>> {
    if rng.chance(1, 8) {
        return None;
    }
    Some(match o {
        oid::BOOL => vec![u8::from(rng.chance(1, 2))],
        oid::INT2 => rng.bytes(2),
        oid::INT4 | oid::OID | oid::FLOAT4 => rng.bytes(4),
        oid::DATE => (rng.range(0, 200_000) as i32 - 100_000)
            .to_be_bytes()
            .to_vec(),
        oid::INT8 | oid::FLOAT8 => rng.bytes(8),
        oid::TIMESTAMP | oid::TIMESTAMPTZ => (rng.below(2_000_000_000_000_000) as i64
            - 1_000_000_000_000_000)
            .to_be_bytes()
            .to_vec(),
        oid::TIME => (rng.below(86_400_000_000) as i64).to_be_bytes().to_vec(),
        oid::TIMETZ => {
            let mut v = (rng.below(86_400_000_000) as i64).to_be_bytes().to_vec();
            v.extend_from_slice(&0i32.to_be_bytes());
            v
        }
        oid::UUID => rng.bytes(16),
        oid::INTERVAL => {
            let mut v = (rng.below(1_000_000_000_000) as i64).to_be_bytes().to_vec();
            v.extend_from_slice(&(rng.range(0, 1000) as i32).to_be_bytes());
            v.extend_from_slice(&(rng.range(0, 100) as i32).to_be_bytes());
            v
        }
        oid::MACADDR => rng.bytes(6),
        oid::MACADDR8 => rng.bytes(8),
        oid::INET | oid::CIDR => {
            // family, bits, is_cidr, length, address
            let mut v = vec![2, 24, 0, 4];
            v.extend(rng.bytes(4));
            v
        }
        oid::NUMERIC => {
            // ndigits, weight, sign, dscale, digits; always a value that fits numeric(12,2), so
            // the typed column decodes cleanly too: up to 4 integer digits and 2 fractional ones
            let nd = rng.range(0, 2);
            let mut v = (nd as i16).to_be_bytes().to_vec();
            v.extend_from_slice(&0i16.to_be_bytes()); // weight 0
            v.extend_from_slice(&if rng.chance(1, 4) { [0x40, 0] } else { [0, 0] });
            v.extend_from_slice(&2i16.to_be_bytes()); // dscale 2
            if nd >= 1 {
                v.extend_from_slice(&(rng.range(0, 9999) as i16).to_be_bytes());
            }
            if nd == 2 {
                v.extend_from_slice(&((rng.range(0, 99) * 100) as i16).to_be_bytes());
            }
            v
        }
        oid::JSONB => {
            let mut v = vec![1];
            v.extend_from_slice(b"{\"a\":1}");
            v
        }
        oid::BIT | oid::VARBIT => {
            let mut v = 12i32.to_be_bytes().to_vec();
            v.extend(rng.bytes(2));
            v
        }
        _ => rng.string(12).into_bytes(),
    })
}

fn data_row(rng: &mut SplitMix64) -> Vec<u8> {
    let mut b = (OIDS.len() as u16).to_be_bytes().to_vec();
    for o in OIDS {
        match value_for(*o, rng) {
            None => b.extend_from_slice(&(-1i32).to_be_bytes()),
            Some(v) => {
                b.extend_from_slice(&(v.len() as i32).to_be_bytes());
                b.extend_from_slice(&v);
            }
        }
    }
    b
}

/// A valid server stream covering every message type the client parses.
fn corpus_stream(rng: &mut SplitMix64) -> Vec<u8> {
    let mut s = Vec::new();
    s.extend(msg(b'R', &[0, 0, 0, 0]));
    s.extend(msg(
        b'R',
        &[0, 0, 0, 10, b'S', b'C', b'R', b'A', b'M', b'-', 0, 0],
    ));
    s.extend(msg(b'R', b"\0\0\0\x0br=abc,s=c2FsdA==,i=4096"));
    s.extend(msg(b'R', b"\0\0\0\x0cv=c2VydmVy"));
    s.extend(msg(b'R', &[0, 0, 0, 3]));
    s.extend(msg(b'R', &[0, 0, 0, 5, 1, 2, 3, 4]));
    s.extend(msg(b'S', b"server_version\x0016.2\0"));
    s.extend(msg(b'K', &[0, 0, 1, 2, 9, 9, 9, 9]));
    s.extend(msg(b'Z', b"I"));
    s.extend(msg(b'1', b""));
    s.extend(msg(b'3', b""));
    s.extend(msg(b't', &[0, 2, 0, 0, 0, 23, 0, 0, 0, 25]));
    s.extend(msg(b'T', &row_description()));
    s.extend(msg(b'n', b""));
    s.extend(msg(b'2', b""));
    for _ in 0..8 {
        s.extend(msg(b'D', &data_row(rng)));
    }
    s.extend(msg(b'N', b"SNOTICE\0C00000\0Mhello\0Dmore\0\0"));
    s.extend(msg(b'C', b"SELECT 8\0"));
    s.extend(msg(b'I', b""));
    s.extend(msg(b's', b""));
    s.extend(msg(b'G', &[0, 0, 2, 0, 0, 0, 0]));
    s.extend(msg(b'H', &[1, 0, 1, 0, 0]));
    s.extend(msg(b'd', b"1\t2\n"));
    s.extend(msg(b'c', b""));
    s.extend(msg(b'A', b"\0\0\x04\xd2chan\0payload\0"));
    s.extend(msg(
        b'E',
        b"SERROR\0VERROR\0C23505\0Mduplicate key\0Dthe detail\0Hthe hint\0P7\0Wctx\0sS\0tT\0cC\0dD\0nN\0Zx\0\0",
    ));
    s.extend(msg(b'Z', b"E"));
    s
}

fn mutate(rng: &mut SplitMix64, mut b: Vec<u8>) -> Vec<u8> {
    for _ in 0..rng.range(1, 6) {
        if b.is_empty() {
            b.push(rng.next_u64() as u8);
            continue;
        }
        let at = rng.range(0, b.len() - 1);
        match rng.below(8) {
            0 => b[at] ^= 1 << rng.below(8), // bit flip
            1 => b[at] = rng.next_u64() as u8,
            2 => {
                let n = rng.range(1, 8).min(b.len() - at);
                b.drain(at..at + n); // delete
            }
            3 => {
                let n = rng.range(1, 8);
                let junk = rng.bytes(n);
                b.splice(at..at, junk); // insert
            }
            4 => b.truncate(at), // cut the stream short
            5 => {
                // tamper with a 4-byte length (or any aligned-ish i32) with an edge value
                if at + 4 <= b.len() {
                    let v: i32 = [0, 3, 4, 5, -1, i32::MAX, i32::MIN, 0x7fff_0000][rng.range(0, 7)];
                    b[at..at + 4].copy_from_slice(&v.to_be_bytes());
                }
            }
            6 => {
                // duplicate a slice
                let n = rng.range(1, 32).min(b.len() - at);
                let chunk = b[at..at + n].to_vec();
                b.splice(at..at, chunk);
            }
            _ => {
                // set a byte to an edge value
                b[at] = [0, 0xff, 0x7f, 0x80, 1][rng.range(0, 4)];
            }
        }
    }
    b
}

/// Feed the bytes to the whole decode path in random splits.
fn drive(rng: &mut SplitMix64, input: &[u8]) {
    let mut reader = FrameReader::new(MAX_MESSAGE);
    let mut plan: Option<ResultPlan> = None;
    let mut builder: Option<ChunkBuilder> = None;
    let mut pos = 0;
    while pos < input.len() {
        let n = rng.range(1, 300).min(input.len() - pos);
        reader.push(&input[pos..pos + n]);
        pos += n;
        // never more than the limit plus one header buffered per frame
        loop {
            let frame = match reader.next_frame() {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(_) => return, // the connection would be dropped here
            };
            let tag = frame.tag;
            let body = frame.body;
            let parsed = BackendMessage::parse(frame);
            if tag == b'D' {
                if let Ok(d) = DataRow::parse(body) {
                    for f in d.fields() {
                        let _ = f;
                    }
                }
                if let (Some(p), Some(b)) = (plan.as_ref(), builder.as_mut()) {
                    if p.decode_row(body, b).is_ok() {
                        let row = b.rows() - 1;
                        if let Ok(r) = b.chunk().row(row) {
                            for c in 0..OIDS.len() {
                                let _ = r.get::<Option<i64>>(c);
                                let _ = r.get::<Option<i32>>(c);
                                let _ = r.get::<Option<f64>>(c);
                                let _ = r.get::<Option<bool>>(c);
                                let _ = r.get::<Option<&str>>(c);
                                let _ = r.get::<Option<&[u8]>>(c);
                            }
                        }
                    }
                    if b.rows() >= 64 {
                        b.reset();
                    }
                }
                continue;
            }
            if let Ok(BackendMessage::RowDescription(d)) = parsed {
                let d: Vec<FieldDescription> = d;
                // a hostile description can name any number of columns; cap what a test allocates
                if d.len() <= 4096 {
                    let p = ResultPlan::from_description(&d);
                    builder = Some(ChunkBuilder::new(p.fields().to_vec()));
                    plan = Some(p);
                }
            }
        }
        assert!(
            reader.buffered() <= MAX_MESSAGE + 5 + 300,
            "buffered {} bytes, over the limit",
            reader.buffered()
        );
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn run_fuzz(
    name: &str,
    one: impl Fn(&mut SplitMix64) -> Vec<u8>,
    check: impl Fn(&mut SplitMix64, &[u8]),
) {
    let iters = env_u64("KAREZ_FUZZ_ITERS", 20_000);
    let seed0 = env_u64("KAREZ_FUZZ_SEED", 1);
    for i in 0..iters {
        let seed = seed0 + i;
        let mut rng = SplitMix64::new(seed);
        let input = one(&mut rng);
        let result = catch_unwind(AssertUnwindSafe(|| check(&mut rng, &input)));
        if result.is_err() {
            let shown = if input.len() > 4000 {
                format!("{}... ({} bytes)", hex(&input[..4000]), input.len())
            } else {
                hex(&input)
            };
            panic!("fuzz target {name} panicked: KAREZ_FUZZ_SEED={seed} KAREZ_FUZZ_ITERS=1, input {shown}");
        }
    }
}

#[test]
fn mutated_server_streams_never_panic() {
    run_fuzz(
        "backend stream",
        |rng| {
            let base = corpus_stream(rng);
            mutate(rng, base)
        },
        drive,
    );
}

#[test]
fn random_bytes_never_panic() {
    run_fuzz(
        "random bytes",
        |rng| {
            let n = rng.range(0, 600);
            rng.bytes(n)
        },
        drive,
    );
}

#[test]
fn unmutated_corpus_decodes_cleanly() {
    // the corpus itself must be valid, or the fuzzing above starts from nothing
    let mut rng = SplitMix64::new(5);
    let stream = corpus_stream(&mut rng);
    let mut reader = FrameReader::new(MAX_MESSAGE);
    reader.push(&stream);
    let mut n = 0;
    let mut plan: Option<ResultPlan> = None;
    let mut b: Option<ChunkBuilder> = None;
    while let Some(frame) = reader.next_frame().expect("framing") {
        let tag = frame.tag;
        let body = frame.body;
        if tag == b'D' {
            plan.as_ref()
                .unwrap()
                .decode_row(body, b.as_mut().unwrap())
                .unwrap_or_else(|e| panic!("corpus row failed: {e}"));
            n += 1;
            continue;
        }
        let m = BackendMessage::parse(frame).unwrap_or_else(|e| panic!("tag {}: {e}", tag as char));
        if let BackendMessage::RowDescription(d) = m {
            let p = ResultPlan::from_description(&d);
            b = Some(ChunkBuilder::new(p.fields().to_vec()));
            plan = Some(p);
        }
    }
    assert_eq!(n, 8, "all corpus rows decode");
    assert_eq!(reader.buffered(), 0);
}

#[test]
fn scram_and_base64_never_panic() {
    run_fuzz(
        "scram and base64",
        |rng| {
            let n = rng.range(0, 200);
            let mut v = rng.bytes(n);
            // bias toward the shapes the parser looks for
            if rng.chance(1, 2) {
                v.splice(0..0, b"r=abc,s=c2FsdA==,i=4096".iter().copied());
            }
            v
        },
        |_, input| {
            let text = String::from_utf8_lossy(input).into_owned();
            let mut c = ScramClient::new("user", "pw", "clientnonce".to_string());
            let _ = c.client_first();
            if let Ok(f) = c.client_final(&text) {
                let _ = f;
            }
            let _ = c.verify_server_final(&text);
            let _ = base64::decode(&text);
            let enc = base64::encode(input);
            assert_eq!(
                base64::decode(&enc).as_deref(),
                Some(input),
                "base64 round trip"
            );
        },
    );
}

/// Write the seed corpus for the coverage-guided targets in `fuzz/` (see `fuzz/README.md`):
/// `cargo test -p karez-proto-pg --test fuzz_backend dump_seed_corpus -- --ignored`.
/// The seeds are valid inputs, so the fuzzer starts from code the parsers actually reach.
#[test]
#[ignore = "writes files under fuzz/corpus"]
fn dump_seed_corpus() {
    use std::fs;
    use std::path::PathBuf;

    // the OID list of fuzz/fuzz_targets/row.rs; the index is what the seed names
    const ROW_TARGET_OIDS: &[u32] = &[
        16, 17, 19, 20, 21, 23, 25, 700, 701, 1042, 1043, 1082, 1114, 1184, 2950, 18, 26, 114, 142,
        650, 774, 829, 869, 1083, 1186, 1266, 1560, 1562, 1700, 3802, 1007, 1009, 1016, 1231, 1183,
        1187, 2951, 3807, 4242,
    ];
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus");
    let put = |target: &str, name: &str, bytes: &[u8]| {
        let dir = root.join(target);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), bytes).unwrap();
    };

    // frame: a piece-size byte and a whole server stream
    let mut rng = SplitMix64::new(11);
    for (i, k) in [0u8, 6, 40, 200].iter().enumerate() {
        let mut s = vec![*k];
        s.extend(corpus_stream(&mut rng));
        put("frame", &format!("stream{i}"), &s);
    }

    // message: tag and body, one seed per message in the stream
    let stream = corpus_stream(&mut SplitMix64::new(12));
    let mut reader = FrameReader::new(MAX_MESSAGE);
    reader.push(&stream);
    let mut n = 0;
    while let Some(f) = reader.next_frame().unwrap() {
        let mut s = vec![f.tag];
        s.extend_from_slice(f.body);
        put("message", &format!("{n:02}-{}", f.tag), &s);
        n += 1;
    }

    // row: columns of the stream's description, mapped to the target's OID list
    let mut rng = SplitMix64::new(13);
    for i in 0..6 {
        let cols: Vec<usize> = OIDS
            .iter()
            .map(|o| ROW_TARGET_OIDS.iter().position(|t| t == o).unwrap())
            .collect();
        let mut s = vec![cols.len() as u8];
        for (j, c) in cols.iter().enumerate() {
            s.push(*c as u8);
            s.extend_from_slice(&typmod(j).to_be_bytes());
        }
        s.extend(data_row(&mut rng));
        put("row", &format!("full{i}"), &s);
    }
    // one narrow seed per type, so each decoder is reached early
    for (j, o) in OIDS.iter().enumerate() {
        let c = ROW_TARGET_OIDS.iter().position(|t| t == o).unwrap();
        let mut s = vec![1u8, c as u8];
        s.extend_from_slice(&typmod(j).to_be_bytes());
        let mut body = 1u16.to_be_bytes().to_vec();
        let v = value_for(*o, &mut rng).unwrap_or_default();
        body.extend_from_slice(&(v.len() as i32).to_be_bytes());
        body.extend_from_slice(&v);
        s.extend(body);
        put("row", &format!("one{j:02}"), &s);
    }

    // scram: a split byte, the server-first message, then the server-final message
    for (i, (first, last)) in [
        ("r=clientnonceXYZ,s=c2FsdA==,i=4096", "v=c2VydmVy"),
        ("r=clientnonceXYZ,s=c2FsdA==,i=1", "e=invalid-proof"),
        ("r=other,s=!!,i=0", ""),
    ]
    .iter()
    .enumerate()
    {
        let mut s = vec![first.len() as u8];
        s.extend_from_slice(first.as_bytes());
        s.extend_from_slice(last.as_bytes());
        put("scram", &format!("exchange{i}"), &s);
    }

    // oauth: the server's JSON error and some tokens
    for (i, j) in [
        r#"{"status":"invalid_token","scope":"openid","openid-configuration":"https://idp.example/.well-known/openid-configuration"}"#,
        r#"{"status":"invalid_token"}"#,
        r#"{"scope":"a b c","status":"x\u00e9\n"}"#,
        "abc.def-ghi_jkl~+/=",
        "bad token\u{1}",
        "",
    ]
    .iter()
    .enumerate()
    {
        put("oauth", &format!("case{i}"), j.as_bytes());
    }

    // cert: the DER of the fixture CA certificates, when the compose fixtures have been started
    let certs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/docker/certs");
    let mut found = 0;
    if let Ok(rd) = fs::read_dir(&certs) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "crt") {
                let pem = fs::read_to_string(&p).unwrap();
                let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
                if let Some(der) = base64::decode(&b64) {
                    put("cert", &format!("fixture{found}"), &der);
                    found += 1;
                }
            }
        }
    }
    println!("certificate seeds: {found}");

    // config_url: connection URLs and key/value tails
    for (i, u) in [
        "postgres://u:pw@localhost:5432/db",
        "postgresql://u@h1:5432,h2:5433/db?target_session_attrs=read-write&load_balance_hosts=random",
        "postgres://u:p%40ss@[::1]:5432/d?sslmode=verify-full&sslrootcert=/ca.pem&channel_binding=require",
        "postgres://u@h/d?sslcert=/c.crt&sslkey=/c.key&sslnegotiation=direct",
        "postgres://u@h/d?auth=gss&krbsrvname=postgres",
        "sslmode=disable&connect_timeout=3&application_name=a%20b",
        "",
        "postgres://",
    ]
    .iter()
    .enumerate()
    {
        put("config_url", &format!("url{i}"), u.as_bytes());
    }
}
