//! The input line: where a key goes, the box or the program.
//!
//! The box at the bottom of the window is either an editor, whose keys land in
//! it, or a one-row status that says where keys go. [`InputLine::mode`] decides
//! which from [`Facts`] about the PTY and the grid; nothing a program prints is
//! consulted. Anything unknown means [`Mode::Raw`]: keys go to the program, so a
//! failed query never swallows a key.
//!
//! Keys the box does not edit with are handed to the program at once, after
//! the box's text, in one write, so a control key is never swallowed.
//! The routing here returns the bytes to write; it neither draws nor writes.

use crate::keys::{Key, Mods, bytes_in};
use crate::modes::KeyModes;
use crate::paste::{Decision, judge};

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";
/// Most lines the history keeps.
const HISTORY_LIMIT: usize = 200;

/// What is known about the PTY and the grid at this moment. `None` is a query
/// that failed or could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    /// The PTY's foreground process group is the shell that was spawned.
    pub shell_in_front: Option<bool>,
    /// The tty's ICANON flag.
    pub icanon: Option<bool>,
    /// The tty's ECHO flag.
    pub echo: Option<bool>,
    /// The grid is on the alternate screen.
    pub alt_screen: bool,
    /// DECCKM and the application keypad.
    pub keys: KeyModes,
    /// DECSET 2004 is on.
    pub bracketed_paste: bool,
    /// The program has ended.
    pub ended: bool,
}

impl Facts {
    /// Facts for a shell sitting at a prompt in its line editor.
    #[must_use]
    pub const fn at_prompt() -> Self {
        Self {
            shell_in_front: Some(true),
            icanon: Some(false),
            echo: Some(false),
            alt_screen: false,
            keys: KeyModes {
                cursor_app: false,
                keypad_app: false,
            },
            bracketed_paste: false,
            ended: false,
        }
    }
}

/// Where keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Keys edit the box.
    Editing,
    /// Keys go to the program: a full-screen program, application key modes,
    /// another program in front, no readable tty state, or typeahead.
    Raw,
    /// Keys go to the program, which is reading without echo (a password).
    Hidden,
    /// A key was handed to the shell, so the shell holds the line until it ends.
    HandedOff,
    /// The program has ended.
    Ended,
}

/// What a key or a paste did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to do.
    None,
    /// The box changed; draw it again.
    Redraw,
    /// Write these bytes to the program.
    Write(Vec<u8>),
    /// A multi-line line is held for a choice (the shell does not take
    /// bracketed paste): send as commands, join, or cancel.
    Hold,
    /// The paste was refused: it carried a frame introducer.
    Refused,
    /// Not the box's to handle; the caller does what it did before the box.
    NotMine,
}

/// The box's text and what it needs to route keys.
#[derive(Debug, Clone, Default)]
pub struct InputLine {
    text: Vec<char>,
    caret: usize,
    handed_off: bool,
    typeahead: bool,
    held: Option<String>,
    history: Vec<String>,
    history_at: Option<usize>,
}

impl InputLine {
    /// An empty box.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The box's text.
    #[must_use]
    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    /// The caret, as a character index into [`text`](Self::text).
    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The held multi-line text, while a choice is pending.
    #[must_use]
    pub fn held(&self) -> Option<&str> {
        self.held.as_deref()
    }

    /// Where keys go now.
    #[must_use]
    pub fn mode(&self, facts: Facts) -> Mode {
        if facts.ended {
            return Mode::Ended;
        }
        if facts.alt_screen || facts.keys.cursor_app || facts.keys.keypad_app {
            return Mode::Raw;
        }
        if facts.icanon == Some(true) && facts.echo == Some(false) {
            return Mode::Hidden;
        }
        if facts.shell_in_front != Some(true) || facts.icanon != Some(false) {
            return Mode::Raw;
        }
        if self.handed_off {
            return Mode::HandedOff;
        }
        if self.typeahead {
            return Mode::Raw;
        }
        Mode::Editing
    }

    /// A key.
    pub fn key(&mut self, facts: Facts, key: Key, mods: Mods) -> Outcome {
        match self.mode(facts) {
            Mode::Editing => self.edit_key(facts, key, mods),
            Mode::Raw | Mode::Hidden | Mode::HandedOff | Mode::Ended => {
                self.raw_key(facts, key, mods)
            }
        }
    }

    /// Text typed or committed as a whole (an input method's result).
    pub fn text_input(&mut self, facts: Facts, text: &str) -> Outcome {
        if self.mode(facts) == Mode::Editing {
            self.insert(text);
            return Outcome::Redraw;
        }
        self.note_raw(text.as_bytes());
        Outcome::Write(text.as_bytes().to_vec())
    }

    /// A paste. While editing it lands in the box for review and nothing runs;
    /// otherwise it is not the box's.
    pub fn paste(&mut self, facts: Facts, text: &str) -> Outcome {
        if self.mode(facts) != Mode::Editing {
            return Outcome::NotMine;
        }
        match judge(text, true) {
            Decision::Send(p) => {
                let clean = p.text.replace("\r\n", "\n").replace('\r', "\n");
                self.insert(&clean);
                Outcome::Redraw
            }
            Decision::Reject(_) => Outcome::Refused,
            Decision::Hold(_) => Outcome::None,
        }
    }

    /// Send the held lines as separate commands.
    pub fn send_held_as_commands(&mut self) -> Outcome {
        let Some(held) = self.held.take() else {
            return Outcome::None;
        };
        let mut out = Vec::new();
        for line in held.lines() {
            out.extend_from_slice(line.as_bytes());
            out.push(b'\r');
        }
        self.remember(&held);
        self.clear();
        Outcome::Write(out)
    }

    /// Send the held lines as one line.
    pub fn send_held_joined(&mut self) -> Outcome {
        let Some(held) = self.held.take() else {
            return Outcome::None;
        };
        let joined = held.lines().collect::<Vec<_>>().join(" ");
        self.remember(&held);
        self.clear();
        let mut out = joined.into_bytes();
        out.push(b'\r');
        Outcome::Write(out)
    }

    /// Cancel a held choice; the text stays in the box.
    pub fn cancel_held(&mut self) {
        self.held = None;
    }

    fn raw_key(&mut self, facts: Facts, key: Key, mods: Mods) -> Outcome {
        let bytes = bytes_in(key, mods, facts.keys);
        if bytes.is_empty() {
            return Outcome::None;
        }
        if ends_line(&bytes) {
            self.handed_off = false;
            self.typeahead = false;
        } else {
            self.note_raw(&bytes);
        }
        Outcome::Write(bytes)
    }

    /// Bytes that went to the program while the box was not editing. At a
    /// prompt they are typeahead the shell's next line already holds.
    fn note_raw(&mut self, bytes: &[u8]) {
        if !self.handed_off && !bytes.is_empty() && !ends_line(bytes) {
            self.typeahead = true;
        }
    }

    fn edit_key(&mut self, facts: Facts, key: Key, mods: Mods) -> Outcome {
        if self.held.is_some() {
            if key == Key::Escape {
                self.held = None;
                return Outcome::Redraw;
            }
            return Outcome::None;
        }
        let plain = !mods.ctrl && !mods.alt;
        match key {
            Key::Char(c) | Key::Keypad(c) if plain => {
                self.insert(&c.to_string());
                Outcome::Redraw
            }
            Key::Backspace if plain => {
                if self.caret > 0 {
                    self.caret -= 1;
                    self.text.remove(self.caret);
                }
                Outcome::Redraw
            }
            Key::Delete if plain => {
                if self.caret < self.text.len() {
                    self.text.remove(self.caret);
                }
                Outcome::Redraw
            }
            Key::Left if plain => {
                self.caret = self.caret.saturating_sub(1);
                Outcome::Redraw
            }
            Key::Right if plain => {
                self.caret = (self.caret + 1).min(self.text.len());
                Outcome::Redraw
            }
            Key::Home if plain => {
                self.caret = self.line_start();
                Outcome::Redraw
            }
            Key::End if plain => {
                self.caret = self.line_end();
                Outcome::Redraw
            }
            Key::Up if plain => self.up(),
            Key::Down if plain => self.down(),
            Key::Enter | Key::KeypadEnter if mods.shift || mods.alt => {
                self.insert("\n");
                Outcome::Redraw
            }
            Key::Enter | Key::KeypadEnter if plain => self.enter(facts),
            _ => self.hand_off(facts, key, mods),
        }
    }

    fn enter(&mut self, facts: Facts) -> Outcome {
        let text = self.text();
        if text.is_empty() {
            return Outcome::Write(vec![b'\r']);
        }
        if text.contains('\n') && !facts.bracketed_paste {
            self.held = Some(text);
            return Outcome::Hold;
        }
        let mut out = wrap(&text, facts.bracketed_paste);
        out.push(b'\r');
        self.remember(&text);
        self.clear();
        Outcome::Write(out)
    }

    /// A key the box does not edit with: the box's text first, then the key.
    fn hand_off(&mut self, facts: Facts, key: Key, mods: Mods) -> Outcome {
        let key_bytes = bytes_in(key, mods, facts.keys);
        if key_bytes.is_empty() {
            return Outcome::None;
        }
        let had_text = !self.text.is_empty();
        let mut out = if had_text {
            wrap(&self.text(), facts.bracketed_paste)
        } else {
            Vec::new()
        };
        out.extend_from_slice(&key_bytes);
        if had_text {
            let text = self.text();
            self.remember(&text);
        }
        self.clear();
        if ends_line(&key_bytes) {
            self.handed_off = false;
        } else if had_text || touches_line(&key_bytes) {
            self.handed_off = true;
        }
        Outcome::Write(out)
    }

    fn insert(&mut self, s: &str) {
        for c in s.chars() {
            self.text.insert(self.caret, c);
            self.caret += 1;
        }
        self.history_at = None;
    }

    fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.history_at = None;
    }

    fn remember(&mut self, line: &str) {
        if self.history.last().map(String::as_str) != Some(line) {
            self.history.push(line.to_owned());
            if self.history.len() > HISTORY_LIMIT {
                self.history.remove(0);
            }
        }
    }

    fn line_start(&self) -> usize {
        self.text[..self.caret]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        self.text[self.caret..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.text.len(), |i| self.caret + i)
    }

    fn up(&mut self) -> Outcome {
        if self.text.contains(&'\n') {
            let start = self.line_start();
            if start == 0 {
                return Outcome::None;
            }
            let col = self.caret - start;
            let prev_end = start - 1;
            let prev_start = self.text[..prev_end]
                .iter()
                .rposition(|&c| c == '\n')
                .map_or(0, |i| i + 1);
            self.caret = (prev_start + col).min(prev_end);
            return Outcome::Redraw;
        }
        let at = match self.history_at {
            Some(0) => return Outcome::None,
            Some(i) => i - 1,
            None if self.history.is_empty() => return Outcome::None,
            None => self.history.len() - 1,
        };
        self.load(at);
        Outcome::Redraw
    }

    fn down(&mut self) -> Outcome {
        if self.text.contains(&'\n') {
            let end = self.line_end();
            if end >= self.text.len() {
                return Outcome::None;
            }
            let col = self.caret - self.line_start();
            let next_start = end + 1;
            let next_end = self.text[next_start..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(self.text.len(), |i| next_start + i);
            self.caret = (next_start + col).min(next_end);
            return Outcome::Redraw;
        }
        match self.history_at {
            None => Outcome::None,
            Some(i) if i + 1 < self.history.len() => {
                self.load(i + 1);
                Outcome::Redraw
            }
            Some(_) => {
                self.clear();
                Outcome::Redraw
            }
        }
    }

    fn load(&mut self, at: usize) {
        self.text = self.history[at].chars().collect();
        self.caret = self.text.len();
        self.history_at = Some(at);
    }
}

/// `text` as the shell should read it: bracketed when the shell asked for that.
fn wrap(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return text.as_bytes().to_vec();
    }
    let mut out = PASTE_START.to_vec();
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(PASTE_END);
    out
}

/// A write that ends the shell's line: Enter, Ctrl+C or Ctrl+D.
fn ends_line(bytes: &[u8]) -> bool {
    matches!(bytes, [b'\r' | 0x03 | 0x04])
}

/// A key that can change the line the shell holds, so the shell has it now.
fn touches_line(bytes: &[u8]) -> bool {
    match bytes {
        // Tab, Esc, Ctrl+R and the other Ctrl+letter line editing keys; but
        // not Ctrl+L or Ctrl+Z, which leave the line alone.
        [b'\t' | 0x1b] => true,
        [b] if *b < 0x20 => !matches!(*b, 0x0c | 0x1a),
        // Alt+key sends ESC and the key.
        [0x1b, b] => *b != b'[' && *b != b'O',
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
    };
    const CTRL: Mods = Mods {
        ctrl: true,
        alt: false,
        shift: false,
    };

    fn typed(line: &mut InputLine, facts: Facts, s: &str) {
        for c in s.chars() {
            assert_eq!(line.key(facts, Key::Char(c), NONE), Outcome::Redraw);
        }
    }

    /// Every state the facts can put the box in, other than editing, with the
    /// mode it must give.
    fn raw_states() -> Vec<(&'static str, Facts, Mode)> {
        let p = Facts::at_prompt();
        vec![
            (
                "alternate screen",
                Facts {
                    alt_screen: true,
                    ..p
                },
                Mode::Raw,
            ),
            (
                "DECCKM",
                Facts {
                    keys: KeyModes {
                        cursor_app: true,
                        keypad_app: false,
                    },
                    ..p
                },
                Mode::Raw,
            ),
            (
                "application keypad",
                Facts {
                    keys: KeyModes {
                        cursor_app: false,
                        keypad_app: true,
                    },
                    ..p
                },
                Mode::Raw,
            ),
            (
                "another program in front",
                Facts {
                    shell_in_front: Some(false),
                    ..p
                },
                Mode::Raw,
            ),
            (
                "foreground unknown",
                Facts {
                    shell_in_front: None,
                    ..p
                },
                Mode::Raw,
            ),
            (
                "termios unknown",
                Facts {
                    icanon: None,
                    echo: None,
                    ..p
                },
                Mode::Raw,
            ),
            (
                "canonical with echo (a plain read)",
                Facts {
                    icanon: Some(true),
                    echo: Some(true),
                    ..p
                },
                Mode::Raw,
            ),
            (
                "hidden read in the shell",
                Facts {
                    icanon: Some(true),
                    echo: Some(false),
                    ..p
                },
                Mode::Hidden,
            ),
            (
                "hidden read in sudo",
                Facts {
                    shell_in_front: Some(false),
                    icanon: Some(true),
                    echo: Some(false),
                    ..p
                },
                Mode::Hidden,
            ),
            ("ended", Facts { ended: true, ..p }, Mode::Ended),
        ]
    }

    #[test]
    fn the_mode_follows_the_facts() {
        let line = InputLine::new();
        assert_eq!(line.mode(Facts::at_prompt()), Mode::Editing);
        for (name, facts, want) in raw_states() {
            assert_eq!(line.mode(facts), want, "{name}");
        }
    }

    #[test]
    fn a_raw_state_writes_exactly_the_keys_bytes_and_leaves_the_box_empty() {
        for (name, facts, _) in raw_states() {
            let mut line = InputLine::new();
            for (key, mods, want) in [
                (Key::Char('a'), NONE, &b"a"[..]),
                (Key::Enter, NONE, b"\r"),
                (Key::Backspace, NONE, b"\x7f"),
                (Key::Tab, NONE, b"\t"),
                (Key::Escape, NONE, b"\x1b"),
                (Key::Char('c'), CTRL, b"\x03"),
            ] {
                let want = if facts.keys.cursor_app && key == Key::Up {
                    b"\x1bOA".to_vec()
                } else {
                    want.to_vec()
                };
                assert_eq!(
                    line.key(facts, key, mods),
                    Outcome::Write(want),
                    "{name}: {key:?}"
                );
            }
            assert_eq!(line.text(), "", "{name}: the box stays empty");
        }
    }

    #[test]
    fn application_cursor_keys_reach_the_program_in_the_application_form() {
        let facts = Facts {
            keys: KeyModes {
                cursor_app: true,
                keypad_app: false,
            },
            ..Facts::at_prompt()
        };
        let mut line = InputLine::new();
        assert_eq!(
            line.key(facts, Key::Up, NONE),
            Outcome::Write(b"\x1bOA".to_vec())
        );
    }

    #[test]
    fn typing_at_a_prompt_edits_the_box_and_writes_nothing() {
        let facts = Facts::at_prompt();
        let mut line = InputLine::new();
        typed(&mut line, facts, "echo hi");
        assert_eq!(line.text(), "echo hi");
        assert_eq!(line.caret(), 7);
    }

    #[test]
    fn enter_sends_the_line_and_a_carriage_return_and_clears_the_box() {
        let facts = Facts::at_prompt();
        let mut line = InputLine::new();
        typed(&mut line, facts, "echo hi");
        assert_eq!(
            line.key(facts, Key::Enter, NONE),
            Outcome::Write(b"echo hi\r".to_vec())
        );
        assert_eq!(line.text(), "");
        assert_eq!(line.mode(facts), Mode::Editing);
    }

    #[test]
    fn enter_wraps_the_line_in_bracketed_paste_when_the_shell_asked_for_it() {
        let facts = Facts {
            bracketed_paste: true,
            ..Facts::at_prompt()
        };
        let mut line = InputLine::new();
        typed(&mut line, facts, "ls");
        assert_eq!(
            line.key(facts, Key::Enter, NONE),
            Outcome::Write(b"\x1b[200~ls\x1b[201~\r".to_vec())
        );
    }

    #[test]
    fn enter_on_an_empty_box_is_a_carriage_return() {
        let mut line = InputLine::new();
        assert_eq!(
            line.key(Facts::at_prompt(), Key::Enter, NONE),
            Outcome::Write(b"\r".to_vec())
        );
    }

    #[test]
    fn editing_keys_move_the_caret_and_delete() {
        let facts = Facts::at_prompt();
        let mut line = InputLine::new();
        typed(&mut line, facts, "abcd");
        line.key(facts, Key::Left, NONE);
        line.key(facts, Key::Left, NONE);
        line.key(facts, Key::Backspace, NONE);
        assert_eq!((line.text().as_str(), line.caret()), ("acd", 1));
        line.key(facts, Key::Delete, NONE);
        assert_eq!(line.text(), "ad");
        line.key(facts, Key::End, NONE);
        assert_eq!(line.caret(), 2);
        line.key(facts, Key::Home, NONE);
        assert_eq!(line.caret(), 0);
        line.key(facts, Key::Backspace, NONE);
        assert_eq!(line.text(), "ad", "Backspace at the start deletes nothing");
    }

    /// Every key that is not the box's to edit with, as it is written.
    fn hand_off_keys() -> Vec<(String, Key, Mods, Vec<u8>)> {
        let mut keys = vec![
            ("Tab".to_owned(), Key::Tab, NONE, b"\t".to_vec()),
            ("Esc".to_owned(), Key::Escape, NONE, b"\x1b".to_vec()),
            ("PageUp".to_owned(), Key::PageUp, NONE, b"\x1b[5~".to_vec()),
            (
                "PageDown".to_owned(),
                Key::PageDown,
                NONE,
                b"\x1b[6~".to_vec(),
            ),
            ("F1".to_owned(), Key::F(1), NONE, b"\x1bOP".to_vec()),
            ("F5".to_owned(), Key::F(5), NONE, b"\x1b[15~".to_vec()),
            ("F12".to_owned(), Key::F(12), NONE, b"\x1b[24~".to_vec()),
            (
                "Alt+b".to_owned(),
                Key::Char('b'),
                Mods { alt: true, ..NONE },
                b"\x1bb".to_vec(),
            ),
        ];
        for (i, c) in ('a'..='z').enumerate() {
            keys.push((
                format!("Ctrl+{c}"),
                Key::Char(c),
                CTRL,
                vec![u8::try_from(i).unwrap_or(0) + 1],
            ));
        }
        keys
    }

    /// Control keys are never swallowed: in every state, with and without text
    /// in the box, every one of them reaches the program, after the box's text
    /// when there is some.
    #[test]
    fn no_control_key_is_swallowed_in_any_state() {
        let mut states = raw_states();
        states.push(("editing", Facts::at_prompt(), Mode::Editing));
        states.push((
            "editing, bracketed paste on",
            Facts {
                bracketed_paste: true,
                ..Facts::at_prompt()
            },
            Mode::Editing,
        ));
        for (name, facts, mode) in states {
            for (key_name, key, mods, bytes) in hand_off_keys() {
                for text in ["", "ls"] {
                    let mut line = InputLine::new();
                    if mode == Mode::Editing {
                        typed(&mut line, facts, text);
                    } else if !text.is_empty() {
                        continue;
                    }
                    let mut want = Vec::new();
                    if !text.is_empty() {
                        want.extend(wrap(text, facts.bracketed_paste));
                    }
                    want.extend(&bytes);
                    assert_eq!(
                        line.key(facts, key, mods),
                        Outcome::Write(want),
                        "{name}, {key_name}, box text {text:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_handed_off_line_stays_with_the_shell_until_it_ends() {
        let facts = Facts::at_prompt();
        let mut line = InputLine::new();
        typed(&mut line, facts, "git ch");
        assert_eq!(
            line.key(facts, Key::Tab, NONE),
            Outcome::Write(b"git ch\t".to_vec())
        );
        assert_eq!(line.text(), "", "the box clears");
        assert_eq!(line.mode(facts), Mode::HandedOff);
        assert_eq!(
            line.key(facts, Key::Char('e'), NONE),
            Outcome::Write(b"e".to_vec()),
            "typing goes to the shell, which holds the line"
        );
        for (end, mods, bytes) in [
            (Key::Enter, NONE, &b"\r"[..]),
            (Key::Char('c'), CTRL, b"\x03"),
            (Key::Char('d'), CTRL, b"\x04"),
        ] {
            let mut l = line.clone();
            assert_eq!(l.key(facts, end, mods), Outcome::Write(bytes.to_vec()));
            assert_eq!(l.mode(facts), Mode::Editing, "{end:?} ends the line");
        }
    }

    #[test]
    fn keys_that_leave_the_line_alone_do_not_hand_the_line_off() {
        let facts = Facts::at_prompt();
        for key in [Key::PageUp, Key::F(5)] {
            let mut line = InputLine::new();
            line.key(facts, key, NONE);
            assert_eq!(line.mode(facts), Mode::Editing, "{key:?} on an empty box");
        }
        let mut line = InputLine::new();
        line.key(facts, Key::Char('l'), CTRL);
        assert_eq!(line.mode(facts), Mode::Editing, "Ctrl+L");
    }

    #[test]
    fn ctrl_c_with_text_sends_the_text_then_the_interrupt_and_ends_the_line() {
        let facts = Facts::at_prompt();
        let mut line = InputLine::new();
        typed(&mut line, facts, "sleep 9");
        assert_eq!(
            line.key(facts, Key::Char('c'), CTRL),
            Outcome::Write(b"sleep 9\x03".to_vec())
        );
        assert_eq!(line.mode(facts), Mode::Editing);
    }

    #[test]
    fn bytes_typed_while_a_command_runs_are_typeahead_the_box_leaves_to_the_shell() {
        let running = Facts {
            shell_in_front: Some(false),
            ..Facts::at_prompt()
        };
        let mut line = InputLine::new();
        assert_eq!(
            line.key(running, Key::Char('x'), NONE),
            Outcome::Write(b"x".to_vec())
        );
        // The command ends and the prompt returns: the shell's line has `x`.
        assert_eq!(line.mode(Facts::at_prompt()), Mode::Raw);
        assert_eq!(
            line.key(Facts::at_prompt(), Key::Char('y'), NONE),
            Outcome::Write(b"y".to_vec())
        );
        // Its end takes the box back.
        line.key(Facts::at_prompt(), Key::Enter, NONE);
        assert_eq!(line.mode(Facts::at_prompt()), Mode::Editing);
    }

    #[test]
    fn up_and_down_step_through_the_lines_this_window_sent() {
        let facts = Facts::at_prompt();
        let mut line = InputLine::new();
        for cmd in ["one", "two"] {
            typed(&mut line, facts, cmd);
            line.key(facts, Key::Enter, NONE);
        }
        line.key(facts, Key::Up, NONE);
        assert_eq!(line.text(), "two");
        line.key(facts, Key::Up, NONE);
        assert_eq!(line.text(), "one");
        assert_eq!(line.key(facts, Key::Up, NONE), Outcome::None);
        line.key(facts, Key::Down, NONE);
        assert_eq!(line.text(), "two");
        line.key(facts, Key::Down, NONE);
        assert_eq!(line.text(), "", "past the newest, the box is empty again");
    }

    #[test]
    fn shift_enter_adds_a_line_and_up_down_move_between_lines() {
        let facts = Facts::at_prompt();
        let mut line = InputLine::new();
        typed(&mut line, facts, "ab");
        line.key(
            facts,
            Key::Enter,
            Mods {
                shift: true,
                ..NONE
            },
        );
        typed(&mut line, facts, "cd");
        assert_eq!(line.text(), "ab\ncd");
        line.key(facts, Key::Up, NONE);
        assert_eq!(line.caret(), 2, "same column on the line above");
        line.key(facts, Key::Down, NONE);
        assert_eq!(line.caret(), 5);
    }

    #[test]
    fn a_multi_line_line_with_bracketed_paste_is_sent_as_one_paste() {
        let facts = Facts {
            bracketed_paste: true,
            ..Facts::at_prompt()
        };
        let mut line = InputLine::new();
        assert_eq!(line.paste(facts, "a\nb"), Outcome::Redraw);
        assert_eq!(line.text(), "a\nb", "nothing runs until Enter");
        assert_eq!(
            line.key(facts, Key::Enter, NONE),
            Outcome::Write(b"\x1b[200~a\nb\x1b[201~\r".to_vec())
        );
    }

    #[test]
    fn a_multi_line_line_without_bracketed_paste_is_held_for_a_choice() {
        let facts = Facts::at_prompt();
        let mut line = InputLine::new();
        line.paste(facts, "a\nb\nc");
        assert_eq!(line.key(facts, Key::Enter, NONE), Outcome::Hold);
        assert_eq!(line.held(), Some("a\nb\nc"));
        assert_eq!(
            line.clone().send_held_as_commands(),
            Outcome::Write(b"a\rb\rc\r".to_vec())
        );
        assert_eq!(
            line.clone().send_held_joined(),
            Outcome::Write(b"a b c\r".to_vec())
        );
        assert_eq!(
            line.key(facts, Key::Escape, NONE),
            Outcome::Redraw,
            "Esc cancels and writes nothing"
        );
        assert_eq!(line.held(), None);
        assert_eq!(line.text(), "a\nb\nc");
    }

    #[test]
    fn a_paste_is_sanitised_and_a_frame_introducer_is_refused() {
        let facts = Facts::at_prompt();
        let mut line = InputLine::new();
        assert_eq!(line.paste(facts, "ls\x1b[31m\x07 -l"), Outcome::Redraw);
        assert_eq!(line.text(), "ls[31m -l", "ESC and BEL are removed");
        let mut line = InputLine::new();
        assert_eq!(line.paste(facts, "x\x1b_Gframe"), Outcome::Refused);
        assert_eq!(line.text(), "");
    }

    #[test]
    fn a_paste_while_raw_is_not_the_boxs() {
        for (name, facts, _) in raw_states() {
            assert_eq!(
                InputLine::new().paste(facts, "x"),
                Outcome::NotMine,
                "{name}"
            );
        }
    }

    #[test]
    fn committed_text_goes_to_the_box_when_editing_and_to_the_program_when_raw() {
        let mut line = InputLine::new();
        assert_eq!(line.text_input(Facts::at_prompt(), "é"), Outcome::Redraw);
        assert_eq!(line.text(), "é");
        let raw = Facts {
            alt_screen: true,
            ..Facts::at_prompt()
        };
        assert_eq!(
            InputLine::new().text_input(raw, "é"),
            Outcome::Write("é".as_bytes().to_vec())
        );
    }

    #[test]
    fn typed_text_is_never_kept_in_the_box_while_a_password_is_read() {
        let facts = Facts {
            shell_in_front: Some(false),
            icanon: Some(true),
            echo: Some(false),
            ..Facts::at_prompt()
        };
        let mut line = InputLine::new();
        for c in "hunter2".chars() {
            assert_eq!(
                line.key(facts, Key::Char(c), NONE),
                Outcome::Write(c.to_string().into_bytes())
            );
        }
        assert_eq!(line.text(), "");
        assert_eq!(line.mode(facts), Mode::Hidden);
    }
}
