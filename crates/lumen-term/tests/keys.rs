//! Keys to bytes.

use lumen_term::keys::{bytes, bytes_in};
use lumen_term::{Key, KeyModes, Mods};

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

const CURSOR_APP: KeyModes = KeyModes {
    cursor_app: true,
    keypad_app: false,
};
const KEYPAD_APP: KeyModes = KeyModes {
    cursor_app: false,
    keypad_app: true,
};

#[test]
fn application_cursor_keys_use_ss3_until_a_modifier() {
    for (key, plain, app) in [
        (Key::Up, &b"\x1b[A"[..], &b"\x1bOA"[..]),
        (Key::Down, b"\x1b[B", b"\x1bOB"),
        (Key::Right, b"\x1b[C", b"\x1bOC"),
        (Key::Left, b"\x1b[D", b"\x1bOD"),
        (Key::Home, b"\x1b[H", b"\x1bOH"),
        (Key::End, b"\x1b[F", b"\x1bOF"),
    ] {
        assert_eq!(
            bytes_in(key, NONE, KeyModes::default()),
            plain,
            "{key:?} off"
        );
        assert_eq!(bytes_in(key, NONE, CURSOR_APP), app, "{key:?} on");
    }
    // A modifier keeps the CSI form in both modes.
    assert_eq!(bytes_in(Key::Left, CTRL, CURSOR_APP), b"\x1b[1;5D");
    assert_eq!(bytes_in(Key::Up, SHIFT, CURSOR_APP), b"\x1b[1;2A");
    // The editing keys and the keypad mode do not move with DECCKM.
    assert_eq!(bytes_in(Key::Delete, NONE, CURSOR_APP), b"\x1b[3~");
    assert_eq!(bytes_in(Key::PageUp, NONE, CURSOR_APP), b"\x1b[5~");
    assert_eq!(bytes_in(Key::Up, NONE, KEYPAD_APP), b"\x1b[A");
}

#[test]
fn keypad_keys_in_both_modes() {
    for (c, app) in [
        ('0', b'p'),
        ('5', b'u'),
        ('9', b'y'),
        ('*', b'j'),
        ('+', b'k'),
        (',', b'l'),
        ('-', b'm'),
        ('.', b'n'),
        ('/', b'o'),
        ('=', b'X'),
    ] {
        let mut want = c.to_string().into_bytes();
        assert_eq!(bytes_in(Key::Keypad(c), NONE, KeyModes::default()), want);
        want = vec![0x1b, b'O', app];
        assert_eq!(bytes_in(Key::Keypad(c), NONE, KEYPAD_APP), want, "{c}");
        // Application cursor keys do not change the keypad.
        assert_eq!(
            bytes_in(Key::Keypad(c), NONE, CURSOR_APP),
            c.to_string().into_bytes()
        );
    }
    assert_eq!(bytes_in(Key::KeypadEnter, NONE, KeyModes::default()), b"\r");
    assert_eq!(bytes_in(Key::KeypadEnter, NONE, KEYPAD_APP), b"\x1bOM");
    assert_eq!(
        bytes_in(Key::KeypadEnter, ALT, KeyModes::default()),
        b"\x1b\r"
    );
    assert_eq!(bytes_in(Key::Keypad('5'), CTRL, KEYPAD_APP), b"\x1bO5u");
}
