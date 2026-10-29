//! Fuzzes the png decoder exactly as the worker calls it. The signature is put
//! in front of inputs that lack it, so the fuzzer spends its time in the
//! decoder and not in the format check.

#![no_main]

use libfuzzer_sys::fuzz_target;
use lumen_decoder::decode::decode;
use lumen_decoder::ffi_alloc::CappedAlloc;
use lumen_decoder::limits::MEMORY_BYTES;
use lumen_decoder::protocol::Format;

// The worker's memory line: an allocation past 512 MiB aborts.
#[global_allocator]
static ALLOC: CappedAlloc = CappedAlloc::new(MEMORY_BYTES as usize);

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

fuzz_target!(|data: &[u8]| {
    if Format::sniff(data) == Some(Format::Png) {
        let _ = decode(Format::Png, data);
    } else {
        let mut v = SIGNATURE.to_vec();
        v.extend_from_slice(data);
        let _ = decode(Format::Png, &v);
    }
});
