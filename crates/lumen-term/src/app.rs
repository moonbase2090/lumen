//! What a window drives: a session, a renderer and the size in pixels.
//!
//! The window code only turns its events into calls on [`App`] and presents
//! the frame [`App::view`] returns, so everything it does is tested here
//! without a window.

use crate::document::Event;
use crate::keys::{self, Key, Mods};
use crate::layer::Layer;
use crate::layout::{GUTTER_COLUMNS, HEADER_ROWS, STRIP_COLLAPSED_ROWS};
use crate::notify::{Desk, Notifier, OsSink, Place, Settings, Toast};
use crate::pty::SpawnError;
use crate::raster::{Frame, Renderer, Window};
use crate::session::{MAX_COLUMNS, MAX_ROWS, Session};

/// A terminal in a window of `width` by `height` pixels.
pub struct App {
    session: Session,
    renderer: Renderer,
    width: u32,
    height: u32,
    notifier: Notifier,
    sink: Box<dyn OsSink + Send>,
    desk: Desk,
    input_notice: Option<InputNotice>,
}

/// How long the notice that input was not sent stays after the last refusal.
pub const INPUT_NOTICE_MS: u64 = 4000;

/// Why input could not be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// The program has left too much of what was typed unread.
    NotReading,
    /// The program's input can no longer be written.
    Gone,
}

/// The terminal's own notice that typed input was not sent. It is drawn as a
/// toast, in the notification area and never in the trust margin, and its text
/// is fixed here: the program cannot choose a word of it.
#[derive(Debug, Clone, Copy)]
struct InputNotice {
    reason: Refusal,
    refused: u64,
    last_ms: u64,
}

impl InputNotice {
    fn text(&self) -> String {
        let why = match self.reason {
            Refusal::NotReading => "the program is not reading its input",
            Refusal::Gone => "the program is no longer reading",
        };
        format!("Input not sent: {why} ({} dropped)", self.refused)
    }
}

/// The layer id of the terminal's own toast: not the id of any layer.
const TERMINAL_LAYER: u32 = 0;

/// The label of the one layer a window has: its position in the layout, which a
/// program cannot choose.
pub const PANE_LABEL: &str = "pane 1";

/// The layer's id in the notifier.
const LAYER: u32 = 1;

/// An [`OsSink`] that drops everything: what a window has until the user turns
/// operating-system notifications on.
#[derive(Debug, Default)]
pub struct NoSink;

impl OsSink for NoSink {
    fn send(&mut self, _: &crate::notify::OsNotification) {}
}

/// Where the session runs, as far as the terminal can tell. It is never
/// [`Place::Local`]: a local layer is one whose foreground process tree is shown
/// to be local, and the terminal has no way to show that yet (the program may
/// start `ssh` or a multiplexer at any time), so nothing a user passes can make
/// it so. A terminal that is itself inside `ssh` or a multiplexer says so, and
/// everything else is unknown. Shell integration is what will supply a local
/// place, through [`App::configure_notifications`]. `set` says whether an
/// environment variable is set.
#[must_use]
pub fn place_of(set: impl Fn(&str) -> bool) -> Place {
    if set("SSH_CONNECTION") || set("SSH_TTY") || set("SSH_CLIENT") {
        Place::Ssh
    } else if set("TMUX") || set("STY") {
        Place::MultiplexerPane
    } else {
        Place::Unknown
    }
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
        let mut app = Self {
            session,
            renderer,
            width,
            height,
            notifier: Notifier::new(Settings::default()),
            sink: Box::new(NoSink),
            // Focused until the window says otherwise: a notification that
            // would be an OS one needs the window to be shown unfocused.
            desk: Desk::default(),
            input_notice: None,
        };
        app.configure_notifications(Settings::default(), Place::Unknown, Box::new(NoSink));
        Ok(app)
    }

    /// Set what the layer's notifications may do. `settings` is the user's, off
    /// by default; `place` is where the session runs; `sink` shows an
    /// operating-system notification and is called only when the settings and
    /// the notifier's conditions allow one. Replaces any earlier notifier.
    pub fn configure_notifications(
        &mut self,
        settings: Settings,
        place: Place,
        sink: Box<dyn OsSink + Send>,
    ) {
        let mut notifier = Notifier::new(settings);
        notifier.set_desk(self.desk);
        notifier.add_layer(LAYER, PANE_LABEL, place);
        self.notifier = notifier;
        self.sink = sink;
    }

    /// Apply the user's notification settings. The operating-system sink is
    /// made only if `os_notifications` is on; otherwise nothing that could show
    /// one exists. The place is [`place_of`] the environment, which is never
    /// local, so until something can show the foreground process tree is local
    /// no unsigned toast and no operating-system notification is shown.
    pub fn use_settings(&mut self, settings: Settings) {
        let place = place_of(|name| std::env::var_os(name).is_some());
        let sink: Box<dyn OsSink + Send> = if settings.os_notifications {
            Box::new(crate::os_backend::SystemSink::new())
        } else {
            Box::new(NoSink)
        };
        self.configure_notifications(settings, place, sink);
    }

    /// The window gained or lost the focus.
    pub fn set_focused(&mut self, focused: bool) {
        self.desk.focused = focused;
        self.notifier.set_desk(self.desk);
    }

    /// Route the document's events to the notifier, as [`pump`](Self::pump)
    /// does with what the program wrote.
    pub fn route(&mut self, events: &[Event]) {
        let now = self.session.now_ms();
        self.notifier.route(now, LAYER, events, self.sink.as_mut());
    }

    /// The toasts that are drawn, top down: the terminal's own notice that input
    /// was not sent, if there is one, then the notifier's. The terminal's is
    /// first so that nothing a program does can push it out of the area.
    #[must_use]
    pub fn toasts(&self) -> Vec<Toast> {
        let mut toasts = Vec::with_capacity(self.notifier.shown().len() + 1);
        if let Some(notice) = &self.input_notice {
            toasts.push(Toast {
                layer: TERMINAL_LAYER,
                text: notice.text(),
                unsigned: false,
                shown_ms: notice.last_ms,
            });
        }
        toasts.extend_from_slice(self.notifier.shown());
        toasts
    }

    /// The user pressed the primary button at pixel `(x, y)`. A press on a toast
    /// dismisses it and returns `true` (the window draws again); a press
    /// anywhere else does nothing here. Only the user's own action dismisses a
    /// toast before its time is up: no output from the program does.
    pub fn click(&mut self, x: u32, y: u32) -> bool {
        match self.click_would_hit(x, y) {
            Some(index) => {
                if self.input_notice.take().is_some() {
                    if index > 0 {
                        self.notifier.dismiss(index - 1);
                    }
                } else {
                    self.notifier.dismiss(index);
                }
                true
            }
            None => false,
        }
    }

    /// Which toast a press at pixel `(x, y)` would dismiss, without dismissing
    /// it, as its index in [`toasts`](Self::toasts). With no notice that input
    /// was not sent, that is its index in the notifier's.
    #[must_use]
    pub fn click_would_hit(&self, x: u32, y: u32) -> Option<usize> {
        let (columns, rows) = cells_for(self.width, self.height, self.renderer.cell());
        let win = window_for(columns, rows);
        self.renderer.toast_at(&win, &self.toasts(), x, y)
    }

    /// Remove the toasts whose time is up. Returns whether any was removed, so
    /// the window knows to draw again.
    pub fn tick(&mut self) -> bool {
        let before = self.notifier.shown().len();
        let now = self.session.now_ms();
        self.notifier.tick(now);
        let mut changed = self.notifier.shown().len() != before;
        if self
            .input_notice
            .is_some_and(|n| now.saturating_sub(n.last_ms) >= INPUT_NOTICE_MS)
        {
            self.input_notice = None;
            changed = true;
        }
        changed
    }

    /// The notifier, for reading what is shown and counted.
    #[must_use]
    pub fn notifier(&self) -> &Notifier {
        &self.notifier
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
        let sent = self.send(&bytes);
        self.note_input(&sent);
        sent
    }

    /// Text was pasted or typed as a whole (an input method's result).
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn text(&mut self, text: &str) -> std::io::Result<()> {
        let sent = self.send(text.as_bytes());
        self.note_input(&sent);
        sent
    }

    /// Write to the program, unless it has ended. Whether a write to a PTY whose
    /// program is gone fails depends on the operating system (it does at once on
    /// macOS and can succeed into a buffer on Linux), so the app asks the session
    /// whether the program has ended and does not leave the answer to the write.
    fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if self.session.has_exited() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the program has ended",
            ));
        }
        self.session.send(bytes)
    }

    /// Keep what the user must be told when typed input was not sent: a refusal
    /// is counted and shown as a toast, and is never silent.
    fn note_input(&mut self, sent: &std::io::Result<()>) {
        let Err(e) = sent else { return };
        let reason = if e.kind() == std::io::ErrorKind::WouldBlock {
            Refusal::NotReading
        } else {
            Refusal::Gone
        };
        let now = self.session.now_ms();
        let refused = self.input_notice.map_or(0, |n| n.refused).saturating_add(1);
        self.input_notice = Some(InputNotice {
            reason,
            refused,
            last_ms: now,
        });
    }

    /// Whether the user has been told, and not yet dismissed or let expire, that
    /// typed input was not sent. For tests and the window.
    #[must_use]
    pub fn input_refused(&self) -> bool {
        self.input_notice.is_some()
    }

    /// Take what the program has written. Returns the bytes handled.
    pub fn pump(&mut self) -> usize {
        let bytes = self.session.pump();
        let events = self.session.take_events();
        if !events.is_empty() {
            self.route(&events);
        }
        self.tick();
        bytes
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
            .render_window_with(
                self.session.document(),
                &window_for(columns, rows),
                &self.toasts(),
            )
            .viewport(self.width, self.height, self.renderer.theme().background)
    }
}
