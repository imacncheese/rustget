#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        // The parser is public so it is also usable by the testkit and tests.
        let _ = rustget_net::probe::parse_content_range(text);
    }
});
