//! What a window drives: a session, a renderer and the size in pixels.
//!
//! The window code only turns its events into calls on [`App`] and presents
//! the frame [`App::view`] returns, so everything it does is tested here
//! without a window.

use crate::actions::{self, Action};
use crate::boundary::{Boundaries, Seen};
#[cfg(feature = "links-clipboard")]
use crate::clipboard::Clipboard;
use crate::design::{Appearance, Fonts, ThemeChoice};
use crate::document::{Block, Event};
use crate::glide::Glide;
use crate::input_line::{Facts, InputLine, Mode, Outcome as Line};
use crate::keys::{Key, Mods};
use crate::layer::Layer;
use crate::layout::{GUTTER_COLUMNS, HEADER_ROWS, STRIP_COLLAPSED_ROWS};
use crate::mouse::{Button, Held, Reporter, Wheel};
use crate::notify::{Desk, Notifier, OsSink, Place, Settings, Toast};
#[cfg(feature = "links-clipboard")]
use crate::paste::{self, Decision, Hold, Pasted, Rejection};
use crate::pty::SpawnError;
use crate::raster::{Frame, INPUT_MAX_LINES, InputView, Renderer, Window};
use crate::scroll::Scroll;
use crate::select::{self, Pos, Selection};
use crate::session::{MAX_COLUMNS, MAX_ROWS, Session};

/// What [`App::paste`] did.
#[cfg(feature = "links-clipboard")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteOutcome {
    /// The paste was sent.
    Sent,
    /// The program has not enabled bracketed paste and the text has a line break
    /// or another control: nothing was sent until the user chooses.
    Held(Hold),
    /// The paste was refused (it carries a frame introducer).
    Rejected(Rejection),
}

/// What the window does after an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing changed.
    Nothing,
    /// The view changed: draw again.
    Redraw,
    /// The paste was refused, and nothing was sent.
    Refused,
    /// The application should end.
    Exit,
}

/// The state of link activation: the question on screen, the opener, and
/// whether the press that took the prompt is still down.
#[cfg(feature = "links-clipboard")]
struct Links {
    prompt: Option<crate::linkprompt::Prompt>,
    opener: Box<dyn crate::linkprompt::LinkOpener + Send>,
    press_down: bool,
}

#[cfg(feature = "links-clipboard")]
impl Default for Links {
    fn default() -> Self {
        Self {
            prompt: None,
            opener: Box::new(crate::linkprompt::SystemOpener),
            press_down: false,
        }
    }
}

/// A terminal in a window of `width` by `height` pixels.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent window states: selecting, redraw, exited, focus"
)]
pub struct App {
    session: Session,
    renderer: Renderer,
    width: u32,
    height: u32,
    notifier: Notifier,
    sink: Box<dyn OsSink + Send>,
    desk: Desk,
    input_notice: Option<InputNotice>,
    /// The terminal's own notice that the shell layer has no sealing keys.
    seal_notice: Option<SealNotice>,
    /// The render scale the display asked for, and the one in use now.
    base_scale: u32,
    scale: u32,
    /// A scale the user fixed with an option: the display does not change it.
    scale_pinned: bool,
    mouse: Reporter,
    theme_choice: ThemeChoice,
    appearance: Appearance,
    selection: Option<Selection>,
    selecting: bool,
    selection_changed: bool,
    copy_on_select: bool,
    copy_pending: bool,
    scroll: Scroll,
    /// What is left of a wheel's move that the view is easing through.
    glide: Glide,
    /// Whether a wheel step eases the view instead of jumping it.
    smooth_scroll: bool,
    /// How many times the view moved since the host last asked.
    view_moves: u32,
    /// The find bar and its matches, while it is open.
    find: Option<crate::search::Find>,
    /// What was typed in the find bar when it was last closed, so reopening it,
    /// or going to the next or previous match, searches for it again.
    find_kept: String,
    /// When the cursor last moved the blink back to its on half: output or a key.
    cursor_epoch_ms: u64,
    /// The blink half the last frame was drawn with, so a change asks for a new frame.
    cursor_phase: bool,
    /// The program has ended: its screen has no cursor any more.
    exited: bool,
    attention: crate::attention::Attention,
    preedit: Option<Preedit>,
    /// What the terminal is asking about a link, and how it opens one.
    #[cfg(feature = "links-clipboard")]
    links: Links,
    /// The input box: what the user is typing, and where keys go.
    input: InputLine,
    /// Rows the editor has now: its border and one row per line of its text.
    box_rows: u32,
    /// The name of the program in front, by process group, for the status row.
    front_name: Option<(i32, String)>,
    /// The sealed layer's boundary frames: whether the shell said, sealed,
    /// that it is at a prompt.
    boundaries: Boundaries,
    /// Whether the layer had sealing keys when last asked.
    sealed_keys: bool,
    /// Enters forwarded for the user while the shell was in front.
    forwarded_enters: u64,
    /// On a sealed layer, the lines of one write still to go, each ending
    /// with its Enter: each is sent only once the shell is back at a sealed
    /// prompt and has passed the checks again.
    queued: std::collections::VecDeque<Vec<u8>>,
    /// The queued lines are the box's own text, sent whole.
    queued_whole: bool,
    /// What the box typed for each sealed command.
    typed: crate::typed::Typed,
    /// The records the box recalls with Up, by [`Typed::made`]; `None` while
    /// it recalls its own history.
    ///
    /// [`Typed::made`]: crate::typed::Typed::made
    recalling: Option<u64>,
}

/// The render scale for a display's scale factor: the nearest whole number,
/// at least 1 and at most the largest zoom. A factor that is not a finite
/// positive number is 1.
#[must_use]
pub fn display_scale(factor: f64) -> u32 {
    if !factor.is_finite() || factor < 1.0 {
        return 1;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a finite factor of at least 1, rounded and capped, fits"
    )]
    let whole = factor.round().min(f64::from(actions::MAX_SCALE)) as u32;
    whole.max(1)
}

/// The longest preedit, in characters, the app keeps. An input method sends a
/// few words at most; the cap keeps a misbehaving one from growing it.
pub const PREEDIT_MAX_CHARS: usize = 256;

/// Text an input method is composing and has not committed yet: it is shown to
/// the user and sent to the program only when composition finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Preedit {
    text: String,
    /// The caret or selection inside `text`, as byte offsets.
    cursor: Option<(usize, usize)>,
}

mod find;

/// Rows of text one wheel step scrolls.
pub const WHEEL_ROWS: u32 = 3;

/// How long the notice that input was not sent stays after the last refusal.
pub const INPUT_NOTICE_MS: u64 = 4000;

/// How long a blinking cursor stays on, and then off, in milliseconds. A key or output from the
/// program starts a new cycle with the cursor on.
pub const CURSOR_BLINK_MS: u64 = 530;

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

/// The terminal's own notice that a shell layer runs without sealing keys:
/// either it never had them, or it lost them. It is drawn as a toast, first,
/// and stays until the user dismisses it or the program ends. Its text is
/// fixed here and the reason is the terminal's own: the program cannot choose
/// a word of it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SealNotice {
    text: String,
    shown_ms: u64,
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

/// Columns a character takes in the editor: 2 for the East Asian wide and
/// fullwidth blocks and emoji, else 1. It only sizes the candidate window's
/// anchor, so the blocks are the common ones, not the full width table.
fn cell_columns(c: char) -> u32 {
    let wide = matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD);
    if wide { 2 } else { 1 }
}

/// Rows of the input editor.
pub const EDITOR_ROWS: u32 = 2;

/// The window, in cells, for `columns` by `rows` of the whole window: the
/// editor sits above the one row kept for the background strip.
#[must_use]
pub fn window_for(columns: usize, rows: usize) -> Window {
    window_for_box(columns, rows, EDITOR_ROWS)
}

/// [`window_for`] with an editor of `editor_rows` rows, as the input box grows
/// with its text.
#[must_use]
pub fn window_for_box(columns: usize, rows: usize, editor_rows: u32) -> Window {
    let cols = u32::try_from(columns).unwrap_or(u32::MAX);
    let rows = u32::try_from(rows).unwrap_or(u32::MAX);
    Window {
        cols,
        rows,
        editor_row: rows.saturating_sub(editor_rows + STRIP_COLLAPSED_ROWS),
        editor_rows,
    }
}

/// The cells of the block area of a window of `columns` by `rows`: what is
/// left of the window after the gutter, the header, the editor and the strip
/// row. At least 1 by 1.
#[must_use]
pub fn block_cells(columns: usize, rows: usize) -> (usize, usize) {
    block_cells_for(columns, rows, EDITOR_ROWS)
}

/// [`block_cells`] with an editor of `editor_rows` rows.
#[must_use]
pub fn block_cells_for(columns: usize, rows: usize, editor_rows: u32) -> (usize, usize) {
    let win = window_for_box(columns, rows, editor_rows);
    let usable_columns = win.cols.saturating_sub(GUTTER_COLUMNS).max(1);
    let usable_rows = win.editor_row.saturating_sub(HEADER_ROWS).max(1);
    (usable_columns as usize, usable_rows as usize)
}

/// The columns and rows of a window of `width` by `height` pixels drawn by
/// `renderer`, whose text column starts a gutter padding right of the gutter.
fn window_cells(width: u32, height: u32, renderer: &Renderer) -> (usize, usize) {
    cells_for(
        width.saturating_sub(renderer.gutter_pad()),
        height,
        renderer.cell(),
    )
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
        Self::start_with_shell(
            layer,
            program,
            arguments,
            scale,
            width,
            height,
            crate::shellint::ShellOptions::default(),
        )
    }

    /// [`start`](Self::start) with the shell's options given: whether a shell
    /// started directly gets the prompt-mark hooks.
    ///
    /// # Errors
    ///
    /// Returns why the renderer or the program could not be started.
    pub fn start_with_shell<S: AsRef<std::ffi::OsStr>>(
        layer: Box<dyn Layer + Send>,
        program: &str,
        arguments: &[S],
        scale: u32,
        width: u32,
        height: u32,
        shell: crate::shellint::ShellOptions,
    ) -> Result<Self, SpawnError> {
        let renderer = Renderer::new(scale, crate::design::theme()).map_err(SpawnError)?;
        let (columns, rows) = window_cells(width, height, &renderer);
        let (block_columns, block_rows) = block_cells(columns, rows);
        let session =
            Session::start_with_shell(layer, block_columns, block_rows, program, arguments, shell)?;
        Ok(Self::with_session(session, renderer, scale, width, height))
    }

    /// [`start`](Self::start) with the sealed layer's settings given: a listed
    /// shell started directly gets a sealed layer if the sealing conditions
    /// hold.
    ///
    /// # Errors
    ///
    /// Returns why the renderer or the program could not be started.
    #[cfg(target_os = "linux")]
    pub fn start_with_seal<S: AsRef<std::ffi::OsStr>>(
        layer: Box<dyn Layer + Send>,
        program: &str,
        arguments: &[S],
        (scale, width, height): (u32, u32, u32),
        config: &crate::seal::Config,
    ) -> Result<Self, SpawnError> {
        let renderer = Renderer::new(scale, crate::design::theme()).map_err(SpawnError)?;
        let (columns, rows) = window_cells(width, height, &renderer);
        let (block_columns, block_rows) = block_cells(columns, rows);
        let session =
            Session::start_with_seal(layer, block_columns, block_rows, program, arguments, config)?;
        Ok(Self::with_session(session, renderer, scale, width, height))
    }

    fn with_session(
        session: Session,
        renderer: Renderer,
        scale: u32,
        width: u32,
        height: u32,
    ) -> Self {
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
            seal_notice: None,
            base_scale: scale,
            scale,
            scale_pinned: false,
            mouse: Reporter::default(),
            theme_choice: ThemeChoice::System,
            appearance: Appearance::Light,
            selection: None,
            selecting: false,
            selection_changed: false,
            copy_on_select: true,
            copy_pending: false,
            scroll: Scroll::Follow,
            glide: Glide::new(),
            smooth_scroll: true,
            view_moves: 0,
            find: None,
            find_kept: String::new(),
            cursor_epoch_ms: 0,
            cursor_phase: true,
            exited: false,
            attention: crate::attention::Attention::new(),
            preedit: None,
            #[cfg(feature = "links-clipboard")]
            links: Links::default(),
            input: InputLine::new(),
            box_rows: EDITOR_ROWS,
            front_name: None,
            boundaries: Boundaries::new(),
            sealed_keys: false,
            forwarded_enters: 0,
            queued: std::collections::VecDeque::new(),
            queued_whole: false,
            typed: crate::typed::Typed::new(),
            recalling: None,
        };
        app.sealed_keys = app.session.has_sealed_keys();
        if let Some(why) = app.session.seal_off_reason() {
            app.seal_notice = Some(SealNotice {
                text: format!("Shell not sealed: {why}"),
                shown_ms: app.session.now_ms(),
            });
        }
        app.configure_notifications(Settings::default(), Place::Unknown, Box::new(NoSink));
        app
    }

    /// The system now shows `appearance`. With [`ThemeChoice::System`] the look
    /// follows it at once, with no restart. Returns whether the look changed,
    /// so the window knows to draw again.
    pub fn set_appearance(&mut self, appearance: Appearance) -> bool {
        self.appearance = appearance;
        self.apply_theme()
    }

    /// Use `choice` from now on. Returns whether the look changed.
    pub fn set_theme_choice(&mut self, choice: ThemeChoice) -> bool {
        self.theme_choice = choice;
        self.apply_theme()
    }

    /// The user's choice of look.
    #[must_use]
    pub fn theme_choice(&self) -> ThemeChoice {
        self.theme_choice
    }

    /// Use `fonts` from the next frame on, with no restart: the cells are
    /// measured again and the program's grid is resized to fit the window.
    /// Returns whether anything changed.
    ///
    /// # Errors
    ///
    /// Returns why a face could not be read or the program could not be
    /// resized. The fonts in use are kept when a face cannot be read.
    pub fn set_fonts(&mut self, fonts: &Fonts) -> Result<bool, SpawnError> {
        if self.renderer.fonts() == fonts {
            return Ok(false);
        }
        let renderer =
            Renderer::with_fonts(self.renderer.scale(), self.renderer.theme().clone(), fonts)
                .map_err(SpawnError)?;
        self.renderer = renderer;
        self.resize(self.width, self.height)?;
        Ok(true)
    }

    /// Draw, and answer colour queries, in the palette the choice and the
    /// appearance give.
    fn apply_theme(&mut self) -> bool {
        let theme = self.theme_choice.palette(self.appearance).theme();
        if *self.renderer.theme() == theme {
            return false;
        }
        #[cfg(feature = "colour-replies")]
        self.session.set_theme(theme.clone());
        self.renderer.set_theme(theme);
        true
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

    /// Apply the user's `images` setting: `false` always shows the alt text.
    pub fn use_images(&mut self, on: bool) {
        self.session.set_images_setting(on);
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
        self.cursor_epoch_ms = self.session.now_ms();
        self.notifier.set_desk(self.desk);
    }

    /// Route the document's events to the notifier, as [`pump`](Self::pump)
    /// does with what the program wrote.
    pub fn route(&mut self, events: &[Event]) {
        for event in events {
            if let Event::Sealed { kind, body, at } = event {
                let seen = self.boundaries.see(*kind, body);
                match seen {
                    Seen::Applied => {
                        if let Ok(frame) = lumen_sealed::types::parse(*kind, body) {
                            self.typed.see_at(&frame, *at);
                        }
                    }
                    Seen::Refused => self.typed.stop(),
                    Seen::Other => {}
                }
                // The shell-replacement check runs at every authenticated
                // command start too, not only at a forwarded Enter.
                // So does a prompt while lines are queued: the next one is due.
                let queued_due =
                    *kind == lumen_sealed::types::KIND_PROMPT && !self.queued.is_empty();
                if seen == Seen::Applied
                    && (*kind == lumen_sealed::types::KIND_COMMAND_START || queued_due)
                {
                    self.session.recheck_seal();
                    self.refresh_seal();
                }
            }
        }
        let now = self.session.now_ms();
        self.notifier.route(now, LAYER, events, self.sink.as_mut());
    }

    /// The toasts that are drawn, top down: the terminal's own notices first
    /// (that the shell layer has no sealing keys, then that input was not
    /// sent), then the notifier's. The terminal's are first so that nothing a
    /// program does can push them out of the area.
    #[must_use]
    pub fn toasts(&self) -> Vec<Toast> {
        let mut toasts = Vec::with_capacity(self.notifier.shown().len() + 2);
        if let Some(notice) = &self.seal_notice {
            toasts.push(Toast {
                layer: TERMINAL_LAYER,
                text: notice.text.clone(),
                unsigned: false,
                shown_ms: notice.shown_ms,
            });
        }
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
            Some(mut index) => {
                if self.seal_notice.is_some() {
                    if index == 0 {
                        self.seal_notice = None;
                        return true;
                    }
                    index -= 1;
                }
                if self.input_notice.is_some() {
                    if index == 0 {
                        self.input_notice = None;
                        return true;
                    }
                    index -= 1;
                }
                self.notifier.dismiss(index);
                true
            }
            None => self.click_box(x, y),
        }
    }

    /// Use `opener` to open links the user confirms.
    #[cfg(feature = "links-clipboard")]
    pub fn use_link_opener(&mut self, opener: Box<dyn crate::linkprompt::LinkOpener + Send>) {
        self.links.opener = opener;
    }

    /// The question the terminal is drawing about a link, if there is one.
    #[cfg(feature = "links-clipboard")]
    #[must_use]
    pub fn link_prompt(&self) -> Option<&crate::linkprompt::Prompt> {
        self.links.prompt.as_ref()
    }

    /// Where the link prompt is drawn and its buttons, if there is one.
    #[cfg(feature = "links-clipboard")]
    #[must_use]
    pub fn link_prompt_box(&self) -> Option<crate::raster::PromptBox> {
        let prompt = self.links.prompt.as_ref()?;
        Some(
            self.renderer
                .link_prompt_box(self.width, self.height, prompt),
        )
    }

    /// The link under pixel `(x, y)` of the live grid, if there is one.
    #[cfg(feature = "links-clipboard")]
    #[must_use]
    pub fn link_at(&self, x: u32, y: u32) -> Option<crate::linkhit::Hit> {
        let (col, row) = self.live_point(x, y)?;
        let Some(Block::Grid(live)) = self.session.document().blocks().last() else {
            return None;
        };
        let col = usize::try_from(col).ok()?;
        let hits = if row < 0 {
            // A line of the scrollback, counted up from the screen's top.
            let back = live
                .history_rows()
                .checked_sub(usize::try_from(row.unsigned_abs()).ok()?)?;
            crate::linkhit::cell_links(live.history_row(back)?)
        } else {
            crate::linkhit::row_links(live.screen(), usize::try_from(row).ok()?)
        };
        hits.into_iter().find(|hit| hit.columns.contains(&col))
    }

    /// The user pressed the link modifier with the primary button at pixel
    /// `(x, y)`. Over a link, the terminal asks before anything opens, and the
    /// press is the terminal's, not the program's: returns `true`. Nothing is
    /// opened here.
    #[cfg(feature = "links-clipboard")]
    pub fn link_press(&mut self, x: u32, y: u32) -> bool {
        if self.links.prompt.is_some() {
            return self.prompt_click(x, y);
        }
        let Some(hit) = self.link_at(x, y) else {
            return false;
        };
        self.links.prompt = Some(crate::linkprompt::Prompt::for_hit(&hit));
        self.links.press_down = true;
        self.view_moves = self.view_moves.wrapping_add(1);
        true
    }

    /// A press at pixel `(x, y)` while a link prompt is shown. A press on the
    /// open button opens, on the cancel button or away from the box closes,
    /// and elsewhere on the box does nothing. Returns whether the press was
    /// the prompt's.
    #[cfg(feature = "links-clipboard")]
    pub fn prompt_click(&mut self, x: u32, y: u32) -> bool {
        use crate::raster::PromptHit;
        let Some(prompt) = &self.links.prompt else {
            return false;
        };
        self.links.press_down = true;
        match self
            .renderer
            .link_prompt_at(self.width, self.height, prompt, x, y)
        {
            PromptHit::Open => {
                let _ = self.answer_link(true);
            }
            PromptHit::Cancel | PromptHit::Outside => {
                let _ = self.answer_link(false);
            }
            PromptHit::Box => {}
        }
        true
    }

    /// The user answered the link prompt: `true` to open, `false` to close it.
    /// Returns whether a link was handed to the system. A prompt that only
    /// informs never opens.
    ///
    /// # Errors
    ///
    /// Returns why the system opener could not be started.
    #[cfg(feature = "links-clipboard")]
    pub fn answer_link(&mut self, open: bool) -> std::io::Result<bool> {
        let Some(prompt) = self.links.prompt.take() else {
            return Ok(false);
        };
        self.view_moves = self.view_moves.wrapping_add(1);
        match (open, prompt.target()) {
            (true, Some(target)) => self.links.opener.open(target).map(|()| true),
            _ => Ok(false),
        }
    }

    /// A press in the input box while it edits puts the caret at the cell
    /// pressed. Returns whether the box changed.
    fn click_box(&mut self, x: u32, y: u32) -> bool {
        if self.input_mode() != Mode::Editing {
            return false;
        }
        let (cell_w, cell_h) = self.renderer.cell();
        let (columns, rows) = window_cells(self.width, self.height, &self.renderer);
        let win = window_for_box(columns, rows, self.box_rows);
        let top = win.editor_row * cell_h;
        let left = GUTTER_COLUMNS * cell_w + self.renderer.gutter_pad() + 3 * cell_w;
        let inside = y >= top && y < (win.editor_row + win.editor_rows) * cell_h;
        if !inside || x < left || cell_h == 0 || cell_w == 0 {
            return false;
        }
        let line = ((y - top) / cell_h) as usize;
        let column = ((x - left) / cell_w) as usize;
        let first = self.visible_first_line();
        self.input.set_caret_at(first + line, column);
        true
    }

    /// The first line of the box's text that is shown: the box scrolls to keep
    /// the caret's line in view.
    fn visible_first_line(&self) -> usize {
        let view = self.input_view();
        let rows = (self.box_rows.saturating_sub(1) as usize).max(1);
        let caret_line = view.caret.map_or(0, |c| c.0);
        (caret_line + 1)
            .saturating_sub(rows)
            .min(view.lines.len().saturating_sub(rows))
    }

    /// The height of a cell in pixels.
    #[must_use]
    pub fn cell_height(&self) -> u32 {
        self.renderer.cell().1
    }

    /// The content area's height in pixels, the columns it is drawn in and the
    /// height of the flow of blocks it shows.
    fn content_metrics(&self) -> (u32, usize, u32) {
        let (columns, rows) = window_cells(self.width, self.height, &self.renderer);
        let win = window_for_box(columns, rows, self.box_rows);
        let area = win
            .editor_row
            .saturating_sub(HEADER_ROWS)
            .saturating_mul(self.renderer.cell().1);
        let content_columns = usize::try_from(win.cols.saturating_sub(GUTTER_COLUMNS)).unwrap_or(1);
        let total = self
            .renderer
            .content_height(self.session.document(), content_columns);
        (area, content_columns, total)
    }

    /// The cell of the live grid block under pixel `(x, y)`, from 0, or `None`
    /// when the pointer is not over it. The live block is the newest block,
    /// when it is a grid: the one the program writes to. Its place is the one the
    /// renderer gives it in the flow, less how far the view is scrolled, so a
    /// pointer over scrolled-away output is not over the live block.
    #[must_use]
    pub fn live_cell(&self, x: u32, y: u32) -> Option<(u32, u32)> {
        let (col, row) = self.live_point(x, y)?;
        let Some(Block::Grid(live)) = self.session.document().blocks().last() else {
            return None;
        };
        let rows = i64::try_from(live.size().1).ok()?;
        (0..rows)
            .contains(&row)
            .then(|| (col, u32::try_from(row).unwrap_or(0)))
    }

    /// The column and row of the live grid under pixel `(x, y)`, where a row
    /// above the screen is negative: it is a line of the scrollback.
    fn live_point(&self, x: u32, y: u32) -> Option<(u32, i64)> {
        let (cw, ch) = self.renderer.cell();
        let (cw, ch) = (cw.max(1), ch.max(1));
        let doc = self.session.document();
        let Some(Block::Grid(live)) = doc.blocks().last() else {
            return None;
        };
        let (area, content_columns, total) = self.content_metrics();
        let content_x = GUTTER_COLUMNS.saturating_mul(cw) + self.renderer.gutter_pad();
        let content_y = HEADER_ROWS.saturating_mul(ch);
        let px = x.checked_sub(content_x)?;
        let py = y.checked_sub(content_y)?;
        if py >= area {
            return None;
        }
        let screen_top = self.renderer.live_screen_top(doc, content_columns)?;
        let in_flow = i64::from(py) + i64::from(self.scroll.top(total, area));
        let py = in_flow - i64::from(screen_top);
        let col = px / cw;
        (col < u32::try_from(live.size().0).ok()?).then_some((col, py.div_euclid(i64::from(ch))))
    }

    /// Whether a press of the primary button at `(x, y)` selects text. It does
    /// unless the program asked for mouse reports and the live block is where
    /// the press is; holding Shift always selects, as in other terminals.
    fn pointer_selects(&self, x: u32, y: u32, held: Held) -> bool {
        if held.shift {
            return true;
        }
        let reporting = self
            .live_modes()
            .is_some_and(|m| m.tracking != crate::mouse::MouseTracking::Off && m.sgr);
        !(reporting && self.live_cell(x, y).is_some())
    }

    /// Whether a right click at `(x, y)` is the terminal's, to open its context
    /// menu. It is unless the program asked for mouse reports and the live block
    /// is where the click is; holding Shift always opens the menu, as Shift
    /// always selects.
    #[must_use]
    pub fn owns_context_click(&self, x: u32, y: u32, held: Held) -> bool {
        self.pointer_selects(x, y, held)
    }

    /// The columns of the content area, in cells.
    fn content_columns(&self) -> usize {
        let (columns, rows) = window_cells(self.width, self.height, &self.renderer);
        let win = window_for_box(columns, rows, self.box_rows);
        usize::try_from(win.cols.saturating_sub(GUTTER_COLUMNS)).unwrap_or(1)
    }

    /// The selectable lines of the window's content.
    fn flow(&self) -> Vec<select::FlowLine> {
        self.renderer
            .flow(self.session.document(), self.content_columns())
    }

    /// The place in the flow nearest pixel `(x, y)`. With `inside` the pixel
    /// must be in the content area; without it, a pixel outside is moved to
    /// the nearest line, so a drag past an edge still selects.
    fn pos_at(&self, lines: &[select::FlowLine], x: u32, y: u32, inside: bool) -> Option<Pos> {
        let (cw, ch) = self.renderer.cell();
        let (columns, rows) = window_cells(self.width, self.height, &self.renderer);
        let win = window_for_box(columns, rows, self.box_rows);
        let content_x = GUTTER_COLUMNS.saturating_mul(cw) + self.renderer.gutter_pad();
        let content_y = HEADER_ROWS.saturating_mul(ch);
        let area_bottom = win.editor_row.saturating_mul(ch);
        if inside && (x < content_x || y < content_y || y >= area_bottom) {
            return None;
        }
        // The pixel row in the flow: the row in the area, plus how far the view is scrolled.
        let (area, _, total) = self.content_metrics();
        let cy = y
            .saturating_sub(content_y)
            .saturating_add(self.scroll.top(total, area));
        let cx = x.saturating_sub(content_x);
        let line = lines
            .iter()
            .position(|l| cy >= l.top && cy < l.top + l.height)
            .or_else(|| {
                if inside {
                    return None;
                }
                // The line nearest the pixel row.
                lines
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, l)| {
                        if cy < l.top {
                            l.top - cy
                        } else {
                            cy.saturating_sub(l.top + l.height - 1)
                        }
                    })
                    .map(|(i, _)| i)
            })?;
        let l = &lines[line];
        let col = self.renderer.char_at(l, cx.saturating_sub(l.left));
        Some(Pos { line, col })
    }

    fn begin_selection(&mut self, x: u32, y: u32) {
        let lines = self.flow();
        let had = self.selection.take().is_some();
        match self.pos_at(&lines, x, y, true) {
            Some(p) => {
                self.selection = Some(Selection { anchor: p, head: p });
                self.selecting = true;
                self.selection_changed = true;
            }
            None => self.selection_changed |= had,
        }
    }

    fn extend_selection(&mut self, x: u32, y: u32) {
        let lines = self.flow();
        let Some(p) = self.pos_at(&lines, x, y, false) else {
            return;
        };
        if let Some(sel) = &mut self.selection
            && sel.head != p
        {
            sel.head = p;
            self.selection_changed = true;
        }
    }

    fn end_selection(&mut self, x: u32, y: u32) {
        self.extend_selection(x, y);
        self.selecting = false;
        // A press and release in one place selects nothing.
        if self.selection.is_some_and(|s| s.is_empty()) {
            self.selection = None;
            self.selection_changed = true;
        } else if self.selection.is_some() {
            self.copy_pending = self.copy_on_select;
        }
    }

    /// Choose whether a finished mouse selection is copied at once.
    pub fn set_copy_on_select(&mut self, on: bool) {
        self.copy_on_select = on;
    }

    /// Whether a mouse selection just finished and the setting asks for it to
    /// be copied. The caller then does a copy, which has the same rules as an
    /// explicit one. Only a selection the terminal itself made is ever
    /// reported: a program that asked for mouse reports gets the clicks and no
    /// selection is made.
    pub fn take_copy_on_select(&mut self) -> bool {
        std::mem::take(&mut self.copy_pending)
    }

    /// Whether the selection changed since the last call, so the window draws
    /// again. The pointer calls report only what they sent to the program.
    pub fn take_selection_redraw(&mut self) -> bool {
        std::mem::take(&mut self.selection_changed)
    }

    /// The text of the selection as a copy would carry it, or `None` if
    /// nothing is selected.
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        select::copy_selection(&self.flow(), &sel).map(|c| c.text)
    }

    /// Whether a mouse selection is being dragged out now.
    #[must_use]
    pub fn is_selecting(&self) -> bool {
        self.selecting
    }

    /// Whether any text is selected.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selection.is_some_and(|s| !s.is_empty())
    }

    /// Drop the selection. Returns whether there was one.
    pub fn clear_selection(&mut self) -> bool {
        self.selecting = false;
        let had = self.selection.take().is_some();
        self.selection_changed |= had;
        had
    }

    /// Whether the view moved since the last call, for the window to draw again
    /// after an input that scrolled it.
    pub fn take_view_changed(&mut self) -> bool {
        std::mem::take(&mut self.view_moves) > 0
    }

    /// The text of the grid rows the window shows in its content area, top
    /// down. The same rows the view paints, so what a test or a search reads is
    /// what the user sees.
    #[must_use]
    pub fn visible_text(&self) -> Vec<String> {
        let (area, columns, total) = self.content_metrics();
        self.renderer.view_rows(
            self.session.document(),
            columns,
            self.scroll.top(total, area),
            area,
        )
    }

    /// Where the view is along the output.
    #[must_use]
    pub fn scroll(&self) -> Scroll {
        self.scroll
    }

    /// Whether the view is at the bottom, following new output.
    #[must_use]
    pub fn following(&self) -> bool {
        let (area, _, total) = self.content_metrics();
        self.scroll.at_bottom(total, area)
    }

    /// Whether the view is at the oldest output.
    #[must_use]
    pub fn at_top(&self) -> bool {
        let (area, _, total) = self.content_metrics();
        self.scroll.top(total, area) == 0 && total > area
    }

    /// Scroll the view by `pixels`: up for a positive value, down for a negative
    /// one. Returns whether the view moved, so the window knows to draw again.
    pub fn scroll_pixels(&mut self, pixels: i64) -> bool {
        self.glide.cancel();
        self.move_view(pixels)
    }

    fn move_view(&mut self, pixels: i64) -> bool {
        let (area, _, total) = self.content_metrics();
        let next = self.scroll.up(total, area, pixels);
        let moved = next.top(total, area) != self.scroll.top(total, area);
        self.scroll = next;
        self.view_moves += u32::from(moved);
        moved
    }

    /// Ease the view by `pixels` over the next frames: up for a positive value.
    /// A move toward a stop the view is already at is dropped. Returns whether
    /// there is movement to draw, which the next [`tick`](Self::tick) gives.
    fn glide_pixels(&mut self, pixels: i64) -> bool {
        let (area, _, total) = self.content_metrics();
        let blocked = (pixels > 0 && self.scroll.top(total, area) == 0)
            || (pixels < 0 && self.scroll.at_bottom(total, area));
        if blocked {
            self.glide.cancel();
            return false;
        }
        self.glide.add(pixels);
        self.glide.is_moving()
    }

    /// Whether wheel steps ease the view (the default) or jump it.
    pub fn set_smooth_scroll(&mut self, on: bool) {
        self.smooth_scroll = on;
        if !on {
            self.glide.cancel();
        }
    }

    /// Whether a wheel's move is still easing the view.
    #[must_use]
    pub fn is_gliding(&self) -> bool {
        self.glide.is_moving()
    }

    /// Scroll the view by `rows` rows of text: up for a positive value.
    pub fn scroll_rows(&mut self, rows: i32) -> bool {
        self.scroll_pixels(i64::from(rows) * i64::from(self.renderer.cell().1))
    }

    /// Scroll the view by a page, up or down.
    pub fn scroll_page(&mut self, up: bool) -> bool {
        self.glide.cancel();
        let (area, _, total) = self.content_metrics();
        let next = self.scroll.page(total, area, self.renderer.cell().1, up);
        let moved = next.top(total, area) != self.scroll.top(total, area);
        self.scroll = next;
        self.view_moves += u32::from(moved);
        moved
    }

    /// Scroll to the oldest output (`true`) or back to the newest.
    pub fn scroll_end(&mut self, top: bool) -> bool {
        self.glide.cancel();
        let (area, _, total) = self.content_metrics();
        let next = if top {
            Scroll::to_top(total, area)
        } else {
            Scroll::Follow
        };
        let moved = next.top(total, area) != self.scroll.top(total, area);
        self.scroll = next;
        self.view_moves += u32::from(moved);
        moved
    }

    /// A button went down at pixel `(x, y)`. If it is over the live grid block
    /// and the program asked for mouse reports, the report is sent. Returns
    /// whether anything was sent.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn mouse_press(&mut self, x: u32, y: u32, b: Button, held: Held) -> std::io::Result<bool> {
        #[cfg(feature = "links-clipboard")]
        {
            if self.links.prompt.is_some() {
                return Ok(false);
            }
            self.links.press_down = false;
        }
        if b == Button::Left && self.pointer_selects(x, y, held) {
            self.begin_selection(x, y);
        }
        let Some(cell) = self.live_cell(x, y) else {
            return Ok(false);
        };
        let Some(modes) = self.live_modes() else {
            return Ok(false);
        };
        let report = self.mouse.press(modes, b, held, cell);
        self.send_report(report)
    }

    /// A button went up at pixel `(x, y)`, which may be outside the block.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn mouse_release(
        &mut self,
        x: u32,
        y: u32,
        b: Button,
        held: Held,
    ) -> std::io::Result<bool> {
        #[cfg(feature = "links-clipboard")]
        if std::mem::take(&mut self.links.press_down) {
            // The release of a press the link prompt took.
            return Ok(false);
        }
        if b == Button::Left && self.selecting {
            self.end_selection(x, y);
            return Ok(false);
        }
        let cell = self.live_cell(x, y);
        let modes = self.live_modes().unwrap_or(crate::mouse::Modes {
            tracking: crate::mouse::MouseTracking::Off,
            sgr: false,
            wheel: false,
        });
        let report = self.mouse.release(modes, b, held, cell);
        self.send_report(report)
    }

    /// The pointer moved to pixel `(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn mouse_move(&mut self, x: u32, y: u32, held: Held) -> std::io::Result<bool> {
        #[cfg(feature = "links-clipboard")]
        if self.links.prompt.is_some() {
            return Ok(false);
        }
        if self.selecting {
            self.extend_selection(x, y);
            return Ok(false);
        }
        let (Some(cell), Some(modes)) = (self.live_cell(x, y), self.live_modes()) else {
            return Ok(false);
        };
        let report = self.mouse.motion(modes, held, cell);
        self.send_report(report)
    }

    /// The wheel turned one step with the pointer at pixel `(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn mouse_wheel(
        &mut self,
        x: u32,
        y: u32,
        step: Wheel,
        held: Held,
    ) -> std::io::Result<bool> {
        self.wheel_step(x, y, step, held, false)
    }

    /// The wheel turned one step, as [`mouse_wheel`](Self::mouse_wheel), but a
    /// step that scrolls the output eases the view there over the next frames
    /// (see [`tick`](Self::tick)) unless smooth scrolling is off. A step the
    /// program is told of is sent at once. Returns whether to draw now.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn mouse_wheel_glide(
        &mut self,
        x: u32,
        y: u32,
        step: Wheel,
        held: Held,
    ) -> std::io::Result<bool> {
        self.wheel_step(x, y, step, held, self.smooth_scroll)
    }

    fn wheel_step(
        &mut self,
        x: u32,
        y: u32,
        step: Wheel,
        held: Held,
        glide: bool,
    ) -> std::io::Result<bool> {
        #[cfg(feature = "links-clipboard")]
        if self.links.prompt.is_some() {
            return Ok(false);
        }
        if let (Some(cell), Some(modes)) = (self.live_cell(x, y), self.live_modes())
            && let Some(report) = Reporter::wheel(modes, step, held, cell)
        {
            return self.send_report(Some(report));
        }
        // The program did not ask for this wheel: it scrolls the output.
        let rows = i32::try_from(WHEEL_ROWS).unwrap_or(3);
        let rows = if matches!(step, Wheel::Up) {
            rows
        } else {
            -rows
        };
        if glide {
            let pixels = i64::from(rows) * i64::from(self.renderer.cell().1);
            Ok(self.glide_pixels(pixels))
        } else {
            Ok(self.scroll_rows(rows))
        }
    }

    /// The wheel or trackpad moved `pixels` with the pointer at pixel `(x, y)`:
    /// up for a positive value. A program that asked for wheel reports gets
    /// them, one step for each cell's height of movement; otherwise the output
    /// scrolls by exactly that many pixels.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn wheel_pixels(
        &mut self,
        x: u32,
        y: u32,
        pixels: i64,
        held: Held,
    ) -> std::io::Result<bool> {
        if pixels == 0 {
            return Ok(false);
        }
        #[cfg(feature = "links-clipboard")]
        if self.links.prompt.is_some() {
            return Ok(false);
        }
        if let (Some(cell), Some(modes)) = (self.live_cell(x, y), self.live_modes()) {
            let step = if pixels > 0 { Wheel::Up } else { Wheel::Down };
            if Reporter::wheel(modes, step, held, cell).is_some() {
                let steps = (pixels.unsigned_abs() / u64::from(self.cell_height().max(1))).max(1);
                let mut sent = false;
                for _ in 0..steps.min(10) {
                    sent |= self.send_report(Reporter::wheel(modes, step, held, cell))?;
                }
                return Ok(sent);
            }
        }
        Ok(self.scroll_pixels(pixels))
    }

    fn live_modes(&self) -> Option<crate::mouse::Modes> {
        match self.session.document().blocks().last() {
            Some(Block::Grid(g)) => Some(g.mouse_modes()),
            _ => None,
        }
    }

    fn send_report(&mut self, report: Option<Vec<u8>>) -> std::io::Result<bool> {
        let Some(bytes) = report else {
            return Ok(false);
        };
        let sent = self.send(&bytes);
        self.note_input(&sent);
        sent.map(|()| true)
    }

    /// Which toast a press at pixel `(x, y)` would dismiss, without dismissing
    /// it, as its index in [`toasts`](Self::toasts). With no notice that input
    /// was not sent, that is its index in the notifier's.
    #[must_use]
    pub fn click_would_hit(&self, x: u32, y: u32) -> Option<usize> {
        let (columns, rows) = window_cells(self.width, self.height, &self.renderer);
        let win = window_for_box(columns, rows, self.box_rows);
        self.renderer.toast_at(&win, &self.toasts(), x, y)
    }

    /// Remove the toasts whose time is up. Returns whether any was removed, so
    /// the window knows to draw again.
    pub fn tick(&mut self) -> bool {
        let before = self.notifier.shown().len();
        let now = self.session.now_ms();
        self.notifier.tick(now);
        let mut changed = self.notifier.shown().len() != before;
        let eased = self.glide.step(now);
        if eased != 0 && !self.move_view(eased) {
            // At a stop: the rest of the move has nowhere to go.
            self.glide.cancel();
        } else {
            changed |= eased != 0;
        }
        let seal_before = self.seal_notice.is_some();
        self.refresh_seal();
        changed |= self.seal_notice.is_some() != seal_before;
        if self
            .input_notice
            .is_some_and(|n| now.saturating_sub(n.last_ms) >= INPUT_NOTICE_MS)
        {
            self.input_notice = None;
            changed = true;
        }
        let phase = self.blink_on(now);
        if phase != self.cursor_phase {
            self.cursor_phase = phase;
            changed |= self.cursor_blinks();
        }
        changed
    }

    /// Whether the live grid has a cursor that blinks in a focused window.
    fn cursor_blinks(&self) -> bool {
        self.desk.focused
            && !self.exited
            && self
                .session
                .document()
                .blocks()
                .iter()
                .rev()
                .find_map(|b| match b {
                    crate::document::Block::Grid(g) => Some(g.cursor()),
                    crate::document::Block::Rich(_) => None,
                })
                .is_some_and(|c| c.visible && c.blink)
    }

    /// The on half of the blink cycle at `now`.
    fn blink_on(&self, now: u64) -> bool {
        (now.saturating_sub(self.cursor_epoch_ms) / CURSOR_BLINK_MS).is_multiple_of(2)
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
        let (columns, rows) = window_cells(width, height, &self.renderer);
        let (block_columns, block_rows) = block_cells_for(columns, rows, self.box_rows);
        self.session.resize(block_columns, block_rows)?;
        // The lines the selection is in are laid out again.
        self.clear_selection();
        self.width = width;
        self.height = height;
        self.refresh_find();
        Ok(())
    }

    /// The render scale in use now.
    #[must_use]
    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// Draw at `scale` from now on, in the same look and fonts: the cells change
    /// size, so the program is told its new window size.
    ///
    /// # Errors
    ///
    /// Returns why the renderer could not be made or the program could not be told.
    pub fn set_scale(&mut self, scale: u32) -> Result<(), SpawnError> {
        self.glide.cancel();
        // A view held somewhere in the output keeps its place by its share of
        // the flow, since the flow's height changes with the cell size and the
        // wrap.
        let (area, _, total) = self.content_metrics();
        let held = match self.scroll {
            Scroll::Pinned(_) => Some(u64::from(self.scroll.top(total, area))),
            Scroll::Follow => None,
        };
        self.renderer =
            Renderer::with_fonts(scale, self.renderer.theme().clone(), self.renderer.fonts())
                .map_err(SpawnError)?;
        self.scale = scale;
        self.resize(self.width, self.height)?;
        if let Some(top) = held {
            let (area, _, now) = self.content_metrics();
            let moved = top * u64::from(now) / u64::from(total.max(1));
            let moved = u32::try_from(moved).unwrap_or(u32::MAX);
            self.scroll = if moved >= now.saturating_sub(area) {
                Scroll::Follow
            } else {
                Scroll::Pinned(moved)
            };
        }
        Ok(())
    }

    /// Keep the scale as given: a display's scale no longer changes it. For a
    /// scale the user fixed with an option.
    pub fn pin_scale(&mut self) {
        self.scale_pinned = true;
    }

    /// The window moved to a display whose scale factor is `factor` (or its
    /// factor changed). The render scale follows it, to the nearest whole
    /// number, so text stays sharp at the display's own pixels, and a zoom the
    /// user made stays the same number of steps from the display's scale. Does
    /// nothing for a pinned scale. Returns whether the scale changed.
    ///
    /// # Errors
    ///
    /// Returns why the renderer could not be made or the program could not be told.
    pub fn set_display_scale(&mut self, factor: f64) -> Result<bool, SpawnError> {
        if self.scale_pinned {
            return Ok(false);
        }
        let base = display_scale(factor);
        let zoom = i64::from(self.scale) - i64::from(self.base_scale);
        let next = (i64::from(base) + zoom)
            .clamp(i64::from(actions::MIN_SCALE), i64::from(actions::MAX_SCALE));
        let next = u32::try_from(next).unwrap_or(base);
        self.base_scale = base;
        if next == self.scale {
            return Ok(false);
        }
        self.set_scale(next).map(|()| true)
    }

    /// Do `action` for the user. A paste reads `clipboard` and goes through the
    /// paste rules: it is sent bracketed if the program asked for that, and text with
    /// a line break or a control that is not bracketed, which the terminal cannot
    /// yet ask the user about, is pasted as a single line, and text that carries
    /// a frame introducer is not pasted at all.
    #[cfg(feature = "links-clipboard")]
    pub fn perform(&mut self, action: Action, clipboard: &dyn Clipboard) -> Outcome {
        #[cfg(feature = "links-clipboard")]
        if self.links.prompt.is_some()
            && !matches!(action, Action::Quit | Action::Close | Action::Copy)
        {
            // The prompt owns the input: a menu item or a shortcut for paste,
            // find, select all or zoom does nothing until it is answered. The
            // window can still be closed, and a copy sends nothing anywhere.
            return Outcome::Nothing;
        }
        match action {
            Action::Quit | Action::Close => Outcome::Exit,
            Action::Find => {
                self.find_open();
                Outcome::Redraw
            }
            Action::FindNext => {
                self.find_next();
                Outcome::Redraw
            }
            Action::FindPrevious => {
                self.find_previous();
                Outcome::Redraw
            }
            Action::Paste if self.find.is_some() => {
                // Into the bar, as one line.
                if let Some(text) = clipboard.text() {
                    let line = text.lines().next().unwrap_or_default().to_owned();
                    self.find_text(&line);
                }
                Outcome::Redraw
            }
            Action::Copy => match self.selected_text() {
                Some(text) if clipboard.set_text(&text) => Outcome::Nothing,
                Some(_) => Outcome::Refused,
                None => Outcome::Nothing,
            },
            Action::SelectAll => {
                self.selection = select::select_all(&self.flow());
                self.selecting = false;
                if self.selection.is_some() {
                    Outcome::Redraw
                } else {
                    Outcome::Nothing
                }
            }
            Action::Paste => {
                let Some(text) = clipboard.text() else {
                    return Outcome::Nothing;
                };
                // While the box edits, a paste lands in it for review and
                // nothing runs until Enter.
                let facts = self.facts();
                match self.input.paste(facts, &text) {
                    Line::Redraw => {
                        let _ = self.apply_line(Line::Redraw);
                        return Outcome::Redraw;
                    }
                    Line::Refused => return Outcome::Refused,
                    _ => {}
                }
                let bracketed = self.session.key_modes().bracketed_paste;
                let pasted = match paste::judge(&text, bracketed) {
                    Decision::Send(p) => p,
                    Decision::Hold(h) => h.single_line,
                    Decision::Reject(_) => return Outcome::Refused,
                };
                if pasted.text.is_empty() {
                    return Outcome::Nothing;
                }
                // A refused write is noted for the user by `text`.
                let _ = self.paste_chosen(&pasted);
                Outcome::Redraw
            }
            Action::ZoomIn | Action::ZoomOut | Action::ZoomReset => {
                match actions::zoomed(action, self.scale, self.base_scale) {
                    Some(scale) if self.set_scale(scale).is_ok() => Outcome::Redraw,
                    _ => Outcome::Nothing,
                }
            }
        }
    }

    /// A key was pressed.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn key(&mut self, key: Key, mods: Mods) -> std::io::Result<()> {
        #[cfg(feature = "links-clipboard")]
        if self.links.prompt.is_some() {
            // The prompt owns the keys: Enter opens, Escape closes, and nothing
            // reaches the program until it is answered.
            match key {
                Key::Enter => self.answer_link(true).map(|_| ()),
                Key::Escape => self.answer_link(false).map(|_| ()),
                _ => Ok(()),
            }?;
            return Ok(());
        }
        if self.scroll_key(key, mods) {
            return Ok(());
        }
        if self.find.is_some() {
            self.find_key(key, mods);
            return Ok(());
        }
        let facts = self.facts();
        if self.input.held().is_some() && self.input.mode(facts) == Mode::Editing {
            let plain = !mods.ctrl && !mods.alt;
            let outcome = match key {
                Key::Enter | Key::KeypadEnter if plain => Some(self.input.send_held_as_commands()),
                Key::Char('j' | 'J') if plain => Some(self.input.send_held_joined()),
                _ => None,
            };
            if let Some(outcome) = outcome {
                return self.apply_line(outcome);
            }
        }
        let outcome = self.input.key(facts, key, mods);
        self.apply_line(outcome)
    }

    /// What the tty and the grid say now, for the input line.
    fn facts(&mut self) -> Facts {
        let ended = self.exited || self.session.has_exited();
        self.refresh_seal();
        self.sync_recall();
        let facts = self.facts_with(ended);
        self.input.observe(facts);
        facts
    }

    /// Give the box what Up recalls: on a layer with sealing keys, the lines
    /// recorded for its sealed commands; without keys, the box's own history.
    /// A layer without keys records nothing more.
    fn sync_recall(&mut self) {
        let want = self.sealed_keys.then(|| self.typed.made());
        if !self.sealed_keys {
            self.typed.stop();
        }
        if want != self.recalling {
            self.recalling = want;
            self.input.set_recall(want.map(|_| self.typed.recall()));
        }
    }

    /// What the box typed for each sealed command, oldest first.
    #[must_use]
    pub fn typed_records(&self) -> Vec<crate::typed::Record> {
        self.typed.records().iter().cloned().collect()
    }

    /// The text of the live grid row a record's typed line ended on, while the
    /// row is still there and the history has not been rewritten since.
    #[must_use]
    pub fn typed_row_text(&self, record: &crate::typed::Record) -> Option<String> {
        let Some(Block::Grid(live)) = self.session.document().blocks().last() else {
            return None;
        };
        let row = live.row_of(record.line?)?;
        live.flow_row_text(row)
    }

    fn facts_with(&self, ended: bool) -> Facts {
        let tty = self.session.tty_state();
        let alt = matches!(self.session.document().blocks().last(), Some(Block::Grid(g)) if g.alt_active());
        Facts {
            shell_in_front: tty.shell_in_front,
            icanon: tty.icanon,
            echo: tty.echo,
            alt_screen: alt,
            keys: self.session.key_modes(),
            ended,
            sealed_prompt: if self.sealed_keys {
                self.boundaries.at_prompt()
            } else {
                None
            },
            sealed_between_commands: self.sealed_keys && self.boundaries.between_commands(),
        }
    }

    /// Do what the input line decided: write to the program, or draw the box
    /// again (which may change how many rows it takes).
    fn apply_line(&mut self, outcome: Line) -> std::io::Result<()> {
        match outcome {
            Line::Write(bytes) => {
                self.follow();
                self.cursor_epoch_ms = self.session.now_ms();
                let for_shell = self.input.wrote_for_shell();
                let whole = self.input.take_whole();
                let sent = self.forward_user_input(&bytes, for_shell, whole);
                self.sync_box();
                sent
            }
            Line::Redraw | Line::Hold => {
                self.follow();
                self.cursor_epoch_ms = self.session.now_ms();
                self.sync_box();
                Ok(())
            }
            Line::None | Line::Refused | Line::NotMine => Ok(()),
        }
    }

    /// Remember the name of the program in front, when it is not the shell. The
    /// name is for the status row only, so it is cut to a plain word: a program
    /// cannot put text of its choosing in the terminal's own chrome.
    fn refresh_front(&mut self) {
        let tty = self.session.tty_state();
        let Some(group) = tty.group.filter(|_| tty.shell_in_front == Some(false)) else {
            self.front_name = None;
            return;
        };
        if self.front_name.as_ref().is_some_and(|(g, _)| *g == group) {
            return;
        }
        // A pipeline's group leader may have ended already, so the last process
        // listed in the group is the one named.
        let out = std::process::Command::new("ps")
            .args(["-ax", "-o", "pgid=,comm="])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
        let comm = out
            .lines()
            .rev()
            .find_map(|l| {
                let (g, c) = l.trim_start().split_once(char::is_whitespace)?;
                (g.parse::<i32>().ok()? == group).then_some(c.trim())
            })
            .unwrap_or("");
        let base = comm.rsplit('/').next().unwrap_or("");
        let name: String = base
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            .take(24)
            .collect();
        self.front_name = Some((group, name));
    }

    /// What the input box shows now.
    fn input_view(&self) -> InputView {
        let facts = self.facts_with(self.exited);
        let mode = self.input.mode(facts);
        let name = self
            .front_name
            .as_ref()
            .map(|(_, n)| n.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or("the program");
        let status = match mode {
            Mode::Editing => None,
            Mode::Ended => Some("The program has ended".to_owned()),
            Mode::Hidden => Some(format!("Keys go to {name} · input not shown")),
            Mode::HandedOff => Some("Editing in the shell: Enter runs the line".to_owned()),
            Mode::Raw if facts.shell_in_front == Some(true) => {
                Some("Keys go to the shell".to_owned())
            }
            Mode::Raw => Some(format!("Keys go to {name}")),
        };
        let text = self.input.text();
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let caret = {
            let before: String = text.chars().take(self.input.caret()).collect();
            let line = before.matches('\n').count();
            let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count());
            Some((line, col))
        };
        let hint = (mode == Mode::Editing)
            .then(|| self.input.held())
            .flatten()
            .map(|h| {
                let n = h.lines().count();
                format!("Send as {n} commands: Enter. Join as one line: J. Cancel: Esc")
            });
        InputView {
            lines,
            caret,
            focused: self.desk.focused,
            status,
            hint,
        }
    }

    /// The text in the input box.
    #[must_use]
    pub fn input_text(&self) -> String {
        self.input.text()
    }

    /// Where keys go now: the box, or the program.
    pub fn input_mode(&mut self) -> Mode {
        let facts = self.facts();
        self.input.mode(facts)
    }

    /// The one place the window writes what the user typed to the program: the
    /// box's line and its Enter, an Enter while handed off, a raw Enter while
    /// the shell is in front, each line of "send as commands", and a paste
    /// sent to the program. Nothing else writes a carriage return for the user.
    ///
    /// Each Enter in the write first runs the sealing checks the layer must
    /// pass at every forwarded Enter (a shell that executed another program
    /// loses its keys here). An Enter `for_shell`, which ends a line the shell
    /// runs, is counted, and the box waits for the shell's next frame.
    ///
    /// On a sealed layer a write with more than one Enter for the shell
    /// (send as commands, a paste with line breaks) goes one line at a time:
    /// the first now, each next one when the shell is back at a sealed prompt,
    /// checked again just before it is written. A new write while lines are
    /// still queued puts them back into the box.
    fn forward_user_input(
        &mut self,
        bytes: &[u8],
        for_shell: bool,
        whole: bool,
    ) -> std::io::Result<()> {
        if !self.queued.is_empty() {
            self.put_back_queued();
        }
        if for_shell && self.sealed_keys {
            let mut parts = crate::input_line::split_at_enters(bytes);
            if parts.len() > 1 {
                let first = parts.remove(0);
                self.queued.extend(parts);
                self.queued_whole = whole;
                return self.write_user_input(&first, for_shell, whole);
            }
        }
        self.write_user_input(bytes, for_shell, whole)
    }

    /// Write `bytes` the user typed: the checks first if it holds an Enter,
    /// then the write, then the count.
    fn write_user_input(
        &mut self,
        bytes: &[u8],
        for_shell: bool,
        whole: bool,
    ) -> std::io::Result<()> {
        let enters = crate::input_line::enters_in(bytes);
        if enters > 0 {
            self.session.recheck_seal();
            self.refresh_seal();
        }
        let sent = self.send(bytes);
        if sent.is_ok() && for_shell && enters > 0 {
            self.forwarded_enters = self.forwarded_enters.saturating_add(u64::from(enters));
            self.boundaries.line_sent();
        }
        if sent.is_ok() && for_shell {
            // Only a write of one line, the box's own text, is a line typed in
            // the box; anything else on the shell's line means no record.
            let line =
                (whole && enters == 1).then(|| crate::input_line::unsent_text(&[bytes.to_vec()]));
            self.typed.wrote(line);
        }
        self.note_input(&sent);
        sent
    }

    /// How many Enters the window has forwarded for the user that end a line
    /// the shell runs: one per line the box sent, per Enter for a line the
    /// shell holds or has typeahead for, per raw Enter while the shell is in
    /// front, and per line of "send as commands".
    #[must_use]
    pub fn forwarded_enters(&self) -> u64 {
        self.forwarded_enters
    }

    /// Give the editor as many rows as its text has lines, up to the most the
    /// box shows, and tell the program its window changed if that moved.
    fn sync_box(&mut self) {
        let lines = if self.input.text().is_empty() {
            1
        } else {
            self.input.text().split('\n').count()
        };
        let rows = 1 + u32::try_from(lines.clamp(1, INPUT_MAX_LINES)).unwrap_or(1);
        if rows != self.box_rows {
            self.box_rows = rows;
            let _ = self.resize(self.width, self.height);
        }
    }

    /// Text was pasted or typed as a whole (an input method's result).
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn text(&mut self, text: &str) -> std::io::Result<()> {
        #[cfg(feature = "links-clipboard")]
        if self.links.prompt.is_some() {
            return Ok(());
        }
        if self.find.is_some() {
            self.find_text(text);
            return Ok(());
        }
        if text.is_empty() {
            return Ok(());
        }
        let facts = self.facts();
        let outcome = self.input.text_input(facts, text);
        self.apply_line(outcome)
    }

    /// Whether a key scrolls the output instead of going to the program: Page Up
    /// and Page Down (with Shift or alone) and Shift or Control with Home and
    /// End, while the program is on the main screen. A program on the alternate
    /// screen keeps every key, because it draws its own pages.
    fn scroll_key(&mut self, key: Key, mods: Mods) -> bool {
        if matches!(self.session.document().blocks().last(), Some(Block::Grid(g)) if g.alt_active())
            || mods.alt
        {
            return false;
        }
        let plain_or_shift = !mods.ctrl;
        match key {
            Key::PageUp if plain_or_shift => {
                self.scroll_page(true);
            }
            Key::PageDown if plain_or_shift => {
                self.scroll_page(false);
            }
            Key::Home if mods.shift || mods.ctrl => {
                self.scroll_end(true);
            }
            Key::End if mods.shift || mods.ctrl => {
                self.scroll_end(false);
            }
            _ => return false,
        }
        true
    }

    /// Return to the newest output, as typing does.
    fn follow(&mut self) {
        self.glide.cancel();
        if self.scroll != Scroll::Follow {
            self.scroll = Scroll::Follow;
            self.view_moves += 1;
        }
    }

    /// The input method is composing `text`, with the caret or selection at
    /// `cursor` (byte offsets into `text`). Empty text ends the composition. The
    /// text is kept for drawing and is not sent to the program.
    pub fn set_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        let kept: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(PREEDIT_MAX_CHARS)
            .collect();
        if kept.is_empty() {
            self.preedit = None;
            return;
        }
        let cursor =
            cursor.filter(|&(a, b)| a <= b && kept.is_char_boundary(a) && kept.is_char_boundary(b));
        self.preedit = Some(Preedit { text: kept, cursor });
    }

    /// The text being composed and its caret or selection, if a composition is
    /// in progress.
    #[must_use]
    pub fn preedit(&self) -> Option<(&str, Option<(usize, usize)>)> {
        self.preedit.as_ref().map(|p| (p.text.as_str(), p.cursor))
    }

    /// Drop any composition without sending it (focus lost, the input method
    /// turned off, or the user cancelled).
    pub fn clear_preedit(&mut self) {
        self.preedit = None;
    }

    /// Composition finished with `text`: the preedit ends and the text goes to
    /// the program once. Control characters are dropped, so a committed string
    /// can never carry an escape sequence to the program.
    ///
    /// # Errors
    ///
    /// Returns the write error.
    pub fn commit(&mut self, text: &str) -> std::io::Result<()> {
        self.preedit = None;
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return Ok(());
        }
        self.text(&clean)
    }

    /// Where the input method's candidate window goes, in pixels: `(x, y, width,
    /// height)`, the editor's first row after the gutter and its padding, as wide as the text
    /// being composed. The terminal keeps no cursor cell of its own.
    #[must_use]
    pub fn ime_area(&self) -> (u32, u32, u32, u32) {
        let (cell_w, cell_h) = self.renderer.cell();
        let (columns, rows) = window_cells(self.width, self.height, &self.renderer);
        let win = window_for_box(columns, rows, self.box_rows);
        let used = self
            .preedit
            .as_ref()
            .map_or(0, |p| p.text.chars().map(cell_columns).sum::<u32>());
        let (line, col) = self.input_view().caret.unwrap_or((0, 0));
        let line = u32::try_from(line.saturating_sub(self.visible_first_line())).unwrap_or(0);
        let col = u32::try_from(col).unwrap_or(0);
        (
            GUTTER_COLUMNS
                .saturating_mul(cell_w)
                .saturating_add(self.renderer.gutter_pad())
                .saturating_add(col.saturating_add(3).saturating_mul(cell_w)),
            win.editor_row.saturating_add(line).saturating_mul(cell_h),
            used.max(1).saturating_mul(cell_w),
            cell_h,
        )
    }

    /// The user pasted `text`. The text is sanitised first (see [`paste::judge`]).
    /// If the program has turned bracketed paste on (DECSET 2004) the text is sent
    /// between `ESC [ 200 ~` and `ESC [ 201 ~`, as one unit, whatever its length
    /// and line breaks. If not, a paste with a line break or another control is
    /// not sent but returned as [`PasteOutcome::Held`], for the window to show
    /// and for [`paste_chosen`](Self::paste_chosen) to send once the user picks.
    ///
    /// # Errors
    ///
    /// Returns the write error; the user is shown the not-sent notice.
    #[cfg(feature = "links-clipboard")]
    pub fn paste(&mut self, text: &str) -> std::io::Result<PasteOutcome> {
        let facts = self.facts();
        if self.input.paste(facts, text) == Line::Redraw {
            self.apply_line(Line::Redraw)?;
            return Ok(PasteOutcome::Sent);
        }
        let bracketed = self.session.key_modes().bracketed_paste;
        match paste::judge(text, bracketed) {
            Decision::Reject(why) => Ok(PasteOutcome::Rejected(why)),
            Decision::Hold(hold) => Ok(PasteOutcome::Held(hold)),
            Decision::Send(pasted) => self.paste_chosen(&pasted).map(|()| PasteOutcome::Sent),
        }
    }

    /// Send a paste that is already judged: the text of a [`Pasted`] the user
    /// chose from a held paste, or one [`paste`](Self::paste) found safe. It is
    /// bracketed if the program has asked for that.
    ///
    /// # Errors
    ///
    /// Returns the write error; the user is shown the not-sent notice.
    #[cfg(feature = "links-clipboard")]
    pub fn paste_chosen(&mut self, pasted: &Pasted) -> std::io::Result<()> {
        let mut bytes = Vec::with_capacity(pasted.text.len() + 12);
        let bracketed = self.session.key_modes().bracketed_paste;
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(pasted.text.as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        // A paste the program does not take bracketed runs a line at each line
        // break, so it goes the way every Enter does.
        let for_shell = self.session.tty_state().shell_in_front == Some(true);
        self.forward_user_input(&bytes, for_shell, false)
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

    /// Ask the session again whether the layer has sealing keys. A layer that
    /// had them and has lost them gets the terminal's notice saying why. Once
    /// the program has ended the layer is closed, and its notice goes with it.
    fn refresh_seal(&mut self) {
        let had = self.sealed_keys;
        self.sealed_keys = self.session.has_sealed_keys();
        if self.exited || self.session.has_exited() {
            self.seal_notice = None;
            return;
        }
        if had && !self.sealed_keys {
            let why = self
                .session
                .seal_off_reason()
                .unwrap_or_else(|| "the layer lost its keys".to_owned());
            self.seal_notice = Some(SealNotice {
                text: format!("Sealing ended: {why}"),
                shown_ms: self.session.now_ms(),
            });
        }
    }

    /// The terminal's notice that the shell layer has no sealing keys, if it
    /// is shown. For tests and the window.
    #[must_use]
    pub fn seal_notice(&self) -> Option<&str> {
        self.seal_notice.as_ref().map(|n| n.text.as_str())
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
        self.pump_taking(None)
    }

    /// Wait up to `wait` for the program's next output, then take it as
    /// [`pump`](Self::pump) does. The wait ends as soon as output arrives.
    pub fn pump_wait(&mut self, wait: std::time::Duration) -> usize {
        self.pump_taking(Some(wait))
    }

    fn pump_taking(&mut self, wait: Option<std::time::Duration>) -> usize {
        self.refresh_front();
        let facts = self.facts_with(self.exited);
        self.input.observe(facts);
        let bytes = match wait {
            Some(wait) => self.session.pump_wait(wait),
            None => self.session.pump(),
        };
        if bytes > 0 {
            self.cursor_epoch_ms = self.session.now_ms();
            self.refresh_find();
        }
        let events = self.session.take_events();
        if !events.is_empty() {
            self.route(&events);
            // A sealed command start ends a line the shell was holding.
            let facts = self.facts();
            self.input.observe(facts);
        }
        self.send_queued();
        self.tick();
        bytes
    }

    /// Send the next queued line once the shell is back at a sealed prompt and
    /// its line editor reads, after the checks every forwarded Enter must pass
    /// (they also ran when the prompt arrived). If the layer has lost its
    /// keys, or the shell's line now holds something else, the lines left go
    /// back into the box unsent.
    fn send_queued(&mut self) {
        if self.queued.is_empty() {
            return;
        }
        let facts = self.facts();
        if !self.sealed_keys || self.input.line_is_held() {
            self.put_back_queued();
            return;
        }
        if facts.sealed_prompt != Some(true) || self.input.mode(facts) != Mode::Editing {
            return;
        }
        self.session.recheck_seal();
        self.refresh_seal();
        if !self.sealed_keys {
            self.put_back_queued();
            return;
        }
        self.send_next_queued();
    }

    /// Write the next queued line. If the write fails (the program has ended
    /// or is not reading), that line and the rest go back into the box unsent.
    fn send_next_queued(&mut self) {
        let Some(next) = self.queued.pop_front() else {
            return;
        };
        if self
            .write_user_input(&next, true, self.queued_whole)
            .is_err()
        {
            self.queued.push_front(next);
            self.put_back_queued();
        }
    }

    fn put_back_queued(&mut self) {
        self.queued_whole = false;
        let parts: Vec<Vec<u8>> = self.queued.drain(..).collect();
        self.input.put_back(&crate::input_line::unsent_text(&parts));
        self.sync_box();
    }

    /// The window title to show if the program changed it since the last call:
    /// its title with the terminal's name, or the name alone after an empty or
    /// reset title. For the window chrome only; it is never drawn in the page.
    pub fn take_window_title(&mut self) -> Option<String> {
        self.session
            .take_title()
            .map(|t| crate::title::window_title(Some(&t)))
    }

    /// What the window should tell the system about the bell since the last
    /// call. The bell is always taken, so it cannot pile up.
    pub fn attention_step(&mut self) -> crate::attention::Step {
        let rang = self.session.take_bell();
        let now = self.session.now_ms();
        self.attention.step(rang, now, self.desk.focused)
    }

    /// Whether the program has exited.
    pub fn has_exited(&mut self) -> bool {
        self.exited = self.session.has_exited();
        self.exited
    }

    /// The window's contents: exactly `width` by `height` pixels.
    #[must_use]
    pub fn view(&self) -> Frame {
        let (columns, rows) = window_cells(self.width, self.height, &self.renderer);
        let editing_box = self.input.mode(self.facts_with(self.exited)) == Mode::Editing;
        let cursor = (!self.exited).then(|| crate::raster::CursorState {
            // While the box is the place to type, the grid's cursor is drawn
            // unfocused: there is one typeable place at a time.
            focused: self.desk.focused && !editing_box,
            blink_on: self.blink_on(self.session.now_ms()),
        });
        #[allow(
            unused_mut,
            reason = "the link prompt is drawn only with links-clipboard"
        )]
        let mut frame = self
            .renderer
            .render_window_input(
                self.session.document(),
                &window_for_box(columns, rows, self.box_rows),
                &self.toasts(),
                self.selection.as_ref(),
                self.scroll,
                (cursor, self.find_overlay().as_ref()),
                Some(&self.input_view()),
            )
            .viewport(self.width, self.height, self.renderer.theme().background);
        #[cfg(feature = "links-clipboard")]
        if let Some(prompt) = &self.links.prompt {
            // Last, over everything: a program's output cannot cover it.
            self.renderer.paint_link_prompt(&mut frame, prompt);
        }
        frame
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::App;
    use crate::layer::Passthrough;

    #[test]
    fn a_queued_line_whose_write_fails_goes_back_to_the_box_with_the_rest() {
        // Once the program has ended every write is refused. Its tty closing
        // ends the output stream, which is what the wait waits for.
        let mut app = App::start(
            Box::new(Passthrough),
            "/bin/sh",
            &["-c", "exit 0"],
            1,
            800,
            600,
        )
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !app.session.has_exited() {
            let left = end.saturating_duration_since(std::time::Instant::now());
            assert!(!left.is_zero(), "the program ended");
            app.pump_wait(left);
        }
        app.queued
            .extend([b"echo two\r".to_vec(), b"echo three\r".to_vec()]);
        app.send_next_queued();
        assert!(app.queued.is_empty(), "nothing is left queued");
        assert_eq!(
            app.input.text(),
            "echo two\necho three",
            "the line whose write failed and the one after it are back in the box"
        );
        assert_eq!(app.forwarded_enters(), 0, "no Enter was counted");
    }
}
