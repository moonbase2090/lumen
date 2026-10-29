//! What a window drives: a session, a renderer and the size in pixels.
//!
//! The window code only turns its events into calls on [`App`] and presents
//! the frame [`App::view`] returns, so everything it does is tested here
//! without a window.

use crate::keys::{self, Key, Mods};
use crate::layer::Layer;
use crate::layout::{GUTTER_COLUMNS, HEADER_ROWS, STRIP_COLLAPSED_ROWS};
use crate::pty::SpawnError;
use crate::raster::{Frame, Renderer, Window};
use crate::session::{MAX_COLUMNS, MAX_ROWS, Session};

/// A terminal in a window of `width` by `height` pixels.
pub struct App {
    session: Session,
    renderer: Renderer,
    width: u32,
    height: u32,
}

/// The columns and rows that fit `width` by `height` pixels at `cell` size, at
/// least 1 by 1 and at most the session's limits.
#[must_use]
pub fn cells_for(width: u32, height: u32, cell: (u32, u32)) -> (usize, usize) {
    let columns = (width / cell.0.max(1)).clamp(1, u32::try_from(MAX_COLUMNS).unwrap_or(1));
    let rows = (height / cell.1.max(1)).clamp(1, u32::try_from(MAX_ROWS).unwrap_or(1));
    (columns as usize, rows as usize)
}

/// Rows of the input editor.
pub const EDITOR_ROWS: u32 = 2;

/// The window, in cells, for `columns` by `rows` of the whole window: the
/// editor sits above the one row kept for the background strip.
#[must_use]
pub fn window_for(columns: usize, rows: usize) -> Window {
    let cols = u32::try_from(columns).unwrap_or(u32::MAX);
    let rows = u32::try_from(rows).unwrap_or(u32::MAX);
    Window {
        cols,
        rows,
        editor_row: rows.saturating_sub(EDITOR_ROWS + STRIP_COLLAPSED_ROWS),
        editor_rows: EDITOR_ROWS,
    }
}

/// The cells of the block area of a window of `columns` by `rows`: what is
/// left of the window after the gutter, the header, the editor and the strip
/// row. At least 1 by 1.
#[must_use]
pub fn block_cells(columns: usize, rows: usize) -> (usize, usize) {
    let win = window_for(columns, rows);
    let usable_columns = win.cols.saturating_sub(GUTTER_COLUMNS).max(1);
    let usable_rows = win.editor_row.saturating_sub(HEADER_ROWS).max(1);
    (usable_columns as usize, usable_rows as usize)
}

impl App {
    /// Run `program` behind `layer` in a window of `width` by `height` pixels,
    /// drawn at `scale`.
    ///
    /// # Errors
    ///
    /// Returns why the renderer or the program could not be started.
    pub fn start<S: AsRef<std::ffi::OsStr>>(
        layer: Box<dyn Layer + Send>,
        program: &str,
        arguments: &[S],
        scale: u32,
        width: u32,
        height: u32,
    ) -> Result<Self, SpawnError> {
        let renderer = Renderer::light(scale).map_err(SpawnError)?;
        let (columns, rows) = cells_for(width, height, renderer.cell());
        let (block_columns, block_rows) = block_cells(columns, rows);
        let session = Session::start(layer, block_columns, block_rows, program, arguments)?;
        Ok(Self {
            session,
            renderer,
            width,
            height,
        })
    }

    /// The session, for reading the document.
    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// The window is now `width` by `height` pixels.
    ///
    /// # Errors
    ///
    /// Returns why the program could not be told.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), SpawnError> {
        let (columns, rows) = cells_for(width, height, self.renderer.cell());
        let (block_columns, block_rows) = block_cells(columns, rows);
        self.session.resize(block_columns, block_rows)?;
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// A key was pressed.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn key(&mut self, key: Key, mods: Mods) -> std::io::Result<()> {
        let bytes = keys::bytes(key, mods);
        if bytes.is_empty() {
            return Ok(());
        }
        self.session.send(&bytes)
    }

    /// Text was pasted or typed as a whole (an input method's result).
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn text(&mut self, text: &str) -> std::io::Result<()> {
        self.session.send(text.as_bytes())
    }

    /// Take what the program has written. Returns the bytes handled.
    pub fn pump(&mut self) -> usize {
        self.session.pump()
    }

    /// Whether the program has exited.
    pub fn has_exited(&mut self) -> bool {
        self.session.has_exited()
    }

    /// The window's contents: exactly `width` by `height` pixels.
    #[must_use]
    pub fn view(&self) -> Frame {
        let (columns, rows) = cells_for(self.width, self.height, self.renderer.cell());
        self.renderer
            .render_window(self.session.document(), &window_for(columns, rows))
            .viewport(self.width, self.height, self.renderer.theme().background)
    }
}
