#![no_main]

use libfuzzer_sys::fuzz_target;
use wsc_core::Address;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = std::str::from_utf8(data) {
        let _: Result<Address, _> = value.parse();
    }
});
