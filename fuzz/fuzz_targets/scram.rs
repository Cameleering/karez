//! SCRAM and base64. The first byte splits the input into a server-first and a server-final message,
//! which the client must accept or refuse without panicking. Base64 must round-trip: decoding what
//! `encode` wrote gives the bytes back.
#![no_main]

use karez_proto_pg::base64;
use karez_proto_pg::scram::{ChannelBinding, ScramClient};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    assert_eq!(base64::decode(&base64::encode(data)).as_deref(), Some(data));

    let Some((&k, rest)) = data.split_first() else {
        return;
    };
    let (a, b) = rest.split_at((usize::from(k)).min(rest.len()));
    let (a, b) = (String::from_utf8_lossy(a), String::from_utf8_lossy(b));
    let _ = base64::decode(&a);
    for binding in [
        ChannelBinding::None,
        ChannelBinding::Supported,
        ChannelBinding::TlsServerEndPoint(vec![1, 2, 3]),
    ] {
        let mut c = ScramClient::with_binding("u", "pw", "clientnonce".into(), binding);
        let _ = c.client_first();
        // iteration counts come from the server: the client must refuse the absurd ones instead
        // of spending seconds on them (the fuzzer's time limit would flag a hang)
        if c.client_final(&a).is_ok() {
            let _ = c.verify_server_final(&b);
        }
        let _ = c.verify_server_final(&b);
    }
});
