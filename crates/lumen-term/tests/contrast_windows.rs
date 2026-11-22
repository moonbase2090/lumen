//! On a real Windows machine the system's answer about High Contrast can be
//! read: the registry program starts, its output is the flags value and the low
//! bit is read. A clean runner has no High Contrast theme on.

#![cfg(windows)]

#[test]
fn the_system_answers_about_high_contrast_and_a_clean_machine_has_it_off() {
    let answer = lumen_term::contrast::system_increases_contrast();
    println!("system_increases_contrast() = {answer:?}");
    assert_eq!(answer, Some(false));
}
