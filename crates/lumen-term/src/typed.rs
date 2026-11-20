//! What the input box typed for each sealed command: the terminal's own record.
//!
//! On a sealed layer a command is known only from the shell's sealed frames. When
//! the box sent a line whole and the shell's next frame is a command start, the
//! terminal keeps that line as what was typed for the command. It is a record of
//! what was typed, not of what ran: aliases, history expansion and edits made in
//! the shell are not in it. It carries no authority: it adds no boundary and no
//! status; those come only from the frames.
//!
//! "Whole" means the box's line was the first thing written to the shell since
//! its last sealed prompt: no key handed to the shell and no raw byte came first.
//! Keys typed after the line's Enter do not change it, because the shell has
//! already taken the line by then. In every other case nothing is recorded.
//! Nothing here ever comes from the program's output.
//!
//! Lines typed ahead. A line typed while a command runs waits in the kernel
//! for the shell, and if the box sends a line before the shell has read it,
//! the next command to start is the queued line, not the box's. So when a
//! command starts, the terminal looks at the row the box's line was sent on:
//! the shell draws the line it runs there. If that row does not show the box's
//! line ([`row_shows`]), the record is left out. The look can only leave a
//! record out; a record is still only ever made from the box's own text and a
//! sealed command start.

use lumen_sealed::types::Frame;
use std::collections::VecDeque;

/// The most records kept: the newest are, and the oldest go first.
pub const RECORDS_KEPT: usize = 256;

/// What the box typed for one sealed command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The command id from the sealed command start.
    pub id: u32,
    /// The text the box sent, without its Enter.
    pub typed: String,
    /// The exit status from the sealed command end, once it has come.
    pub status: Option<i32>,
    /// The grid line the typed text starts on: where the shell's cursor was
    /// when the box sent the line. `None` when there was no such line (the
    /// alternate screen).
    pub line: Option<crate::grid::Line>,
}

/// Follows the box's writes and the sealed boundary frames that applied.
#[derive(Debug, Clone)]
pub struct Typed {
    /// Something other than a whole line from the box went to the shell since
    /// its last sealed prompt, or no prompt has been seen yet.
    dirty: bool,
    /// The whole line the box sent since the last prompt, waiting for the
    /// command start.
    pending: Option<(String, Option<crate::grid::Line>)>,
    records: VecDeque<Record>,
    /// Records made so far, so a reader knows when the list has changed.
    made: u64,
    /// The layer lost its keys or a frame was refused: nothing is recorded
    /// from then on, whatever comes later.
    stopped: bool,
}

impl Default for Typed {
    fn default() -> Self {
        Self::new()
    }
}

impl Typed {
    /// Nothing seen yet: nothing is recorded before the first sealed prompt.
    #[must_use]
    pub fn new() -> Self {
        Self {
            dirty: true,
            pending: None,
            records: VecDeque::new(),
            made: 0,
            stopped: false,
        }
    }

    /// The user wrote to the shell: `Some(line)` when it is a line the box sent
    /// whole, `None` for anything else (a raw key, a handed-off key, a paste
    /// that did not come through the box).
    pub fn wrote(&mut self, line: Option<String>) {
        self.wrote_at(line, None);
    }

    /// [`wrote`](Self::wrote), with the grid line a whole line starts on.
    pub fn wrote_at(&mut self, line: Option<String>, at: Option<crate::grid::Line>) {
        if self.stopped || self.pending.is_some() {
            // The shell has the line already; this is for what runs next.
            return;
        }
        match line {
            Some(text) if !self.dirty => self.pending = Some((text, at)),
            _ => self.dirty = true,
        }
    }

    /// A sealed boundary frame that applied in order.
    pub fn see(&mut self, frame: &Frame) {
        if self.stopped {
            return;
        }
        match frame {
            Frame::Prompt => {
                self.dirty = false;
                self.pending = None;
            }
            Frame::CommandStart { id } => {
                if let Some((typed, line)) = self.pending.take() {
                    if self.records.len() == RECORDS_KEPT {
                        self.records.pop_front();
                    }
                    self.records.push_back(Record {
                        id: *id,
                        typed,
                        status: None,
                        line,
                    });
                    self.made += 1;
                }
                self.dirty = true;
            }
            Frame::CommandEnd { id, status } => {
                if let Some(r) = self.records.iter_mut().rev().find(|r| r.id == *id) {
                    r.status = Some(*status);
                }
            }
            _ => {}
        }
    }

    /// The layer lost its keys, or a frame was refused: the frames say nothing
    /// from now on, so nothing more is recorded, for good. A layer does not get
    /// its keys back, and a refusal is not undone. What was recorded stays.
    pub fn stop(&mut self) {
        self.stopped = true;
        self.dirty = true;
        self.pending = None;
    }

    /// How many records have been made: it changes whenever a record is added.
    #[must_use]
    pub fn made(&self) -> u64 {
        self.made
    }

    /// The lines to recall, oldest first, without a line repeated next to
    /// itself.
    #[must_use]
    pub fn recall(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::with_capacity(self.records.len());
        for r in &self.records {
            if lines.last() != Some(&r.typed) {
                lines.push(r.typed.clone());
            }
        }
        lines
    }

    /// Leave out the newest record: its command turned out not to be the line
    /// the box sent.
    pub fn drop_newest(&mut self) {
        if self.records.pop_back().is_some() {
            self.made += 1;
        }
    }

    /// The records, oldest first.
    #[must_use]
    pub fn records(&self) -> &VecDeque<Record> {
        &self.records
    }
}

/// The least of the box's line a row that wraps on must end with.
const LEAST_SHOWN: usize = 8;

/// Whether a grid `row`, as the shell drew it, shows the start of the line
/// `typed`. The row ends with the line's first line, or, when the row is
/// `full` (the line wraps on to the next row), with the part of it that fits,
/// at least [`LEAST_SHOWN`] characters of it. Either way that text is the whole
/// row, or follows a space (where a prompt ends), so a short line cannot match
/// the tail of another. A shell that drew the line some other way (a character
/// as `<200d>`, say), or a prompt that does not end in a space, fails this,
/// and the record is left out. What it cannot catch: a box line that is word
/// for word the end of the line that ran instead (`hi` after `echo hi`).
#[must_use]
pub fn row_shows(row: &str, typed: &str, full: bool) -> bool {
    let first = typed.lines().next().unwrap_or("").trim_end();
    if first.is_empty() {
        return false;
    }
    let ends_with_word = |part: &str| {
        row.strip_suffix(part)
            .is_some_and(|before| before.is_empty() || before.ends_with(' '))
    };
    if ends_with_word(first) {
        return true;
    }
    full && first
        .char_indices()
        .skip(LEAST_SHOWN - 1)
        .map(|(i, c)| i + c.len_utf8())
        .any(|end| ends_with_word(&first[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(id: u32) -> Frame {
        Frame::CommandStart { id }
    }

    fn typed_lines(t: &Typed) -> Vec<(u32, &str, Option<i32>)> {
        t.records()
            .iter()
            .map(|r| (r.id, r.typed.as_str(), r.status))
            .collect()
    }

    #[test]
    fn a_whole_line_then_a_command_start_is_recorded_with_its_status() {
        let mut t = Typed::new();
        t.see(&Frame::Prompt);
        t.wrote(Some("echo hi".to_owned()));
        t.see(&start(1));
        assert_eq!(typed_lines(&t), [(1, "echo hi", None)]);
        t.see(&Frame::CommandEnd { id: 1, status: 3 });
        assert_eq!(typed_lines(&t), [(1, "echo hi", Some(3))]);
    }

    #[test]
    fn nothing_is_recorded_before_the_first_prompt() {
        let mut t = Typed::new();
        t.wrote(Some("echo early".to_owned()));
        t.see(&start(1));
        assert!(t.records().is_empty());
    }

    #[test]
    fn a_raw_byte_before_the_line_means_no_record() {
        let mut t = Typed::new();
        t.see(&Frame::Prompt);
        t.wrote(None);
        t.wrote(Some("echo hi".to_owned()));
        t.see(&start(1));
        assert!(t.records().is_empty());
    }

    #[test]
    fn keys_typed_after_the_enter_do_not_change_the_record() {
        let mut t = Typed::new();
        t.see(&Frame::Prompt);
        t.wrote(Some("sleep 1".to_owned()));
        t.wrote(None);
        t.wrote(Some("echo next".to_owned()));
        t.see(&start(1));
        assert_eq!(typed_lines(&t), [(1, "sleep 1", None)]);
    }

    #[test]
    fn a_prompt_without_a_command_start_drops_the_line() {
        // An empty line, or one the shell would not run, gets a new prompt.
        let mut t = Typed::new();
        t.see(&Frame::Prompt);
        t.wrote(Some("echo 'unclosed".to_owned()));
        t.see(&Frame::Prompt);
        t.see(&start(1));
        assert!(t.records().is_empty());
    }

    #[test]
    fn keys_for_a_running_command_are_never_a_record() {
        let mut t = Typed::new();
        t.see(&Frame::Prompt);
        t.wrote(Some("read x".to_owned()));
        t.see(&start(1));
        // Answering the command: not a line at a prompt.
        t.wrote(Some("answer".to_owned()));
        t.see(&Frame::CommandEnd { id: 1, status: 0 });
        t.see(&start(2));
        assert_eq!(typed_lines(&t), [(1, "read x", Some(0))]);
    }

    #[test]
    fn after_stop_nothing_more_is_recorded_and_the_records_stay() {
        let mut t = Typed::new();
        t.see(&Frame::Prompt);
        t.wrote(Some("echo one".to_owned()));
        t.see(&start(1));
        t.see(&Frame::Prompt);
        t.wrote(Some("echo two".to_owned()));
        t.stop();
        t.see(&start(2));
        assert_eq!(typed_lines(&t), [(1, "echo one", None)]);
        // Later frames and lines do not start it again, and a command end does
        // not change what was recorded.
        t.see(&Frame::CommandEnd { id: 1, status: 7 });
        t.see(&Frame::Prompt);
        t.wrote(Some("echo three".to_owned()));
        t.see(&start(3));
        assert_eq!(typed_lines(&t), [(1, "echo one", None)]);
    }

    #[test]
    fn a_row_shows_the_line_the_shell_drew_there() {
        assert!(row_shows("% echo hi", "echo hi", false));
        assert!(row_shows("echo hi", "echo hi", false), "no prompt drawn");
        // A line typed ahead ran on that row, not the box's line.
        assert!(!row_shows("% ls", "rm x", false));
        assert!(!row_shows("% echo queued-x", "echo box-y", false));
        // A short line that happens to end another is not enough.
        assert!(!row_shows("% ls", "s", true));
        assert!(!row_shows("% ls", "s", false));
        // Only the start of a long line fits on a full row.
        assert!(row_shows(
            "% echo wwwwwwwwww",
            "echo wwwwwwwwwwwwwwwwww''-end",
            true
        ));
        assert!(!row_shows(
            "% echo wwwwwwwwww",
            "echo wwwwwwwwwwwwwwwwww''-end",
            false
        ));
        assert!(!row_shows(
            "% echo w",
            "echo wwwwwwwwwwwwwwwwww''-end",
            true
        ));
        // The first line of a line sent with line breaks.
        assert!(row_shows(
            "% for i in 1 2; do",
            "for i in 1 2; do\necho $i\ndone",
            false
        ));
        assert!(!row_shows("% anything", "", false));
    }

    #[test]
    fn dropping_the_newest_record_is_seen_as_a_change() {
        let mut t = Typed::new();
        t.see(&Frame::Prompt);
        t.wrote(Some("echo one".to_owned()));
        t.see(&start(1));
        let made = t.made();
        t.drop_newest();
        assert!(t.records().is_empty());
        assert_ne!(t.made(), made);
    }

    #[test]
    fn the_oldest_record_goes_first() {
        let mut t = Typed::new();
        for id in 1..=u32::try_from(RECORDS_KEPT).unwrap_or(0) + 2 {
            t.see(&Frame::Prompt);
            t.wrote(Some(format!("echo {id}")));
            t.see(&start(id));
        }
        assert_eq!(t.records().len(), RECORDS_KEPT);
        assert_eq!(t.records().front().map(|r| r.id), Some(3));
    }
}
