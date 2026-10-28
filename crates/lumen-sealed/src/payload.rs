//! Sealed payload protection: ChaCha20-Poly1305 under the layer's app key.
//!
//! The nonce is the direction byte, three zero bytes and the 64-bit sequence
//! number (the spec's direction || sequence, padded to the cipher's 12 bytes).
//! The associated data covers layer id, capability, direction, origin,
//! sequence number, type and length. A [`Sender`] never reuses a sequence
//! number, so it never reuses a nonce.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};

use crate::{
    Direction, DirectionKeys, HEADER_LEN, Header, HeaderError, LayerId, MAX_PAYLOAD, ORIGIN_APP,
};

/// Length of the AEAD tag appended to every sealed payload.
pub const TAG_BYTES: usize = 16;

/// Why a payload was refused or could not be sealed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadError {
    /// The header was refused.
    Header(HeaderError),
    /// The body is not exactly the length the header declares, or is shorter
    /// than an AEAD tag.
    Length,
    /// The AEAD check failed.
    Aead,
    /// The plaintext is too long for one frame.
    TooLong,
    /// The sender has used every sequence number.
    SequenceExhausted,
}

fn nonce(dir: Direction, seq: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[0] = dir as u8;
    n[4..].copy_from_slice(&seq.to_be_bytes());
    Nonce::from(n)
}

fn aad(header: &Header, cap: u8) -> [u8; 20] {
    let mut a = [0u8; 20];
    a[0..4].copy_from_slice(&header.layer.0.to_be_bytes());
    a[4] = cap;
    a[5] = header.dir as u8;
    a[6] = header.origin;
    a[7..15].copy_from_slice(&header.seq.to_be_bytes());
    a[15] = header.kind;
    a[16..20].copy_from_slice(&header.len.to_be_bytes());
    a
}

fn cipher(keys: &DirectionKeys) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(Key::from_slice(keys.aead.as_ref()))
}

/// Decrypt `body` for an already verified `header`.
pub(crate) fn open(
    keys: &DirectionKeys,
    header: &Header,
    cap: u8,
    body: &[u8],
) -> Result<Vec<u8>, PayloadError> {
    if body.len() != header.len as usize || body.len() < TAG_BYTES {
        return Err(PayloadError::Length);
    }
    cipher(keys)
        .decrypt(
            &nonce(header.dir, header.seq),
            Payload {
                msg: body,
                aad: &aad(header, cap),
            },
        )
        .map_err(|_| PayloadError::Aead)
}

/// Builds sealed frames for one (layer, direction) with a sequence number that
/// only moves forward.
///
/// Keep exactly one `Sender` per key set. The sequence number lives in the
/// sender, so two senders built from the same keys would each start at their
/// own `first_seq` and reuse nonces, which breaks the AEAD.
#[derive(Debug)]
pub struct Sender {
    keys: DirectionKeys,
    layer: LayerId,
    dir: Direction,
    next_seq: Option<u64>,
}

impl Sender {
    /// A sender whose first frame carries sequence number `first_seq`.
    #[must_use]
    pub fn new(keys: DirectionKeys, layer: LayerId, dir: Direction, first_seq: u64) -> Self {
        Self {
            keys,
            layer,
            dir,
            next_seq: Some(first_seq),
        }
    }

    /// Seal one frame: header followed by the encrypted payload.
    ///
    /// # Errors
    ///
    /// [`PayloadError::TooLong`] if the sealed payload would exceed the frame
    /// limit; [`PayloadError::SequenceExhausted`] after the last sequence number.
    pub fn seal(&mut self, kind: u8, cap: u8, plaintext: &[u8]) -> Result<Vec<u8>, PayloadError> {
        let len = plaintext
            .len()
            .checked_add(TAG_BYTES)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n <= MAX_PAYLOAD)
            .ok_or(PayloadError::TooLong)?;
        let seq = self.next_seq.ok_or(PayloadError::SequenceExhausted)?;
        let header = Header {
            layer: self.layer,
            dir: self.dir,
            origin: ORIGIN_APP,
            seq,
            kind,
            len,
        };
        let body = cipher(&self.keys)
            .encrypt(
                &nonce(self.dir, seq),
                Payload {
                    msg: plaintext,
                    aad: &aad(&header, cap),
                },
            )
            .map_err(|_| PayloadError::Aead)?;
        self.next_seq = seq.checked_add(1);
        let mut out = Vec::with_capacity(HEADER_LEN + body.len());
        out.extend_from_slice(&header.seal(&self.keys));
        out.extend_from_slice(&body);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayerRoot, Receiver};

    const LAYER: LayerId = LayerId(7);
    const CAP: u8 = 5;

    fn keys() -> DirectionKeys {
        DirectionKeys::derive(&LayerRoot::new([1; 32]), LAYER, Direction::ToTerminal)
    }

    fn sender(first_seq: u64) -> Sender {
        Sender::new(keys(), LAYER, Direction::ToTerminal, first_seq)
    }

    fn receiver() -> Receiver {
        Receiver::new(keys(), LAYER, Direction::ToTerminal)
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
            .collect()
    }

    // Known answer: ChaCha20-Poly1305 over "hello sealed" with the derived
    // app key, sequence 1, type 3, capability 5. Nonce = 00 00 00 00 || seq;
    // associated data = layer || capability || direction || origin || seq ||
    // type || length. Computed outside this crate with an independent
    // RFC 8439 implementation that reproduces the RFC's own test vector.
    #[test]
    fn sealed_payload_matches_known_answer() {
        let frame = sender(1).seal(3, CAP, b"hello sealed").unwrap_or_default();
        assert_eq!(
            &frame[HEADER_LEN..],
            hex("c69ce62a986f51d14674830060bf18e7ef7c4fc782e7ec511bab4747")
        );
    }

    // Same inputs in the other direction: the key, nonce and associated data
    // all differ.
    #[test]
    fn sealed_payload_in_the_other_direction_matches_known_answer() {
        let keys = DirectionKeys::derive(&LayerRoot::new([1; 32]), LAYER, Direction::ToShell);
        let frame = Sender::new(keys, LAYER, Direction::ToShell, 1)
            .seal(3, CAP, b"hello sealed")
            .unwrap_or_default();
        assert_eq!(
            &frame[HEADER_LEN..],
            hex("223b00ace263e2a988e67268ba163c9749e2d175498e0e54e3666b93")
        );
    }

    #[test]
    fn seal_then_open_round_trips() {
        let frame = sender(1).seal(3, CAP, b"hello sealed").unwrap_or_default();
        let opened = receiver()
            .open(CAP, &frame)
            .map(|(h, p)| (h.seq, h.kind, p));
        assert_eq!(opened, Ok((1, 3, b"hello sealed".to_vec())));
    }

    #[test]
    fn an_empty_payload_round_trips() {
        let frame = sender(1).seal(3, CAP, b"").unwrap_or_default();
        assert_eq!(frame.len(), HEADER_LEN + TAG_BYTES);
        assert_eq!(receiver().open(CAP, &frame).map(|(_, p)| p), Ok(Vec::new()));
    }

    #[test]
    fn flipping_any_body_byte_fails_the_aead() {
        let good = sender(1).seal(3, CAP, b"hello sealed").unwrap_or_default();
        for i in HEADER_LEN..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 1;
            assert_eq!(
                receiver().open(CAP, &bad),
                Err(PayloadError::Aead),
                "byte {i}"
            );
        }
    }

    #[test]
    fn a_different_capability_fails() {
        let frame = sender(1).seal(3, CAP, b"x").unwrap_or_default();
        assert_eq!(receiver().open(CAP + 1, &frame), Err(PayloadError::Aead));
    }

    #[test]
    fn a_body_of_the_wrong_length_is_refused() {
        let frame = sender(1).seal(3, CAP, b"hello sealed").unwrap_or_default();
        assert_eq!(
            receiver().open(CAP, &frame[..frame.len() - 1]),
            Err(PayloadError::Length)
        );
        let mut longer = frame.clone();
        longer.push(0);
        assert_eq!(receiver().open(CAP, &longer), Err(PayloadError::Length));
    }

    #[test]
    fn a_header_that_fails_is_reported_as_a_header_error() {
        let mut frame = sender(1).seal(3, CAP, b"x").unwrap_or_default();
        frame[0] ^= 1;
        assert!(matches!(
            receiver().open(CAP, &frame),
            Err(PayloadError::Header(_))
        ));
    }

    #[test]
    fn each_frame_uses_a_new_sequence_number_and_nonce() {
        let mut tx = sender(10);
        let a = tx.seal(3, CAP, b"same").unwrap_or_default();
        let b = tx.seal(3, CAP, b"same").unwrap_or_default();
        assert_ne!(a[..HEADER_LEN], b[..HEADER_LEN]);
        assert_ne!(a[HEADER_LEN..], b[HEADER_LEN..]);
        let mut rx = receiver();
        assert_eq!(rx.open(CAP, &a).map(|(h, _)| h.seq), Ok(10));
        assert_eq!(rx.open(CAP, &b).map(|(h, _)| h.seq), Ok(11));
    }

    #[test]
    fn a_replayed_frame_is_refused() {
        let frame = sender(1).seal(3, CAP, b"x").unwrap_or_default();
        let mut rx = receiver();
        assert!(rx.open(CAP, &frame).is_ok());
        assert_eq!(
            rx.open(CAP, &frame),
            Err(PayloadError::Header(HeaderError::Replay))
        );
    }

    #[test]
    fn the_sender_stops_after_the_last_sequence_number() {
        let mut tx = sender(u64::MAX);
        assert!(tx.seal(3, CAP, b"x").is_ok());
        assert_eq!(tx.seal(3, CAP, b"x"), Err(PayloadError::SequenceExhausted));
    }

    #[test]
    fn the_payload_limit_includes_the_tag() {
        let max_plain = MAX_PAYLOAD as usize - TAG_BYTES;
        assert!(sender(1).seal(3, CAP, &vec![0; max_plain]).is_ok());
        assert_eq!(
            sender(1).seal(3, CAP, &vec![0; max_plain + 1]),
            Err(PayloadError::TooLong)
        );
    }
}
