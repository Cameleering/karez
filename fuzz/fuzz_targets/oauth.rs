//! OAUTHBEARER: the server's JSON error is read without panicking, and a token is built into a
//! message only when it cannot change the message's structure.
#![no_main]

use karez_proto_pg::oauth::{initial_response, parse_challenge, valid_token};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse_challenge(data);
    let text = String::from_utf8_lossy(data);
    let ok = valid_token(&text);
    match initial_response(&text) {
        Ok(m) => {
            // exactly three 0x01 separators-or-fewer structure: the token added none
            assert!(ok || text.is_empty());
            assert!(m.starts_with(b"n,,\x01auth="));
            assert!(m.ends_with(b"\x01\x01"));
            assert_eq!(m.iter().filter(|b| **b == 1).count(), 3);
        }
        Err(_) => assert!(!ok && !text.is_empty()),
    }
});
