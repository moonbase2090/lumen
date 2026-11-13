//! The input modes a program has switched on: application cursor keys
//! (DECCKM, `CSI ? 1 h`) and the application keypad (DECKPAM `ESC =`,
//! DECKPNM `ESC >`).
//!
//! The tracker reads the bytes the grid is given, after isolation, so it sees
//! what the emulator sees. It is kept apart from the grid blocks: a mode a
//! program set stays set when a rich block ends one grid block and a new one
//! starts. A soft reset (`CSI ! p`) and a full reset (`ESC c`) clear both modes.

/// The modes that change which bytes a key sends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModes {
    /// DECCKM: unmodified cursor keys send `ESC O x` instead of `ESC [ x`.
    pub cursor_app: bool,
    /// DECKPAM: keypad keys send `ESC O x` instead of the plain character.
    pub keypad_app: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    /// Inside `CSI`, before the final byte.
    Csi,
    /// Inside a string (OSC, DCS and the like), until BEL or ESC.
    Str,
}

/// Longest run of CSI parameter bytes kept; a longer one is not a mode change.
const MAX_PARAMS: usize = 64;

/// Follows a program's output for [`KeyModes`].
#[derive(Debug, Clone)]
pub struct ModeTracker {
    modes: KeyModes,
    state: State,
    params: Vec<u8>,
    overlong: bool,
}

impl Default for ModeTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ModeTracker {
    /// A tracker with both modes off.
    #[must_use]
    pub fn new() -> Self {
        Self {
            modes: KeyModes::default(),
            state: State::Ground,
            params: Vec::new(),
            overlong: false,
        }
    }

    /// The modes now.
    #[must_use]
    pub fn modes(&self) -> KeyModes {
        self.modes
    }

    /// Read `bytes` of program output.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.step(b);
        }
    }

    fn step(&mut self, b: u8) {
        match self.state {
            State::Ground => {
                if b == 0x1b {
                    self.state = State::Esc;
                }
            }
            State::Esc => {
                self.state = State::Ground;
                match b {
                    b'=' => self.modes.keypad_app = true,
                    b'>' => self.modes.keypad_app = false,
                    b'c' => self.modes = KeyModes::default(),
                    b'[' => {
                        self.params.clear();
                        self.overlong = false;
                        self.state = State::Csi;
                    }
                    b']' | b'P' | b'_' | b'^' | b'X' => self.state = State::Str,
                    0x1b => self.state = State::Esc,
                    _ => {}
                }
            }
            State::Csi => match b {
                0x1b => self.state = State::Esc,
                0x20..=0x3f => {
                    if self.params.len() < MAX_PARAMS {
                        self.params.push(b);
                    } else {
                        self.overlong = true;
                    }
                }
                0x40..=0x7e => {
                    self.state = State::Ground;
                    self.csi(b);
                }
                // A control or a byte outside the sequence ends it unread.
                _ => self.state = State::Ground,
            },
            State::Str => match b {
                0x07 => self.state = State::Ground,
                0x1b => self.state = State::Esc,
                _ => {}
            },
        }
    }

    fn csi(&mut self, final_byte: u8) {
        if self.overlong {
            return;
        }
        match (self.params.as_slice(), final_byte) {
            ([b'!'], b'p') => self.modes = KeyModes::default(),
            ([b'?', rest @ ..], b'h' | b'l')
                if rest.iter().all(|c| c.is_ascii_digit() || *c == b';')
                    && rest.split(|c| *c == b';').any(|p| p == b"1") =>
            {
                self.modes.cursor_app = final_byte == b'h';
            }
            _ => {}
        }
    }
}
