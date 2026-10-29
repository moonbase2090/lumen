//! Keys to bytes.

use lumen_term::keys::bytes;
use lumen_term::{Key, Mods};

const NONE: Mods = Mods {
    ctrl: false,
    alt: false,
    shift: false,
};
const CTRL: Mods = Mods { ctrl: true, ..NONE };
const ALT: Mods = Mods { alt: true, ..NONE };
const SHIFT: Mods = Mods {
    shift: true,
    ..NONE
};

#[test]
fn characters_are_their_utf8() {
    assert_eq!(bytes(Key::Char('a'), NONE), b"a");
    assert_eq!(bytes(Key::Char('Z'), SHIFT), b"Z");
    assert_eq!(bytes(Key::Char('\u{e9}'), NONE), "\u{e9}".as_bytes());
    assert_eq!(bytes(Key::Char('\u{1f600}'), NONE), "\u{1f600}".as_bytes());
}

#[test]
fn control_letters_are_the_control_codes() {
    assert_eq!(bytes(Key::Char('a'), CTRL), [1]);
    assert_eq!(bytes(Key::Char('c'), CTRL), [3]);
    assert_eq!(bytes(Key::Char('Z'), CTRL), [26]);
    assert_eq!(bytes(Key::Char('['), CTRL), [0x1b]);
    assert_eq!(bytes(Key::Char(' '), CTRL), [0]);
    assert_eq!(bytes(Key::Char('/'), CTRL), [0x1f]);
    assert_eq!(bytes(Key::Char('?'), CTRL), [0x7f]);
}

#[test]
fn alt_is_an_escape_prefix() {
    assert_eq!(bytes(Key::Char('b'), ALT), b"\x1bb");
    assert_eq!(
        bytes(
            Key::Char('b'),
            Mods {
                ctrl: true,
                alt: true,
                shift: false
            }
        ),
        [0x1b, 2]
    );
    assert_eq!(bytes(Key::Enter, ALT), b"\x1b\r");
    assert_eq!(bytes(Key::Backspace, ALT), [0x1b, 0x7f]);
}

#[test]
fn the_plain_keys() {
    assert_eq!(bytes(Key::Enter, NONE), b"\r");
    assert_eq!(bytes(Key::Backspace, NONE), [0x7f]);
    assert_eq!(bytes(Key::Tab, NONE), b"\t");
    assert_eq!(bytes(Key::Tab, SHIFT), b"\x1b[Z");
    assert_eq!(bytes(Key::Escape, NONE), [0x1b]);
}

#[test]
fn cursor_keys_and_their_modifiers() {
    assert_eq!(bytes(Key::Up, NONE), b"\x1b[A");
    assert_eq!(bytes(Key::Down, NONE), b"\x1b[B");
    assert_eq!(bytes(Key::Right, NONE), b"\x1b[C");
    assert_eq!(bytes(Key::Left, NONE), b"\x1b[D");
    assert_eq!(bytes(Key::Home, NONE), b"\x1b[H");
    assert_eq!(bytes(Key::End, NONE), b"\x1b[F");
    assert_eq!(bytes(Key::Up, SHIFT), b"\x1b[1;2A");
    assert_eq!(bytes(Key::Left, ALT), b"\x1b[1;3D");
    assert_eq!(bytes(Key::Right, CTRL), b"\x1b[1;5C");
    assert_eq!(
        bytes(
            Key::Right,
            Mods {
                ctrl: true,
                alt: true,
                shift: true
            }
        ),
        b"\x1b[1;8C"
    );
}

#[test]
fn editing_keys() {
    assert_eq!(bytes(Key::Insert, NONE), b"\x1b[2~");
    assert_eq!(bytes(Key::Delete, NONE), b"\x1b[3~");
    assert_eq!(bytes(Key::PageUp, NONE), b"\x1b[5~");
    assert_eq!(bytes(Key::PageDown, NONE), b"\x1b[6~");
    assert_eq!(bytes(Key::Delete, CTRL), b"\x1b[3;5~");
}

#[test]
fn function_keys() {
    assert_eq!(bytes(Key::F(1), NONE), b"\x1bOP");
    assert_eq!(bytes(Key::F(4), NONE), b"\x1bOS");
    assert_eq!(bytes(Key::F(1), SHIFT), b"\x1b[1;2P");
    assert_eq!(bytes(Key::F(5), NONE), b"\x1b[15~");
    assert_eq!(bytes(Key::F(10), NONE), b"\x1b[21~");
    assert_eq!(bytes(Key::F(11), NONE), b"\x1b[23~");
    assert_eq!(bytes(Key::F(12), NONE), b"\x1b[24~");
    assert_eq!(bytes(Key::F(5), CTRL), b"\x1b[15;5~");
    assert_eq!(bytes(Key::F(0), NONE), Vec::<u8>::new());
    assert_eq!(bytes(Key::F(13), NONE), Vec::<u8>::new());
}
