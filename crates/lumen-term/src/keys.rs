//! Keys to the bytes a program on a PTY expects (xterm style).
//!
//! The window turns its own key events into a [`Key`] and [`Mods`]; this turns
//! them into bytes. It has no dependency on a window, so it is tested on its own.
//! Application cursor keys (DECCKM) and the application keypad (DECKPAM) change
//! the bytes of the keys they cover: [`bytes_in`] takes the modes the program
//! has set, and [`bytes`] is the same with both off.

use crate::modes::KeyModes;

/// A key that is not just a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character, as typed (with shift already applied).
    Char(char),
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Tab.
    Tab,
    /// Escape.
    Escape,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Right arrow.
    Right,
    /// Left arrow.
    Left,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Insert.
    Insert,
    /// Delete.
    Delete,
    /// A function key, 1 to 12.
    F(u8),
    /// A key on the numeric keypad that types `0`-`9`, `*`, `+`, `,`, `-`, `.`,
    /// `/` or `=`. Any other character is sent as typed.
    Keypad(char),
    /// Enter on the numeric keypad.
    KeypadEnter,
}

/// Modifier keys held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    /// Control.
    pub ctrl: bool,
    /// Alt or Option.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
}

/// The xterm modifier parameter: 1 + shift (1) + alt (2) + ctrl (4).
fn param(m: Mods) -> u8 {
    1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl)
}

fn csi(final_byte: u8, m: Mods) -> Vec<u8> {
    if param(m) == 1 {
        vec![0x1b, b'[', final_byte]
    } else {
        format!("\x1b[1;{}{}", param(m), char::from(final_byte)).into_bytes()
    }
}

fn tilde(n: u8, m: Mods) -> Vec<u8> {
    if param(m) == 1 {
        format!("\x1b[{n}~").into_bytes()
    } else {
        format!("\x1b[{n};{}~", param(m)).into_bytes()
    }
}

/// `ESC O` then the final byte, with the modifier parameter between them when
/// one is held (xterm's form for SS3 keys).
fn ss3(final_byte: u8, m: Mods) -> Vec<u8> {
    if param(m) == 1 {
        vec![0x1b, b'O', final_byte]
    } else {
        format!("\x1bO{}{}", param(m), char::from(final_byte)).into_bytes()
    }
}

/// The final byte of a keypad key in application mode.
fn keypad_final(c: char) -> Option<u8> {
    Some(match c {
        '0'..='9' => b'p' + (c as u8 - b'0'),
        '*' => b'j',
        '+' => b'k',
        ',' => b'l',
        '-' => b'm',
        '.' => b'n',
        '/' => b'o',
        '=' => b'X',
        _ => return None,
    })
}

/// A cursor key: `ESC O x` in application mode when no modifier is held,
/// otherwise `CSI x` or `CSI 1 ; mod x`.
fn cursor(final_byte: u8, m: Mods, app: bool) -> Vec<u8> {
    if app && param(m) == 1 {
        ss3(final_byte, m)
    } else {
        csi(final_byte, m)
    }
}

/// The bytes to send for `key` with `mods` held, with both application modes
/// off. Empty for a key with no bytes.
#[must_use]
pub fn bytes(key: Key, mods: Mods) -> Vec<u8> {
    bytes_in(key, mods, KeyModes::default())
}

/// The bytes to send for `key` with `mods` held while the program has set
/// the application modes `app`. Empty for a key with no bytes.
#[must_use]
pub fn bytes_in(key: Key, mods: Mods, app: KeyModes) -> Vec<u8> {
    let out = match key {
        Key::Char(c) => return char_bytes(c, mods),
        Key::Keypad(c) => {
            return match keypad_final(c).filter(|_| app.keypad_app) {
                Some(f) => ss3(f, mods),
                None => char_bytes(c, mods),
            };
        }
        Key::KeypadEnter if app.keypad_app => return ss3(b'M', mods),
        Key::Enter | Key::KeypadEnter => vec![b'\r'],
        Key::Backspace => vec![0x7f],
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::Escape => vec![0x1b],
        Key::Up => cursor(b'A', mods, app.cursor_app),
        Key::Down => cursor(b'B', mods, app.cursor_app),
        Key::Right => cursor(b'C', mods, app.cursor_app),
        Key::Left => cursor(b'D', mods, app.cursor_app),
        Key::Home => cursor(b'H', mods, app.cursor_app),
        Key::End => cursor(b'F', mods, app.cursor_app),
        Key::PageUp => tilde(5, mods),
        Key::PageDown => tilde(6, mods),
        Key::Insert => tilde(2, mods),
        Key::Delete => tilde(3, mods),
        Key::F(n @ 1..=4) => {
            let final_byte = b'P' + (n - 1);
            if param(mods) == 1 {
                vec![0x1b, b'O', final_byte]
            } else {
                format!("\x1b[1;{}{}", param(mods), char::from(final_byte)).into_bytes()
            }
        }
        Key::F(n @ 5..=12) => {
            let code = match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                _ => 24,
            };
            tilde(code, mods)
        }
        Key::F(_) => Vec::new(),
    };
    // Alt on a key that sends a plain byte is an ESC prefix; the sequences above
    // carry their modifier in a parameter instead.
    if mods.alt
        && matches!(
            key,
            Key::Enter | Key::KeypadEnter | Key::Backspace | Key::Tab | Key::Escape
        )
    {
        let mut prefixed = vec![0x1b];
        prefixed.extend(out);
        return prefixed;
    }
    out
}

fn char_bytes(c: char, mods: Mods) -> Vec<u8> {
    let mut out = Vec::new();
    if mods.alt {
        out.push(0x1b);
    }
    if mods.ctrl {
        match c {
            'a'..='z' => out.push(c as u8 - b'a' + 1),
            'A'..='Z' => out.push(c as u8 - b'A' + 1),
            '@' | ' ' | '2' => out.push(0),
            '[' | '3' => out.push(0x1b),
            '\\' | '4' => out.push(0x1c),
            ']' | '5' => out.push(0x1d),
            '^' | '6' => out.push(0x1e),
            '_' | '7' | '/' => out.push(0x1f),
            '8' | '?' => out.push(0x7f),
            _ => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    } else {
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
    out
}
