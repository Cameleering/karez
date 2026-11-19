//! Framing: bytes arrive in pieces of any size and must never panic, loop or buffer more than the
//! limit. The first byte chooses the piece size; every complete message is also parsed.
#![no_main]

use karez_proto_pg::{BackendMessage, FrameReader};
use libfuzzer_sys::fuzz_target;

const MAX_MESSAGE: usize = 1 << 16;

fuzz_target!(|data: &[u8]| {
    let Some((&k, rest)) = data.split_first() else {
        return;
    };
    let piece = usize::from(k) % 97 + 1;
    let mut reader = FrameReader::new(MAX_MESSAGE);
    for chunk in rest.chunks(piece) {
        reader.push(chunk);
        loop {
            match reader.next_frame() {
                Ok(Some(frame)) => {
                    let _ = BackendMessage::parse(frame);
                }
                Ok(None) => break,
                // the connection would be closed here
                Err(_) => return,
            }
        }
        assert!(
            reader.buffered() <= MAX_MESSAGE + 5 + piece,
            "buffered {} bytes, over the limit",
            reader.buffered()
        );
    }
});
