//! Row decoding. The input is `n`, then `n` column descriptions of five bytes (an index into a list
//! of type OIDs, and a four-byte type modifier), then the body of a `DataRow`. Decoding must not
//! panic, must leave the chunk consistent (`validate`), and every value it produced must be
//! readable as its own type without panicking.
#![no_main]

use karez_chunk::ChunkBuilder;
use karez_proto_pg::backend::FieldDescription;
use karez_proto_pg::ResultPlan;
use libfuzzer_sys::fuzz_target;

/// Types with a decoder (native or rendered), arrays, an enum-like unknown and a type with none.
const OIDS: &[u32] = &[
    16, 17, 19, 20, 21, 23, 25, 700, 701, 1042, 1043, 1082, 1114, 1184, 2950, 18, 26, 114, 142, 650,
    774, 829, 869, 1083, 1186, 1266, 1560, 1562, 1700, 3802, 1007, 1009, 1016, 1231, 1183, 1187,
    2951, 3807, 4242,
];

fuzz_target!(|data: &[u8]| {
    let Some((&n, rest)) = data.split_first() else {
        return;
    };
    let n = usize::from(n) % 33;
    if rest.len() < n * 5 {
        return;
    }
    let (cols, body) = rest.split_at(n * 5);
    let desc: Vec<FieldDescription> = cols
        .chunks_exact(5)
        .enumerate()
        .map(|(i, c)| FieldDescription {
            name: format!("c{i}"),
            table_oid: 0,
            column_attr: 0,
            type_oid: OIDS[usize::from(c[0]) % OIDS.len()],
            type_size: -1,
            type_modifier: i32::from_be_bytes([c[1], c[2], c[3], c[4]]),
            format: 1,
        })
        .collect();
    let plan = ResultPlan::from_description(&desc);
    let mut b = ChunkBuilder::new(plan.fields().to_vec());
    // twice: a failed row must leave the builder able to take the next one
    for _ in 0..2 {
        if plan.decode_row(body, &mut b).is_ok() {
            let row = b.rows() - 1;
            if let Ok(r) = b.chunk().row(row) {
                for c in 0..n {
                    let _ = r.get::<Option<i16>>(c);
                    let _ = r.get::<Option<i32>>(c);
                    let _ = r.get::<Option<i64>>(c);
                    let _ = r.get::<Option<f64>>(c);
                    let _ = r.get::<Option<bool>>(c);
                    let _ = r.get::<Option<&str>>(c);
                    let _ = r.get::<Option<&[u8]>>(c);
                    let _ = r.get::<Option<karez_chunk::Decimal128>>(c);
                    let _ = r.get::<Option<karez_chunk::Interval>>(c);
                    let _ = r.get::<Option<karez_chunk::Time>>(c);
                }
            }
        }
        b.chunk().validate().expect("the chunk stays consistent");
    }
});
