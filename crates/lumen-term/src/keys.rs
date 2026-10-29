//! Keys to the bytes a program on a PTY expects (xterm style).
//!
//! The window turns its own key events into a [`Key`] and [`Mods`]; this turns
//! them into bytes. It has no dependency on a window, so it is tested on its own.
//! Cursor keys are sent in normal mode: application cursor mode is not tracked.

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

/// The bytes to send for `key` with `mods` held. Empty for a key with no bytes.
#[must_use]
pub fn bytes(key: Key, mods: Mods) -> Vec<u8> {
    let out = match key {
        Key::Char(c) => return char_bytes(c, mods),
        Key::Enter => vec![b'\r'],
        Key::Backspace => vec![0x7f],
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::Escape => vec![0x1b],
        Key::Up => csi(b'A', mods),
        Key::Down => csi(b'B', mods),
        Key::Right => csi(b'C', mods),
        Key::Left => csi(b'D', mods),
        Key::Home => csi(b'H', mods),
        Key::End => csi(b'F', mods),
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
    if mods.alt && matches!(key, Key::Enter | Key::Backspace | Key::Tab | Key::Escape) {
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
