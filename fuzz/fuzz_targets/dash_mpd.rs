#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let base = url::Url::parse("https://fuzz.example.test/path/manifest.mpd").unwrap();
        let _ = rustget_media::parse_mpd(text, &base);
    }
});
