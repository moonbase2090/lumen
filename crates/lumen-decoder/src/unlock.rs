//! When images may be shown: the platform unlock rule of the spec
//! ([images.unlock]).
//!
//! Images are enabled only after the decoder gate and the decoder memory gate
//! have passed on the platform, for the decoder build identity **and** for the
//! terminal's image-path build identity. Any change to an identity means the
//! gate has not passed for the new build, and the alt text is shown until it
//! has. The local setting `images = off` always wins, and there is no remote
//! switch: the only inputs are the setting, the build and the gate records the
//! build was shipped with.

/// The two build identities of the running terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identities {
    /// Hash of the decoder's sources, compiler, flags and dependency closure.
    pub decoder_build: String,
    /// The same for the terminal's image path.
    pub image_path_build: String,
}

/// A recorded gate result for one platform and one pair of identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateRecord {
    /// The platform the gates were run on.
    pub platform: String,
    /// The decoder identity the gates passed for.
    pub decoder_build: String,
    /// The image-path identity the gates passed for.
    pub image_path_build: String,
    /// The decoder gate passed.
    pub decoder_gate_passed: bool,
    /// The decoder memory gate passed.
    pub memory_gate_passed: bool,
}

/// An identity that stands for "none": a build that could not compute one, or a
/// ledger that says the image path does not exist yet. It never unlocks.
fn usable(id: &str) -> bool {
    !id.is_empty() && !id.starts_with("absent")
}

/// Whether images are shown (`true`) or the alt text is (`false`).
#[must_use]
pub fn images_enabled(
    setting_on: bool,
    platform: &str,
    current: &Identities,
    records: &[GateRecord],
) -> bool {
    setting_on
        && usable(&current.decoder_build)
        && usable(&current.image_path_build)
        && records.iter().any(|r| {
            r.platform == platform
                && r.decoder_gate_passed
                && r.memory_gate_passed
                && r.decoder_build == current.decoder_build
                && r.image_path_build == current.image_path_build
        })
}
