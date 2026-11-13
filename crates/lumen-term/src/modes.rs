//! The input modes a program has switched on: application cursor keys
//! (DECCKM, `CSI ? 1 h`) and the application keypad (DECKPAM `ESC =`,
//! DECKPNM `ESC >`).
//!
//! Each grid block has a tracker that reads the bytes the block is given, after
//! isolation, so it sees what the emulator sees. A tracker records only what
//! its own block changed ([`Changes`]); the session folds the blocks in order,
//! so a mode a program set stays set when a rich block ends one grid block and a
//! new one starts. A soft reset (`CSI ! p`) and a full reset (`ESC c`) clear
//! both modes.

/// The modes that change which bytes a key sends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModes {
    /// DECCKM: unmodified cursor keys send `ESC O x` instead of `ESC [ x`.
    pub cursor_app: bool,
    /// DECKPAM: keypad keys send `ESC O x` instead of the plain character.
    pub keypad_app: bool,
}

/// What one stretch of output did to the modes: `None` is left as it was.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changes {
    cursor_app: Option<bool>,
    keypad_app: Option<bool>,
}

impl Changes {
    /// `base` with these changes made.
    #[must_use]
    pub fn apply(self, base: KeyModes) -> KeyModes {
        KeyModes {
            cursor_app: self.cursor_app.unwrap_or(base.cursor_app),
            keypad_app: self.keypad_app.unwrap_or(base.keypad_app),
        }
    }

    fn reset() -> Self {
        Self {
            cursor_app: Some(false),
            keypad_app: Some(false),
        }
    }
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
    changes: Changes,
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
            changes: Changes::default(),
            state: State::Ground,
            params: Vec::new(),
            overlong: false,
        }
    }

    /// What the output read so far changed.
    #[must_use]
    pub fn changes(&self) -> Changes {
        self.changes
    }

    /// The modes after the output read so far, starting from both off.
    #[must_use]
    pub fn modes(&self) -> KeyModes {
        self.changes.apply(KeyModes::default())
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
                    b'=' => self.changes.keypad_app = Some(true),
                    b'>' => self.changes.keypad_app = Some(false),
                    b'c' => self.changes = Changes::reset(),
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
            ([b'!'], b'p') => self.changes = Changes::reset(),
            ([b'?', rest @ ..], b'h' | b'l')
                if rest.iter().all(|c| c.is_ascii_digit() || *c == b';')
                    && rest.split(|c| *c == b';').any(|p| p == b"1") =>
            {
                self.changes.cursor_app = Some(final_byte == b'h');
            }
            _ => {}
        }
    }
}

/// The modes after all of a document's blocks: each grid block's changes in
/// order, so a mode stays set across a rich block between two grid blocks.
#[must_use]
pub fn key_modes_of(blocks: &[crate::document::Block]) -> KeyModes {
    blocks
        .iter()
        .fold(KeyModes::default(), |modes, block| match block {
            crate::document::Block::Grid(g) => g.key_mode_changes().apply(modes),
            crate::document::Block::Rich(_) => modes,
        })
}
