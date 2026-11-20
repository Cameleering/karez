//! The connection URL parser: any text, no panic, and a password in the URL never shows in `Debug`.
#![no_main]

use karez_pg::Config;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(cfg) = Config::from_url(&text) {
        let _ = format!("{cfg:?}");
        let _ = cfg.hosts();
    }
    // the same text as the query part of a URL that carries a known password
    let url = format!("postgres://u:SECRETpw9x@h/db?{text}");
    if let Ok(cfg) = Config::from_url(&url) {
        let shown = format!("{cfg:?}");
        // the fuzzer may have set the password itself, or put the text in the output by other keys
        if !text.contains("SECRETpw9x") && !text.contains("password") {
            assert!(!shown.contains("SECRETpw9x"), "the password is in Debug");
        }
        let _ = cfg.hosts();
    }
});
