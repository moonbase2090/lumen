//! Mouse reports for a grid block: the SGR encoding (DECSET 1006) of a press,
//! a release, a drag and a wheel step, in the block's own cells.
//!
//! The window turns what the user did into a call here; this decides whether
//! the program asked for it and what bytes it gets. It has no dependency on a
//! window. A program that has not switched a mouse mode on gets nothing, and a
//! program that switched tracking on without the SGR encoding gets nothing
//! either: the older encodings cannot name a cell past column 223, so they are
//! not sent.

pub use prismattyc_emulator::MouseTracking;

/// A button of the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The primary button.
    Left,
    /// The middle button.
    Middle,
    /// The secondary button.
    Right,
}

/// A step of the wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    /// Away from the user.
    Up,
    /// Towards the user.
    Down,
}

/// Modifier keys held while the pointer acted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Held {
    /// Shift.
    pub shift: bool,
    /// Alt or Option.
    pub alt: bool,
    /// Control.
    pub ctrl: bool,
}

/// The mouse modes a program has switched on in a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes {
    /// The highest tracking level (1000, 1002 or 1003).
    pub tracking: MouseTracking,
    /// Whether the program asked for the SGR encoding (1006).
    pub sgr: bool,
    /// Whether the program takes wheel reports (tracking on, or wheel only).
    pub wheel: bool,
}

/// A cell of the block, from 0 (the top left).
pub type Cell = (u32, u32);

/// The bytes of one SGR report: `ESC [ < code ; column ; row M` for a press or
/// motion and `m` for a release. Column and row count from 1.
#[must_use]
pub fn sgr(code: u32, cell: Cell, release: bool) -> Vec<u8> {
    let end = if release { 'm' } else { 'M' };
    format!(
        "\x1b[<{code};{};{}{end}",
        cell.0.saturating_add(1),
        cell.1.saturating_add(1)
    )
    .into_bytes()
}

fn button_code(b: Button) -> u32 {
    match b {
        Button::Left => 0,
        Button::Middle => 1,
        Button::Right => 2,
    }
}

fn held_code(h: Held) -> u32 {
    4 * u32::from(h.shift) + 8 * u32::from(h.alt) + 16 * u32::from(h.ctrl)
}

/// What the pointer has done in one block since a button went down.
#[derive(Debug, Default)]
pub struct Reporter {
    /// The button held, and whether its press was sent.
    held: Option<(Button, bool)>,
    /// The last cell reported, so a move inside one cell sends nothing.
    last: Option<Cell>,
}

impl Reporter {
    /// A button went down on `cell`. Returns the report to send, if any.
    pub fn press(&mut self, modes: Modes, b: Button, held: Held, cell: Cell) -> Option<Vec<u8>> {
        let sent = modes.tracking.is_on() && modes.sgr;
        self.held = Some((b, sent));
        self.last = Some(cell);
        sent.then(|| sgr(button_code(b) + held_code(held), cell, false))
    }

    /// A button went up. `cell` is where the pointer is now, or `None` when it
    /// left the block, in which case the last cell is used. The release is sent
    /// only if its press was sent and the modes are still on.
    pub fn release(
        &mut self,
        modes: Modes,
        b: Button,
        held: Held,
        cell: Option<Cell>,
    ) -> Option<Vec<u8>> {
        let (down, press_sent) = self.held?;
        if down != b {
            return None;
        }
        self.held = None;
        let at = cell.or(self.last)?;
        self.last = None;
        (press_sent && modes.tracking.is_on() && modes.sgr)
            .then(|| sgr(button_code(b) + held_code(held), at, true))
    }

    /// The pointer moved to `cell`. A drag is reported at level 1002 and above,
    /// bare motion at 1003, and only when the cell changed.
    pub fn motion(&mut self, modes: Modes, held: Held, cell: Cell) -> Option<Vec<u8>> {
        if self.last == Some(cell) {
            return None;
        }
        let code = match self.held {
            Some((b, true)) if modes.tracking.reports_drag() => button_code(b),
            None if modes.tracking.reports_motion() => 3,
            _ => return None,
        };
        self.last = Some(cell);
        modes
            .sgr
            .then(|| sgr(32 + code + held_code(held), cell, false))
    }

    /// The wheel turned one step over `cell`.
    #[must_use]
    pub fn wheel(modes: Modes, step: Wheel, held: Held, cell: Cell) -> Option<Vec<u8>> {
        if !(modes.wheel && modes.sgr) {
            return None;
        }
        let code = match step {
            Wheel::Up => 64,
            Wheel::Down => 65,
        };
        Some(sgr(code + held_code(held), cell, false))
    }
}
