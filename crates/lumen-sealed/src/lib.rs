//! Sealed-layer keys and frame headers.
//!
//! Keys are derived with HKDF-SHA256 from a per-layer root, with the layer id
//! and the direction bound into every derivation. A sealed frame header has one
//! fixed layout; its 64-byte authenticator field holds a 128-bit tag,
//! zero-padded, and is verified before any payload byte is consumed.
//!
//! The tag also covers the origin byte, which the spec's list of tagged fields
//! does not name; covering more than the spec lists only makes forgery harder.
//!
//! Memory hygiene: the root and the derived keys are zeroized on drop. The intermediate HKDF pseudo-random keys
//! and the HMAC states inside the `hkdf` and `hmac` crates are not, because
//! those types do not zeroize themselves. That is a known gap. Like the spec's
//! other key-page measures it is hygiene, not a security boundary.
//!
//! The byte layout is the phase 0.5 experiment profile, not a normative
//! encoding. This crate does no I/O and holds no process state beyond a
//! receiver's last sequence number.

#![forbid(unsafe_code)]

mod payload;

pub use payload::{PayloadError, Sender, TAG_BYTES};

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

/// Length of the authenticator field in a header.
pub const AUTH_LEN: usize = 64;
/// Length of the tag inside the authenticator field.
pub const TAG_LEN: usize = 16;
/// Length of the fixed fields that the tag covers.
const FIELDS_LEN: usize = 4 + 1 + 1 + 8 + 1 + 4;
/// Total header length on the wire.
pub const HEADER_LEN: usize = FIELDS_LEN + AUTH_LEN;
/// Largest payload length a header may declare (the 1 MiB frame limit).
pub const MAX_PAYLOAD: u32 = 1 << 20;
/// Frame type bytes at or above this value are unsealed (Track 0) types. A
/// sealed header never carries one (domain separation).
pub const FIRST_UNSEALED_KIND: u8 = 0x80;
/// The only valid origin byte in v0: the app.
pub const ORIGIN_APP: u8 = 0;

/// Terminal-assigned layer id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerId(pub u32);

/// Direction of a frame, bound into every key and tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// From the shell integration to the terminal.
    ToTerminal = 0,
    /// From the terminal to the shell integration.
    ToShell = 1,
}

/// A per-layer root secret. It never leaves the terminal.
pub struct LayerRoot(Zeroizing<[u8; 32]>);

impl LayerRoot {
    /// Wrap 32 bytes of root key material.
    #[must_use]
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }
}

impl core::fmt::Debug for LayerRoot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("LayerRoot(<redacted>)")
    }
}

/// Keys for one (layer, direction).
pub struct DirectionKeys {
    aead: Zeroizing<[u8; 32]>,
    header: Zeroizing<[u8; 32]>,
}

impl core::fmt::Debug for DirectionKeys {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("DirectionKeys(<redacted>)")
    }
}

fn expand(prk: &Hkdf<Sha256>, layer: LayerId, dir: Direction, label: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut info = Vec::with_capacity(label.len() + 5);
    info.extend_from_slice(label);
    info.extend_from_slice(&layer.0.to_be_bytes());
    info.push(dir as u8);
    let mut out = Zeroizing::new([0u8; 32]);
    // 32 bytes is far below the HKDF-SHA256 output limit, so this cannot fail.
    if prk.expand(&info, out.as_mut()).is_err() {
        unreachable!("HKDF-SHA256 expand of 32 bytes cannot fail");
    }
    out
}

impl DirectionKeys {
    /// Derive the app AEAD key and the header subkey for a layer and direction.
    #[must_use]
    pub fn derive(root: &LayerRoot, layer: LayerId, dir: Direction) -> Self {
        let prk = Hkdf::<Sha256>::new(None, root.0.as_ref());
        let app = expand(&prk, layer, dir, b"lumen v0 app key");
        let app_prk = Hkdf::<Sha256>::new(None, app.as_ref());
        let header = expand(&app_prk, layer, dir, b"lumen v0 header subkey");
        Self { aead: app, header }
    }

    /// The payload AEAD key (used by the payload layer, not by this crate).
    #[must_use]
    pub fn aead_key(&self) -> &[u8; 32] {
        &self.aead
    }
}

/// The fields of a sealed frame header that the tag covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Layer id.
    pub layer: LayerId,
    /// Direction.
    pub dir: Direction,
    /// Origin byte; only [`ORIGIN_APP`] is valid in v0.
    pub origin: u8,
    /// Sequence number, strictly increasing per (layer, direction).
    pub seq: u64,
    /// Frame type byte.
    pub kind: u8,
    /// Declared payload length.
    pub len: u32,
}

impl Header {
    fn fields(&self) -> [u8; FIELDS_LEN] {
        let mut out = [0u8; FIELDS_LEN];
        out[0..4].copy_from_slice(&self.layer.0.to_be_bytes());
        out[4] = self.dir as u8;
        out[5] = self.origin;
        out[6..14].copy_from_slice(&self.seq.to_be_bytes());
        out[14] = self.kind;
        out[15..19].copy_from_slice(&self.len.to_be_bytes());
        out
    }

    fn mac(keys: &DirectionKeys, fields: &[u8; FIELDS_LEN]) -> Hmac<Sha256> {
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(keys.header.as_ref()) else {
            unreachable!("HMAC accepts any key length")
        };
        mac.update(b"lumen v0 header tag");
        mac.update(fields);
        mac
    }

    /// Build the wire header, tagged under `keys`.
    #[must_use]
    pub fn seal(&self, keys: &DirectionKeys) -> [u8; HEADER_LEN] {
        let fields = self.fields();
        let tag = Self::mac(keys, &fields).finalize().into_bytes();
        let mut out = [0u8; HEADER_LEN];
        out[..FIELDS_LEN].copy_from_slice(&fields);
        out[FIELDS_LEN..FIELDS_LEN + TAG_LEN].copy_from_slice(&tag[..TAG_LEN]);
        out
    }
}

/// Why a header was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// Fewer than [`HEADER_LEN`] bytes.
    Short,
    /// The zero padding after the tag is not all zero.
    Padding,
    /// The tag does not verify.
    Tag,
    /// The layer id or direction is not this receiver's.
    WrongLayerOrDirection,
    /// The origin byte is not [`ORIGIN_APP`].
    Origin,
    /// The frame type is in the unsealed range.
    UnsealedKind,
    /// The declared length is over [`MAX_PAYLOAD`].
    TooLong,
    /// The sequence number is not greater than the last accepted one.
    Replay,
}

/// Verifies headers for one (layer, direction) and tracks the sequence.
#[derive(Debug)]
pub struct Receiver {
    keys: DirectionKeys,
    layer: LayerId,
    dir: Direction,
    last_seq: Option<u64>,
}

impl Receiver {
    /// A receiver for frames travelling `dir` on `layer`.
    #[must_use]
    pub fn new(keys: DirectionKeys, layer: LayerId, dir: Direction) -> Self {
        Self {
            keys,
            layer,
            dir,
            last_seq: None,
        }
    }

    /// Verify a header before any payload byte is consumed. Only the first
    /// [`HEADER_LEN`] bytes are read. The sequence number advances only when
    /// everything else has verified, so unauthenticated bytes change nothing.
    ///
    /// # Errors
    ///
    /// Returns the first reason the header is refused.
    pub fn verify(&mut self, bytes: &[u8]) -> Result<Header, HeaderError> {
        let bytes = bytes.get(..HEADER_LEN).ok_or(HeaderError::Short)?;
        let (fields, auth) = bytes.split_at(FIELDS_LEN);
        let (tag, padding) = auth.split_at(TAG_LEN);
        if padding.iter().fold(0u8, |acc, b| acc | b) != 0 {
            return Err(HeaderError::Padding);
        }
        let mut raw = [0u8; FIELDS_LEN];
        raw.copy_from_slice(fields);
        Header::mac(&self.keys, &raw)
            .verify_truncated_left(tag)
            .map_err(|_| HeaderError::Tag)?;

        let layer = LayerId(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]));
        let dir = match raw[4] {
            0 => Direction::ToTerminal,
            1 => Direction::ToShell,
            _ => return Err(HeaderError::WrongLayerOrDirection),
        };
        if layer != self.layer || dir != self.dir {
            return Err(HeaderError::WrongLayerOrDirection);
        }
        if raw[5] != ORIGIN_APP {
            return Err(HeaderError::Origin);
        }
        if raw[14] >= FIRST_UNSEALED_KIND {
            return Err(HeaderError::UnsealedKind);
        }
        let mut seq = [0u8; 8];
        seq.copy_from_slice(&raw[6..14]);
        let seq = u64::from_be_bytes(seq);
        let len = u32::from_be_bytes([raw[15], raw[16], raw[17], raw[18]]);
        if len > MAX_PAYLOAD {
            return Err(HeaderError::TooLong);
        }
        if self.last_seq.is_some_and(|last| seq <= last) {
            return Err(HeaderError::Replay);
        }
        self.last_seq = Some(seq);
        Ok(Header {
            layer,
            dir,
            origin: raw[5],
            seq,
            kind: raw[14],
            len,
        })
    }

    /// Verify a whole sealed frame (header, then payload) and return the
    /// header and the decrypted payload. `cap` is the capability the caller
    /// expects this frame to carry; it is bound into the associated data, so a
    /// frame sealed for another capability fails.
    ///
    /// # Errors
    ///
    /// Returns a header error if the header is refused, or a payload error if
    /// the body length is wrong or the AEAD check fails. A header that verified
    /// has consumed its sequence number even if the body then fails.
    pub fn open(&mut self, cap: u8, frame: &[u8]) -> Result<(Header, Vec<u8>), PayloadError> {
        let header = self.verify(frame).map_err(PayloadError::Header)?;
        let body = &frame[HEADER_LEN..];
        let plain = payload::open(&self.keys, &header, cap, body)?;
        Ok((header, plain))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYER: LayerId = LayerId(7);

    fn keys(root_byte: u8, layer: LayerId, dir: Direction) -> DirectionKeys {
        DirectionKeys::derive(&LayerRoot::new([root_byte; 32]), layer, dir)
    }

    fn header(seq: u64) -> Header {
        Header {
            layer: LAYER,
            dir: Direction::ToTerminal,
            origin: ORIGIN_APP,
            seq,
            kind: 3,
            len: 100,
        }
    }

    fn receiver() -> Receiver {
        Receiver::new(
            keys(1, LAYER, Direction::ToTerminal),
            LAYER,
            Direction::ToTerminal,
        )
    }

    fn sealed(seq: u64) -> [u8; HEADER_LEN] {
        header(seq).seal(&keys(1, LAYER, Direction::ToTerminal))
    }

    #[test]
    fn a_sealed_header_verifies() {
        assert_eq!(receiver().verify(&sealed(1)), Ok(header(1)));
    }

    #[test]
    fn flipping_any_header_byte_is_refused() {
        let good = sealed(1);
        for i in 0..HEADER_LEN {
            let mut bad = good;
            bad[i] ^= 0x01;
            assert!(receiver().verify(&bad).is_err(), "byte {i} accepted");
        }
    }

    #[test]
    fn nonzero_padding_is_refused_as_padding() {
        let mut bad = sealed(1);
        bad[FIELDS_LEN + TAG_LEN + 5] = 1;
        assert_eq!(receiver().verify(&bad), Err(HeaderError::Padding));
    }

    #[test]
    fn short_input_is_refused() {
        assert_eq!(
            receiver().verify(&sealed(1)[..HEADER_LEN - 1]),
            Err(HeaderError::Short)
        );
    }

    #[test]
    fn a_header_under_other_keys_is_refused() {
        for other in [
            keys(2, LAYER, Direction::ToTerminal),
            keys(1, LayerId(8), Direction::ToTerminal),
            keys(1, LAYER, Direction::ToShell),
        ] {
            let bytes = header(1).seal(&other);
            assert_eq!(receiver().verify(&bytes), Err(HeaderError::Tag));
        }
    }

    #[test]
    fn a_valid_tag_for_another_layer_or_direction_is_refused() {
        // Correctly tagged under this receiver's keys, but for fields that
        // are not this receiver's.
        let k = keys(1, LAYER, Direction::ToTerminal);
        let other_layer = Header {
            layer: LayerId(8),
            ..header(1)
        }
        .seal(&k);
        let other_dir = Header {
            dir: Direction::ToShell,
            ..header(1)
        }
        .seal(&k);
        assert_eq!(
            receiver().verify(&other_layer),
            Err(HeaderError::WrongLayerOrDirection)
        );
        assert_eq!(
            receiver().verify(&other_dir),
            Err(HeaderError::WrongLayerOrDirection)
        );
    }

    #[test]
    fn only_app_origin_is_valid() {
        let k = keys(1, LAYER, Direction::ToTerminal);
        let bytes = Header {
            origin: 1,
            ..header(1)
        }
        .seal(&k);
        assert_eq!(receiver().verify(&bytes), Err(HeaderError::Origin));
    }

    #[test]
    fn unsealed_type_bytes_are_refused() {
        let k = keys(1, LAYER, Direction::ToTerminal);
        let top_sealed = Header {
            kind: FIRST_UNSEALED_KIND - 1,
            ..header(1)
        }
        .seal(&k);
        let bottom_unsealed = Header {
            kind: FIRST_UNSEALED_KIND,
            ..header(1)
        }
        .seal(&k);
        assert!(receiver().verify(&top_sealed).is_ok());
        assert_eq!(
            receiver().verify(&bottom_unsealed),
            Err(HeaderError::UnsealedKind)
        );
    }

    #[test]
    fn declared_length_is_capped() {
        let k = keys(1, LAYER, Direction::ToTerminal);
        let ok = Header {
            len: MAX_PAYLOAD,
            ..header(1)
        }
        .seal(&k);
        let too_long = Header {
            len: MAX_PAYLOAD + 1,
            ..header(1)
        }
        .seal(&k);
        assert!(receiver().verify(&ok).is_ok());
        assert_eq!(receiver().verify(&too_long), Err(HeaderError::TooLong));
    }

    #[test]
    fn sequence_must_strictly_increase_and_gaps_are_allowed() {
        let mut rx = receiver();
        assert!(rx.verify(&sealed(5)).is_ok());
        assert_eq!(rx.verify(&sealed(5)), Err(HeaderError::Replay));
        assert_eq!(rx.verify(&sealed(4)), Err(HeaderError::Replay));
        assert!(rx.verify(&sealed(100)).is_ok());
    }

    #[test]
    fn a_refused_header_does_not_advance_the_sequence() {
        let mut rx = receiver();
        let mut forged = sealed(50);
        forged[FIELDS_LEN] ^= 1;
        assert_eq!(rx.verify(&forged), Err(HeaderError::Tag));
        assert!(rx.verify(&sealed(1)).is_ok());
    }

    #[test]
    fn keys_differ_by_purpose_layer_and_direction() {
        let a = keys(1, LAYER, Direction::ToTerminal);
        let b = keys(1, LayerId(8), Direction::ToTerminal);
        let c = keys(1, LAYER, Direction::ToShell);
        assert_ne!(a.aead_key(), b.aead_key());
        assert_ne!(a.aead_key(), c.aead_key());
        assert_ne!(a.aead_key(), &*a.header);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
            .collect()
    }

    // Known-answer vectors, computed outside this crate with an independent
    // HKDF-SHA256 and HMAC-SHA256 from the stated inputs: root = 32 bytes of 0x01, no salt, info = label || layer id
    // (4 bytes big-endian) || direction byte.
    #[test]
    fn derivation_matches_known_answers() {
        let a = keys(1, LAYER, Direction::ToTerminal);
        assert_eq!(
            a.aead_key().as_slice(),
            hex("145545e30e7b0dfb41aa9b4a37be0f100b1c16e1452b926e8198ab20fc25d0ff")
        );
        assert_eq!(
            a.header.as_slice(),
            hex("412412728eb4aef1c7c69ebc2b496922421b02b5146cac0f521da9a024d24bc9")
        );
        assert_eq!(
            keys(1, LAYER, Direction::ToShell).aead_key().as_slice(),
            hex("dcfd97ba198233732b6f3f3df6f3212c698863701f647b891d0a27b4f7d9929f")
        );
        assert_eq!(
            keys(1, LayerId(8), Direction::ToTerminal)
                .aead_key()
                .as_slice(),
            hex("19efe04f609ab26453e5eb9ae7295417ee979e45add69d92e9df224ddb21fade")
        );
    }

    #[test]
    fn sealed_header_matches_known_answer() {
        // Fields: layer 7, to-terminal, app origin, seq 1, type 3, length 100;
        // tag = HMAC-SHA256(header subkey, "lumen v0 header tag" || fields)[..16].
        let expected = hex(concat!(
            "00000007000000000000000000010300",
            "0000644f62d6150b951234f240332d55",
            "d9f6d900000000000000000000000000",
            "00000000000000000000000000000000",
            "00000000000000000000000000000000",
            "000000",
        ));
        assert_eq!(expected.len(), HEADER_LEN);
        assert_eq!(sealed(1).as_slice(), expected);
        assert_eq!(receiver().verify(&expected), Ok(header(1)));
    }

    #[test]
    fn debug_output_redacts_key_material() {
        let k = keys(1, LAYER, Direction::ToTerminal);
        assert_eq!(format!("{k:?}"), "DirectionKeys(<redacted>)");
        assert_eq!(
            format!("{:?}", LayerRoot::new([9; 32])),
            "LayerRoot(<redacted>)"
        );
    }
}
