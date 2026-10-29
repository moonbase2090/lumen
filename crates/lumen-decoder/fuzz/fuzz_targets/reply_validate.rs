//! Fuzzes the host's reply validation with arbitrary worker output, and checks
//! what it promises: an accepted image has exact dimensions, stride and length
//! within the reservation, and a header that encodes then parses is unchanged.

#![no_main]

use libfuzzer_sys::fuzz_target;
use lumen_decoder::limits::{self, READ_SLACK};
use lumen_decoder::protocol::{REPLY_LEN, Reply};
use lumen_decoder::runner::{Outcome, validate};

fuzz_target!(|data: &[u8]| {
    let Some((&flags, rest)) = data.split_first() else { return };
    // A hint of up to 65535 x 65535 (refused later, but validation must cope).
    let (w, h, out) = match rest {
        [a, b, c, d, out @ ..] => (u32::from(u16::from_le_bytes([*a, *b])), u32::from(u16::from_le_bytes([*c, *d])), out),
        _ => return,
    };
    let Some(reserved) = limits::rgba_bytes(u64::from(w), u64::from(h)) else { return };
    let cap = reserved + READ_SLACK;
    // The runner never hands over more than cap + 1 bytes.
    let out = &out[..out.len().min(usize::try_from(cap + 1).unwrap_or(usize::MAX))];
    if let Outcome::Image(img) = validate(out, flags & 1 == 1, reserved, cap) {
        let bytes = u64::from(img.width) * u64::from(img.height) * 4;
        assert!(img.width > 0 && img.height > 0);
        assert_eq!(img.rgba.len() as u64, bytes);
        assert!(bytes <= reserved);
        assert!(bytes <= limits::MAX_PIXELS * 4);
    }
    if let Some(head) = out.first_chunk::<REPLY_LEN>() {
        if let Some(r) = Reply::parse(head) {
            assert_eq!(&r.encode(), head);
        }
    }
});
