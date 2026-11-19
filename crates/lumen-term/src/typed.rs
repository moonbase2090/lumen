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
    /// The grid line the typed text ends on, worked out from where the
    /// shell's cursor was when the box sent the line. `None` when there was
    /// no such line (the alternate screen).
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

    /// [`wrote`](Self::wrote), with the grid line a whole line will end on.
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

    /// The records, oldest first.
    #[must_use]
    pub fn records(&self) -> &VecDeque<Record> {
        &self.records
    }
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
