//! A block of terminal cells: an emulator and the text it holds.

use prismattyc_emulator::Emulator;

/// Scrollback lines kept per block.
pub const SCROLLBACK: usize = 1000;

/// A block of `columns` by `rows` cells.
///
/// A block holds what an emulator has been fed. It owns no PTY: the bytes of a
/// program reach it only through a [`Session`](crate::Session), which passes
/// them through the arm layer first, so nothing can read a program's output
/// around the layer. The boundary is checked at compile time:
///
/// ```compile_fail
/// let mut block = lumen_term::GridBlock::new(10, 2);
/// let _ = block.poll(); // a block has no way to read a program's output
/// ```
///
/// ```compile_fail
/// use lumen_term::Pty; // the PTY is not part of the public API
/// ```
pub struct GridBlock {
    emulator: Emulator,
}

impl GridBlock {
    /// An empty block, fed by [`feed`](Self::feed).
    #[must_use]
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            emulator: Emulator::new(columns, rows, SCROLLBACK),
        }
    }

    /// The size in columns and rows.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        let screen = self.emulator.screen();
        (screen.columns(), screen.rows())
    }

    /// The screen, for a renderer.
    #[must_use]
    pub fn screen(&self) -> &prismattyc_core::Screen {
        self.emulator.screen()
    }

    /// Give the emulator bytes.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.emulator.feed(bytes);
    }

    /// Resize the block.
    pub fn resize(&mut self, columns: usize, rows: usize) {
        self.emulator.resize(columns, rows);
    }

    /// The text of each row, with trailing blanks removed.
    #[must_use]
    pub fn text_rows(&self) -> Vec<String> {
        let screen = self.emulator.screen();
        (0..screen.rows())
            .map(|r| {
                let line: String = screen
                    .row(r)
                    .unwrap_or_default()
                    .iter()
                    .filter(|c| !c.wide_cont)
                    .map(|c| c.character)
                    .collect();
                line.trim_end().to_owned()
            })
            .collect()
    }

    /// Whether the alternate screen is active.
    #[must_use]
    pub fn alt_active(&self) -> bool {
        self.emulator.screen().alt_active()
    }
}
