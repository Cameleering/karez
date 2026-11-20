//! One backend message: the first byte is the tag, the rest the body. Parsing must return a message
//! or an error, never panic, and reading the fields of a parsed `DataRow` must stay in bounds.
#![no_main]

use karez_proto_pg::{BackendMessage, DataRow, Frame};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&tag, body)) = data.split_first() else {
        return;
    };
    if let Ok(BackendMessage::DataRow(row)) = BackendMessage::parse(Frame { tag, body }) {
        for f in row.fields() {
            let _ = f;
        }
    }
    if let Ok(row) = DataRow::parse(body) {
        let n = row.fields().count();
        assert!(n <= usize::from(row.columns));
    }
});
