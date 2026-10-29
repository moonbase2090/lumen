//! What turns a window event into a key, and a frame into pixels. No window is
//! opened.

#![cfg(feature = "window")]

use lumen_term::host::{map_key, to_xrgb};
use lumen_term::{Frame, Key, Mods};
use winit::keyboard::{Key as W, ModifiersState, NamedKey};

fn none() -> ModifiersState {
    ModifiersState::empty()
}

fn ch(s: &str) -> W {
    W::Character(s.into())
}

#[test]
fn named_keys_map_to_their_keys() {
    let plain = Mods::default();
    for (named, key) in [
        (NamedKey::Enter, Key::Enter),
        (NamedKey::Backspace, Key::Backspace),
        (NamedKey::Tab, Key::Tab),
        (NamedKey::Escape, Key::Escape),
        (NamedKey::Space, Key::Char(' ')),
        (NamedKey::ArrowUp, Key::Up),
        (NamedKey::ArrowDown, Key::Down),
        (NamedKey::ArrowLeft, Key::Left),
        (NamedKey::ArrowRight, Key::Right),
        (NamedKey::Home, Key::Home),
        (NamedKey::End, Key::End),
        (NamedKey::PageUp, Key::PageUp),
        (NamedKey::PageDown, Key::PageDown),
        (NamedKey::Insert, Key::Insert),
        (NamedKey::Delete, Key::Delete),
        (NamedKey::F1, Key::F(1)),
        (NamedKey::F5, Key::F(5)),
        (NamedKey::F12, Key::F(12)),
    ] {
        assert_eq!(map_key(&W::Named(named), None, none()), Some((key, plain)));
    }
    assert_eq!(map_key(&W::Named(NamedKey::CapsLock), None, none()), None);
    assert_eq!(map_key(&W::Named(NamedKey::Shift), None, none()), None);
}

#[test]
fn a_character_uses_the_text_the_key_produced() {
    // Shift and the layout are already in the text.
    assert_eq!(
        map_key(&ch("a"), Some("A"), ModifiersState::SHIFT),
        Some((
            Key::Char('A'),
            Mods {
                shift: true,
                ..Mods::default()
            }
        ))
    );
    assert_eq!(
        map_key(&ch("e"), Some("\u{e9}"), none()),
        Some((Key::Char('\u{e9}'), Mods::default()))
    );
    assert_eq!(
        map_key(&ch("x"), None, none()),
        Some((Key::Char('x'), Mods::default())),
        "with no text the logical key is used"
    );
}

#[test]
fn control_uses_the_logical_key_not_the_control_code_in_the_text() {
    let got = map_key(&ch("c"), Some("\u{3}"), ModifiersState::CONTROL);
    assert_eq!(
        got,
        Some((
            Key::Char('c'),
            Mods {
                ctrl: true,
                ..Mods::default()
            }
        ))
    );
}

#[test]
fn alt_and_shift_tab_carry_their_modifiers() {
    assert_eq!(
        map_key(&ch("b"), Some("b"), ModifiersState::ALT),
        Some((
            Key::Char('b'),
            Mods {
                alt: true,
                ..Mods::default()
            }
        ))
    );
    assert_eq!(
        map_key(&W::Named(NamedKey::Tab), None, ModifiersState::SHIFT),
        Some((
            Key::Tab,
            Mods {
                shift: true,
                ..Mods::default()
            }
        ))
    );
}

#[test]
fn super_belongs_to_the_operating_system() {
    assert_eq!(map_key(&ch("c"), Some("c"), ModifiersState::SUPER), None);
    assert_eq!(
        map_key(&W::Named(NamedKey::Enter), None, ModifiersState::SUPER),
        None
    );
}

#[test]
fn a_composed_string_is_not_a_key() {
    assert_eq!(map_key(&ch("a"), Some("ab"), none()), None);
    assert_eq!(
        map_key(&ch("e"), Some(""), none()),
        Some((Key::Char('e'), Mods::default()))
    );
}

#[test]
fn a_frame_becomes_xrgb_words() {
    let mut f = Frame::new(2, 1, [1, 2, 3]);
    f.fill(1, 0, 1, 1, [255, 128, 0]);
    assert_eq!(to_xrgb(&f), [0x0001_0203, 0x00ff_8000]);
}
