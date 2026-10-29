//! The unlock rule: images are shown only when both build identities have a
//! passing gate record on this platform, and the setting allows it.

#![allow(missing_docs)]

use lumen_decoder::unlock::{GateRecord, Identities, images_enabled};

fn ids(d: &str, p: &str) -> Identities {
    Identities {
        decoder_build: d.into(),
        image_path_build: p.into(),
    }
}

fn record(platform: &str, d: &str, p: &str) -> GateRecord {
    GateRecord {
        platform: platform.into(),
        decoder_build: d.into(),
        image_path_build: p.into(),
        decoder_gate_passed: true,
        memory_gate_passed: true,
    }
}

#[test]
fn both_identities_with_both_gates_on_this_platform_unlock_images() {
    assert!(images_enabled(
        true,
        "linux",
        &ids("d1", "p1"),
        &[record("linux", "d1", "p1")]
    ));
}

#[test]
fn the_local_setting_off_always_wins() {
    assert!(!images_enabled(
        false,
        "linux",
        &ids("d1", "p1"),
        &[record("linux", "d1", "p1")]
    ));
}

#[test]
fn a_changed_decoder_identity_shows_alt_text() {
    assert!(!images_enabled(
        true,
        "linux",
        &ids("d2", "p1"),
        &[record("linux", "d1", "p1")]
    ));
}

#[test]
fn a_changed_image_path_identity_shows_alt_text() {
    assert!(!images_enabled(
        true,
        "linux",
        &ids("d1", "p2"),
        &[record("linux", "d1", "p1")]
    ));
}

#[test]
fn a_record_that_matches_only_one_identity_does_not_unlock() {
    // Passed for the decoder as built, but the image path is a build that never passed.
    let records = [record("linux", "d1", "p-old")];
    assert!(!images_enabled(
        true,
        "linux",
        &ids("d1", "p-new"),
        &records
    ));
    // The other way round.
    let records = [record("linux", "d-old", "p1")];
    assert!(!images_enabled(
        true,
        "linux",
        &ids("d-new", "p1"),
        &records
    ));
    // Two records, each matching one identity, are still not a match for the pair.
    let records = [
        record("linux", "d1", "p-old"),
        record("linux", "d-old", "p1"),
    ];
    assert!(!images_enabled(true, "linux", &ids("d1", "p1"), &records));
}

#[test]
fn both_gates_are_needed() {
    let mut r = record("linux", "d1", "p1");
    r.memory_gate_passed = false;
    assert!(!images_enabled(true, "linux", &ids("d1", "p1"), &[r]));
    let mut r = record("linux", "d1", "p1");
    r.decoder_gate_passed = false;
    assert!(!images_enabled(true, "linux", &ids("d1", "p1"), &[r]));
}

#[test]
fn a_record_for_another_platform_does_not_count() {
    assert!(!images_enabled(
        true,
        "macos",
        &ids("d1", "p1"),
        &[record("linux", "d1", "p1")]
    ));
}

#[test]
fn no_record_means_alt_text() {
    assert!(!images_enabled(true, "linux", &ids("d1", "p1"), &[]));
}

#[test]
fn an_identity_that_stands_for_none_never_unlocks() {
    // A ledger written before an image path existed says "absent"; a record made
    // of two equal "absent" strings must not match two equal "absent" strings.
    let a = "absent: no terminal image path exists yet";
    assert!(!images_enabled(
        true,
        "linux",
        &ids("d1", a),
        &[record("linux", "d1", a)]
    ));
    assert!(!images_enabled(
        true,
        "linux",
        &ids("", "p1"),
        &[record("linux", "", "p1")]
    ));
    assert!(!images_enabled(
        true,
        "linux",
        &ids("d1", ""),
        &[record("linux", "d1", "")]
    ));
}

#[test]
fn any_one_of_several_records_may_match() {
    let records = [
        record("linux", "d0", "p0"),
        record("linux", "d1", "p1"),
        record("macos", "d1", "p1"),
    ];
    assert!(images_enabled(true, "linux", &ids("d1", "p1"), &records));
}
