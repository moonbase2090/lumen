//! Arm A of the phase 0.5 comparison: sealed-only rich.
//!
//! Rich blocks render only on a sealed layer. On an unsealed layer, Track 0
//! output renders as escaped text. This crate holds the pure policy; it does
//! no I/O and holds no keys.

#![forbid(unsafe_code)]

pub mod pipeline;

/// The result of each default sealing requirement for one layer, checked
/// once at layer start (see the spec's sealed-layers page).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per spec requirement"
)]
pub struct SealingChecks {
    /// Per-layer AEAD key derived from a terminal-held root.
    pub per_layer_key: bool,
    /// A separate key-holder process with only its seal and control fds.
    pub separate_holder: bool,
    /// Keys go only to the listed shell integration over the control fd.
    pub key_delivery: bool,
    /// Key pages are non-dumpable, excluded from dumps and locked.
    pub memory_hygiene: bool,
    /// The session is local, not over SSH.
    pub local_session: bool,
    /// The policy file passes its owner and mode checks.
    pub policy_file: bool,
    /// If Track B is used, its socket requires its token. Pass `true` when
    /// Track B is not used.
    pub side_channel: bool,
}

/// Whether a layer holds keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerState {
    /// All sealing requirements hold, so keys exist.
    Sealed,
    /// At least one requirement failed, so no keys exist: grid plus Track 0.
    Unsealed,
}

impl SealingChecks {
    /// Keys exist only if every requirement holds.
    #[must_use]
    pub fn layer_state(self) -> LayerState {
        let Self {
            per_layer_key,
            separate_holder,
            key_delivery,
            memory_hygiene,
            local_session,
            policy_file,
            side_channel,
        } = self;
        if per_layer_key
            && separate_holder
            && key_delivery
            && memory_hygiene
            && local_session
            && policy_file
            && side_channel
        {
            LayerState::Sealed
        } else {
            LayerState::Unsealed
        }
    }
}

/// Where a rich block came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A frame whose header and AEAD verified under the layer's keys.
    VerifiedSealedFrame,
    /// An unsealed Track 0 frame, which any program may write.
    Track0,
}

/// How the terminal draws a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rendering {
    /// Drawn as a rich block from a verified sealed frame.
    Rich,
    /// Drawn as a rich block inside the unverified output container, with no
    /// authority. This is what Track 0 gets on a sealed layer.
    RichUnverified,
    /// Drawn as escaped literal text.
    EscapedText,
}

/// Arm A's render rule (comparison criteria, section 1): rich blocks only on
/// a sealed layer. A verified sealed frame is `Rich`; Track 0 stays inside
/// the unverified output container (`RichUnverified`). Anything on an unsealed
/// layer is escaped text. A sealed-looking frame on an unsealed layer cannot
/// verify, so it is escaped text too.
#[must_use]
pub fn rendering(layer: LayerState, source: Source) -> Rendering {
    match (layer, source) {
        (LayerState::Sealed, Source::VerifiedSealedFrame) => Rendering::Rich,
        (LayerState::Sealed, Source::Track0) => Rendering::RichUnverified,
        (LayerState::Unsealed, _) => Rendering::EscapedText,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_OK: SealingChecks = SealingChecks {
        per_layer_key: true,
        separate_holder: true,
        key_delivery: true,
        memory_hygiene: true,
        local_session: true,
        policy_file: true,
        side_channel: true,
    };

    #[test]
    fn all_requirements_seal() {
        assert_eq!(ALL_OK.layer_state(), LayerState::Sealed);
    }

    #[test]
    fn each_failed_requirement_removes_keys() {
        let broken = [
            SealingChecks {
                per_layer_key: false,
                ..ALL_OK
            },
            SealingChecks {
                separate_holder: false,
                ..ALL_OK
            },
            SealingChecks {
                key_delivery: false,
                ..ALL_OK
            },
            SealingChecks {
                memory_hygiene: false,
                ..ALL_OK
            },
            SealingChecks {
                local_session: false,
                ..ALL_OK
            },
            SealingChecks {
                policy_file: false,
                ..ALL_OK
            },
            SealingChecks {
                side_channel: false,
                ..ALL_OK
            },
        ];
        for checks in broken {
            assert_eq!(checks.layer_state(), LayerState::Unsealed, "{checks:?}");
        }
    }

    #[test]
    fn nothing_is_rich_on_an_unsealed_layer() {
        for source in [Source::Track0, Source::VerifiedSealedFrame] {
            assert_eq!(
                rendering(LayerState::Unsealed, source),
                Rendering::EscapedText,
                "{source:?}"
            );
        }
    }

    #[test]
    fn sealed_frame_is_rich_on_a_sealed_layer() {
        assert_eq!(
            rendering(LayerState::Sealed, Source::VerifiedSealedFrame),
            Rendering::Rich
        );
    }

    #[test]
    fn track0_on_a_sealed_layer_stays_in_the_unverified_container() {
        assert_eq!(
            rendering(LayerState::Sealed, Source::Track0),
            Rendering::RichUnverified
        );
    }
}
