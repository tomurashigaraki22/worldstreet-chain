#![no_main]

use libfuzzer_sys::fuzz_target;
use wsc_core::{canonical_decode, Block, GenesisConfig, Transaction};

fuzz_target!(|data: &[u8]| {
    let _: Result<Block, _> = canonical_decode(data);
    let _: Result<Transaction, _> = canonical_decode(data);
    let _: Result<GenesisConfig, _> = canonical_decode(data);
});
