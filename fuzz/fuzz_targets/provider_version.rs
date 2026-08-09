#![no_main]

use libfuzzer_sys::fuzz_target;
use semver::VersionReq;
use token_shrinker_provider::validate_version;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let requirement = VersionReq::parse(">=0.1.0, <100.0.0").expect("static requirement");
        let _ = validate_version(text, &requirement);
    }
});
