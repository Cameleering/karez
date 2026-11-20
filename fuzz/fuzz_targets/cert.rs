//! The signature algorithm reader for SCRAM channel binding: any bytes, no panic, and a hash chosen
//! only for an OID that was really found in the input.
#![no_main]

use karez_proto_pg::scram::{cert_hash_algorithm, certificate_signature_oid};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(oid) = certificate_signature_oid(data) {
        // an empty OID is a valid read of a bad certificate; it must just choose no hash
        assert!(oid.len() < data.len());
        if oid.is_empty() {
            assert!(cert_hash_algorithm(oid).is_none());
        }
        let _ = cert_hash_algorithm(oid);
    }
    let _ = cert_hash_algorithm(data);
});
