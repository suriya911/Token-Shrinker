#![no_main]

use libfuzzer_sys::fuzz_target;
use token_shrinker_provider::parse_mcp_response;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = parse_mcp_response(text, 1024 * 1024);
    }
});
