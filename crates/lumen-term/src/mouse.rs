//! Mouse reports for a grid block: a press, a release, a drag and a wheel step,
//! in the block's own cells, in the SGR encoding (DECSET 1006) or, when the
//! program asked for tracking without it, the default encoding.
//!
//! The window turns what the user did into a call here; this decides whether
//! the program asked for it and what bytes it gets. It has no dependency on a
//! window. A program that has not switched a mouse mode on gets nothing. The
//! default encoding (`ESC [ M` and three bytes) cannot name a cell past column
//! or row 223: a press there is not sent, nor is anything of that click, and a
//! release or a move that ends there is clamped to the last cell it can name.

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

/// The most a coordinate can be, counting from 1, in the default encoding: the
/// byte is the coordinate plus 32 and must fit in 255.
const LEGACY_LIMIT: u32 = 223;

/// The bytes of one report in the default encoding: `ESC [ M` then the code,
/// the column and the row, each plus 32 as one byte, the cells counting from 1.
/// `None` when a coordinate is past what a byte can say.
#[must_use]
pub fn legacy(code: u32, cell: Cell) -> Option<Vec<u8>> {
    let (column, row) = (cell.0.checked_add(1)?, cell.1.checked_add(1)?);
    if column > LEGACY_LIMIT || row > LEGACY_LIMIT {
        return None;
    }
    let byte = |n: u32| u8::try_from(32 + n).ok();
    Some(vec![
        0x1b,
        b'[',
        b'M',
        byte(code)?,
        byte(column)?,
        byte(row)?,
    ])
}

/// The cell, kept to what the default encoding can name.
fn clamped(cell: Cell) -> Cell {
    (cell.0.min(LEGACY_LIMIT - 1), cell.1.min(LEGACY_LIMIT - 1))
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
        let code = button_code(b) + held_code(held);
        let report = if !modes.tracking.is_on() {
            None
        } else if modes.sgr {
            Some(sgr(code, cell, false))
        } else {
            legacy(code, cell)
        };
        self.held = Some((b, report.is_some()));
        self.last = Some(cell);
        report
    }

    /// A button went up. `cell` is where the pointer is now, or `None` when it
    /// left the block, in which case the last cell is used. The release is sent
    /// only if its press was sent and the modes are still on. In the default
    /// encoding it names no button, only the modifiers.
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
        if !(press_sent && modes.tracking.is_on()) {
            return None;
        }
        if modes.sgr {
            Some(sgr(button_code(b) + held_code(held), at, true))
        } else {
            legacy(3 + held_code(held), clamped(at))
        }
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
        let code = 32 + code + held_code(held);
        if modes.sgr {
            Some(sgr(code, cell, false))
        } else {
            legacy(code, cell)
        }
    }

    /// The wheel turned one step over `cell`.
    #[must_use]
    pub fn wheel(modes: Modes, step: Wheel, held: Held, cell: Cell) -> Option<Vec<u8>> {
        if !modes.wheel {
            return None;
        }
        let code = match step {
            Wheel::Up => 64,
            Wheel::Down => 65,
        } + held_code(held);
        if modes.sgr {
            Some(sgr(code, cell, false))
        } else {
            legacy(code, cell)
        }
    }
}
