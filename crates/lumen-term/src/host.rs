//! The window: winit for the window and its events, softbuffer to put the
//! frame on it. Everything it does is a call on [`App`], which is tested
//! without a window; this file holds only what turns an OS event into one.

use crate::actions::{self, Action, Platform};
use crate::app::{App, Outcome};
use crate::clipboard::{Clipboard, SystemClipboard};
use crate::design::Appearance;
use crate::keys::{Key, Mods};
use crate::layer::Layer;
use crate::mouse::{Button, Held, Wheel};
use crate::panes::{Layout, Panes, Split};
use crate::raster::Frame;
use crate::settings::Watcher as SettingsWatcher;
use crate::tabs::Tabs;
use crate::window_state::{Placement, Store as WindowStore};
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
#[cfg(not(target_os = "macos"))]
use winit::event_loop::ControlFlow;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key as WinitKey, KeyLocation, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Theme as WinitTheme, Window, WindowId};

const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// What wakes the event loop from outside it.
#[derive(Debug)]
enum UserEvent {
    /// The frame wake thread asks for a redraw pass (macOS).
    #[cfg(target_os = "macos")]
    Wake,
    /// A screen reader or other assistive technology asks something.
    Access(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Access(event)
    }
}

/// A timed wait on macOS can stall until input arrives. Wake the event loop
/// from a worker and keep at most one wake queued while the window is busy.
#[cfg(any(target_os = "macos", all(test, unix)))]
fn start_frame_wakes(
    wake: impl Fn() -> bool + Send + 'static,
) -> std::io::Result<std::sync::mpsc::Sender<()>> {
    use std::sync::mpsc::{RecvTimeoutError, channel};

    let (handled, pending) = channel();
    std::thread::Builder::new()
        .name("window-frame-wake".into())
        .spawn(move || {
            while pending.recv_timeout(FRAME_INTERVAL) == Err(RecvTimeoutError::Timeout) {
                if !wake() || pending.recv().is_err() {
                    break;
                }
            }
        })?;
    Ok(handled)
}

fn pump_frame(app: &mut App) -> bool {
    let animated = app.tick();
    app.pump() > 0 || animated
}

/// A pointer position in physical pixels as whole pixels: negative and
/// non-finite positions are the edge, and nothing is larger than the window.
#[must_use]
pub fn pointer_pixels(x: f64, y: f64) -> (u32, u32) {
    let whole = |v: f64| {
        if v.is_finite() && v > 0.0 {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "positive and clamped to u32 range"
            )]
            let n = v.min(f64::from(u32::MAX)) as u32;
            n
        } else {
            0
        }
    };
    (whole(x), whole(y))
}

/// Most wheel steps one window event sends.
pub const MAX_WHEEL_STEPS: u32 = 10;

/// The wheel steps a scroll event makes: the direction and how many, at least
/// one for any movement and at most [`MAX_WHEEL_STEPS`]. `None` when it did not
/// move. A pixel delta counts a step for each `line` pixels.
#[must_use]
pub fn wheel_steps(delta: &MouseScrollDelta, line: f64) -> Option<(Wheel, u32)> {
    let y = match *delta {
        MouseScrollDelta::LineDelta(_, y) => f64::from(y),
        MouseScrollDelta::PixelDelta(p) => p.y / line.max(1.0),
    };
    if !y.is_finite() || y == 0.0 {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to a small positive range first"
    )]
    let n = y.abs().round().clamp(1.0, f64::from(MAX_WHEEL_STEPS)) as u32;
    Some((if y > 0.0 { Wheel::Up } else { Wheel::Down }, n))
}

/// The pixels a trackpad moved, as a whole number: up for a positive value.
/// A movement that is not a number counts as none.
#[must_use]
pub fn wheel_pixels(y: f64) -> i64 {
    if y.is_finite() {
        #[allow(clippy::cast_possible_truncation, reason = "a pixel count")]
        let n = y.round().clamp(-100_000.0, 100_000.0) as i64;
        n
    } else {
        0
    }
}

/// A window button to the button the program is told of. `None` for the others.
#[must_use]
pub fn map_button(b: MouseButton) -> Option<Button> {
    match b {
        MouseButton::Left => Some(Button::Left),
        MouseButton::Middle => Some(Button::Middle),
        MouseButton::Right => Some(Button::Right),
        _ => None,
    }
}

/// A window key to the key the program gets, with its modifiers. `None` for a
/// key that sends nothing, or that belongs to the operating system (Super).
///
/// `text` is what the key produced, which carries shift and layout; with Control
/// held the logical key is used instead, since the text is a control code then.
#[must_use]
pub fn map_key(key: &WinitKey, text: Option<&str>, state: ModifiersState) -> Option<(Key, Mods)> {
    map_key_at(key, text, state, KeyLocation::Standard)
}

/// [`map_key`] for a key at `location`: a digit, operator or Enter on the
/// numeric keypad becomes a keypad key, which a program that has set the
/// application keypad sees differently from the same character typed above the
/// letters.
#[must_use]
pub fn map_key_at(
    key: &WinitKey,
    text: Option<&str>,
    state: ModifiersState,
    location: KeyLocation,
) -> Option<(Key, Mods)> {
    let (mapped, mods) = map_key_for(key, text, state, cfg!(target_os = "macos"))?;
    if location != KeyLocation::Numpad {
        return Some((mapped, mods));
    }
    let keypad = match mapped {
        Key::Enter => Key::KeypadEnter,
        Key::Char(c) if "0123456789*+,-./=".contains(c) => Key::Keypad(c),
        other => other,
    };
    Some((keypad, mods))
}

/// [`map_key`] with the platform's Option behaviour given. Where `option_composes`
/// is set (macOS), Alt held with a key that produced non-ASCII text means the
/// layout made that text (Option+e then e is an accented letter), so the text
/// is sent as typed and not with an Escape prefix. ASCII text stays Alt.
#[must_use]
pub fn map_key_for(
    key: &WinitKey,
    text: Option<&str>,
    state: ModifiersState,
    option_composes: bool,
) -> Option<(Key, Mods)> {
    if state.super_key() {
        return None;
    }
    let produced = text.is_some_and(|t| !t.is_ascii() && t.chars().all(|c| !c.is_control()));
    let mods = Mods {
        ctrl: state.control_key(),
        alt: state.alt_key()
            && !(option_composes && produced && matches!(key, WinitKey::Character(_))),
        shift: state.shift_key(),
    };
    let mapped = match key {
        WinitKey::Named(named) => match named {
            NamedKey::Enter => Key::Enter,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Tab => Key::Tab,
            NamedKey::Escape => Key::Escape,
            NamedKey::Space => Key::Char(' '),
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::Insert => Key::Insert,
            NamedKey::Delete => Key::Delete,
            NamedKey::F1 => Key::F(1),
            NamedKey::F2 => Key::F(2),
            NamedKey::F3 => Key::F(3),
            NamedKey::F4 => Key::F(4),
            NamedKey::F5 => Key::F(5),
            NamedKey::F6 => Key::F(6),
            NamedKey::F7 => Key::F(7),
            NamedKey::F8 => Key::F(8),
            NamedKey::F9 => Key::F(9),
            NamedKey::F10 => Key::F(10),
            NamedKey::F11 => Key::F(11),
            NamedKey::F12 => Key::F(12),
            _ => return None,
        },
        WinitKey::Character(logical) => {
            let source = if mods.ctrl {
                logical.as_str()
            } else {
                text.filter(|t| !t.is_empty()).unwrap_or(logical.as_str())
            };
            let mut chars = source.chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                // More than one character (a composed result): the whole string
                // is sent by the caller as text, not as a key.
                return None;
            }
            Key::Char(first)
        }
        _ => return None,
    };
    Some((mapped, mods))
}

/// Typed input the window sends to the program.
#[derive(Debug, Clone, Copy)]
pub enum Input<'a> {
    /// A key with its modifiers.
    Key(Key, Mods),
    /// Text typed or pasted as a whole.
    Text(&'a str),
}

/// Apply an input method event to the app. Returns whether the window must draw
/// again. Composition in progress is kept, not sent; the committed text is sent
/// once and ends it; turning the input method off drops what was unfinished.
pub fn ime_event(app: &mut App, event: &Ime) -> bool {
    match event {
        Ime::Enabled => false,
        Ime::Preedit(text, cursor) => {
            let before = app.preedit().map(|(t, c)| (t.to_owned(), c));
            app.set_preedit(text, *cursor);
            before != app.preedit().map(|(t, c)| (t.to_owned(), c))
        }
        Ime::Commit(text) => {
            let had = app.preedit().is_some();
            // Commit ends the composition even when it sends nothing, and a
            // refused write puts the app's own notice on screen.
            let _ = app.commit(text);
            had || !text.is_empty()
        }
        Ime::Disabled => {
            let had = app.preedit().is_some();
            app.clear_preedit();
            had
        }
    }
}

/// The candidate window's rectangle: `(x, y, width, height)` in pixels.
pub type ImeRect = (u32, u32, u32, u32);

/// Where the candidate rectangle goes: the window, which hands it to the system.
pub trait ImeSink {
    /// The input method's candidate window should now sit at `area`.
    fn set_area(&mut self, area: ImeRect);
}

/// Keeps the system's candidate rectangle where the app says it is. The
/// rectangle moves with the window size, the render scale and the text being
/// composed, so it is checked after every event that can change them, and given
/// to the sink only when it differs from what the sink already has.
#[derive(Debug, Default)]
pub struct ImeFollower {
    last: Option<ImeRect>,
}

impl ImeFollower {
    /// Give `sink` the app's candidate rectangle if it changed. Returns whether it did.
    pub fn follow(&mut self, app: &App, sink: &mut dyn ImeSink) -> bool {
        let now = app.ime_area();
        if self.last == Some(now) {
            return false;
        }
        self.last = Some(now);
        sink.set_area(now);
        true
    }

    /// Forget the last rectangle, so a newly active tab tells the system its
    /// own candidate position even when the numbers happen to match.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// Apply a window event that changes the app's size, focus or composition, then
/// keep the candidate rectangle with it. Returns whether the window must draw
/// again. Other events do nothing here.
pub fn apply_window_event(
    app: &mut App,
    event: &WindowEvent,
    ime: &mut ImeFollower,
    sink: &mut dyn ImeSink,
) -> bool {
    let redraw = match event {
        WindowEvent::Resized(PhysicalSize { width, height }) => app.resize(*width, *height).is_ok(),
        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            app.set_display_scale(*scale_factor).unwrap_or(false)
        }
        WindowEvent::Ime(event) => ime_event(app, event),
        WindowEvent::Focused(focused) => {
            app.set_focused(*focused);
            if !*focused {
                app.clear_preedit();
            }
            // The cursor is drawn dimmed when the window loses focus.
            true
        }
        _ => false,
    };
    ime.follow(app, sink);
    redraw
}

/// Do a window action (a zoom changes the cell size), then keep the candidate
/// rectangle with it.
pub fn perform_action(
    app: &mut App,
    action: Action,
    clipboard: &dyn Clipboard,
    ime: &mut ImeFollower,
    sink: &mut dyn ImeSink,
) -> Outcome {
    let outcome = app.perform(action, clipboard);
    ime.follow(app, sink);
    outcome
}

/// The window as an [`ImeSink`].
struct WindowSink<'a> {
    window: &'a Window,
    left: u32,
    top: u32,
}

impl ImeSink for WindowSink<'_> {
    fn set_area(&mut self, (x, y, w, h): ImeRect) {
        self.window.set_ime_cursor_area(
            PhysicalPosition::new(x.saturating_add(self.left), y.saturating_add(self.top)),
            PhysicalSize::new(w, h),
        );
    }
}

/// Send `input` to the app. Returns whether an edit, scroll or write failure
/// needs a new frame.
pub fn send_input(app: &mut App, input: Input<'_>) -> bool {
    let sent = match input {
        Input::Key(key, mods) => app.key(key, mods),
        Input::Text(text) => app.text(text),
    };
    sent.is_err() | app.take_view_changed()
}

/// A frame as the 0RGB words softbuffer wants.
#[must_use]
pub fn to_xrgb(frame: &Frame) -> Vec<u32> {
    let (pixels, _) = frame.rgba.as_chunks::<4>();
    pixels
        .iter()
        .map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]))
        .collect()
}

/// What the window needs to start the terminal.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is one independent setting the user turns on or off"
)]
pub struct Config {
    /// Builds the layer the program's output goes through.
    pub layer: Box<dyn Fn() -> Box<dyn Layer + Send>>,
    /// The program and its arguments.
    pub command: Vec<String>,
    /// Initial window size in logical pixels.
    pub size: (u32, u32),
    /// Render scale; `None` follows the screen's scale factor.
    pub scale: Option<u32>,
    /// Close the window after this long.
    pub exit_after: Option<Duration>,
    /// What the user allowed for notifications.
    pub notifications: crate::notify::Settings,
    /// The user's `images` setting; `false` always shows the alt text.
    pub images: bool,
    /// The user's settings file and its current value.
    pub settings: SettingsWatcher,
    /// The notice for a policy file that was ignored, if it was. It stays on
    /// screen while the defaults apply.
    pub policy_notice: Option<String>,
}

struct Tab {
    panes: Panes<App>,
    title: String,
}

/// The least a pane may be after a split, in cells.
const MIN_PANE: (usize, usize) = (20, 5);

impl Tab {
    fn new(app: App, title: String) -> Self {
        Self {
            panes: Panes::new(app),
            title,
        }
    }

    /// The focused pane.
    fn app(&self) -> &App {
        self.panes.focused()
    }

    /// The focused pane, for a change.
    fn app_mut(&mut self) -> &mut App {
        self.panes.focused_mut()
    }

    /// The height of the tab strip above the panes.
    fn strip(&self) -> u32 {
        self.app().cell_height()
    }

    /// The thickness of the divider between panes.
    fn gap(&self) -> u32 {
        2 * self.app().scale().max(1)
    }

    /// Where the panes go in a window of `size`, below the strip.
    fn layout(&self, size: PhysicalSize<u32>) -> Layout {
        let height = size.height.saturating_sub(self.strip());
        self.panes.layout(size.width, height, self.gap())
    }

    /// The window pixel the focused pane's top left is at.
    fn origin(&self, size: PhysicalSize<u32>) -> (u32, u32) {
        let at = self
            .layout(size)
            .panes
            .get(self.panes.focus_index())
            .map_or((0, 0), |r| (r.x, r.y));
        (at.0, at.1 + self.strip())
    }

    /// The pane under window pixel `(x, y)` and the pixel in its own terms.
    fn pane_at(&self, size: PhysicalSize<u32>, x: u32, y: u32) -> Option<(usize, u32, u32)> {
        let y = y.checked_sub(self.strip())?;
        let layout = self.layout(size);
        let index = layout.panes.iter().position(|r| r.contains(x, y))?;
        let r = layout.panes[index];
        Some((index, x - r.x, y - r.y))
    }

    /// How many pixels either side of a divider a press still takes hold of it.
    fn grab(&self) -> u32 {
        2 * self.app().scale().max(1)
    }

    /// The divider under window pixel `(x, y)`, and which way it divides.
    fn divider_at(&self, size: PhysicalSize<u32>, x: u32, y: u32) -> Option<(usize, Split)> {
        let y = y.checked_sub(self.strip())?;
        let height = size.height.saturating_sub(self.strip());
        let area = (size.width, height, self.gap());
        let index = self.panes.divider_at(area, x, y, self.grab())?;
        let layout = self.panes.layout(area.0, area.1, area.2);
        layout.splits.get(index).map(|(split, _)| (index, *split))
    }

    /// Move divider `index` towards window pixel `(x, y)`, as far as every
    /// pane keeps the least a split leaves it, and size the panes to their new
    /// places. Whether it moved.
    fn drag_divider(&mut self, size: PhysicalSize<u32>, index: usize, x: u32, y: u32) -> bool {
        let height = size.height.saturating_sub(self.strip());
        let gap = self.gap();
        let Some((split, _)) = self
            .panes
            .layout(size.width, height, gap)
            .splits
            .get(index)
            .copied()
        else {
            return false;
        };
        let at = match split {
            Split::Right => x,
            Split::Down => y.saturating_sub(self.strip()),
        };
        let moved = self
            .panes
            .drag(index, at, (size.width, height, gap), |app, w, h| {
                app.pane_has(w, h, MIN_PANE.0, MIN_PANE.1)
            });
        if moved {
            self.resize(size);
        }
        moved
    }

    /// Pane `index`'s own terms for window pixel `(x, y)`, held at its edge
    /// when the pixel is outside it (a drag that began there).
    fn pane_clamped(
        &self,
        size: PhysicalSize<u32>,
        index: usize,
        x: u32,
        y: u32,
    ) -> Option<(u32, u32)> {
        let layout = self.layout(size);
        let r = layout.panes.get(index)?;
        let y = y.saturating_sub(self.strip());
        Some((
            x.saturating_sub(r.x).min(r.w.saturating_sub(1)),
            y.saturating_sub(r.y).min(r.h.saturating_sub(1)),
        ))
    }

    /// Every pane, for a change.
    fn apps_mut(&mut self) -> Vec<&mut App> {
        self.panes.all_mut()
    }

    /// Size every pane to its place in a window of `size`. Whether any did.
    fn resize(&mut self, size: PhysicalSize<u32>) -> bool {
        let layout = self.layout(size);
        let mut changed = false;
        for (app, r) in self.panes.all_mut().into_iter().zip(layout.panes) {
            changed |= app.resize(r.w, r.h).is_ok();
        }
        changed
    }
}

struct Running {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    tabs: Tabs<Tab>,
    shown_title: String,
    modifiers: ModifiersState,
    pointer: (u32, u32),
    ime: ImeFollower,
    focused: bool,
    mouse_capture: Option<MouseButton>,
    /// The pane a held button went down in; the drag stays there.
    capture_pane: usize,
    /// The divider of the active tab being dragged, while the button is held.
    divider_drag: Option<usize>,
    /// The divider the pointer is over, by the way it divides, for the
    /// pointer's shape.
    divider_hover: Option<Split>,
    placement: Placement,
    /// The window's connection to the platform's accessibility.
    access: accesskit_winit::Adapter,
    /// Whether an assistive technology is connected and wants the tree.
    access_on: bool,
    /// The facts the reader was last told, so an unchanged screen sends nothing.
    access_told: Option<crate::access::Snapshot>,
}

struct Host {
    config: Config,
    windows: Vec<Running>,
    active: Option<WindowId>,
    started: Instant,
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
    error: Option<String>,
    window_store: WindowStore,
    state_dirty: bool,
    next_state_save: Instant,
    /// The last answer the system gave about reduced motion, read when the window
    /// opens and when it gains focus, and kept when a later read gives none. Every
    /// tab, old or new, takes this one value.
    reduce_motion: std::cell::Cell<bool>,
    /// The last answer the system gave about more contrast, kept like the one
    /// about motion: read when a window opens and when it gains focus.
    more_contrast: std::cell::Cell<bool>,
    /// The next time the small settings file is read for a change.
    next_settings_check: Instant,
    /// The opener started for the settings file, while it runs: it sends a
    /// message if it fails, for the notice.
    settings_opening: Option<std::sync::mpsc::Receiver<String>>,
    #[cfg(target_os = "macos")]
    frame_wake_handled: std::sync::mpsc::Sender<()>,
    #[cfg(target_os = "macos")]
    menu: Option<crate::menu::MenuBar>,
}

impl Host {
    /// The modifier keys held now, as a mouse report counts them.
    fn held(run: &Running) -> Held {
        Held {
            shift: run.modifiers.shift_key(),
            alt: run.modifiers.alt_key(),
            ctrl: run.modifiers.control_key(),
        }
    }

    fn index(&self, id: WindowId) -> Option<usize> {
        self.windows.iter().position(|run| run.window.id() == id)
    }

    fn active_id(&self) -> Option<WindowId> {
        self.active
            .or_else(|| self.windows.last().map(|run| run.window.id()))
    }

    /// Show the terminal's own notice in the focused pane of window `target`,
    /// or of the active window, and on standard error.
    fn notice(&mut self, target: Option<WindowId>, text: &str) {
        eprintln!("{}: {text}", crate::APP_NAME);
        let Some(index) = target
            .or_else(|| self.active_id())
            .and_then(|id| self.index(id))
        else {
            return;
        };
        let run = &mut self.windows[index];
        if let Some(tab) = run.tabs.active_mut() {
            tab.app_mut().show_notice(text);
            run.window.request_redraw();
        }
    }

    /// Open the settings file, and say so in the window when it cannot be.
    fn open_settings(&mut self, target: Option<WindowId>) {
        match crate::settings_command::open() {
            Ok(opening) => self.settings_opening = opening,
            Err(why) => self.notice(target, &format!("Settings not opened: {why}")),
        }
    }

    /// Show a notice if the settings file's opener has failed since the last
    /// look; forget it once it has ended either way.
    fn check_settings_opening(&mut self) {
        let Some(opening) = &self.settings_opening else {
            return;
        };
        match opening.try_recv() {
            Ok(why) => {
                self.settings_opening = None;
                self.notice(None, &format!("Settings not opened: {why}"));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.settings_opening = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Record the zoom each window shows now, its active tab's, and mark the
    /// state to be saved when one changed: a zoom, and a new, switched or
    /// closed tab, can each change it.
    fn track_zoom(&mut self) {
        for run in &mut self.windows {
            let shown = run.tabs.active().map(|tab| tab.app().zoom_level());
            if run.placement.shown(shown) {
                self.state_dirty = true;
            }
        }
    }

    fn placements(&self) -> Vec<Placement> {
        self.windows.iter().map(|run| run.placement).collect()
    }

    fn save_window_state(&mut self) {
        self.track_zoom();
        let placements = self.placements();
        self.write_window_state(&placements);
    }

    fn write_window_state(&mut self, placements: &[Placement]) {
        if let Err(e) = self.window_store.save(placements) {
            eprintln!("{}: window state not saved ({e})", crate::APP_NAME);
        }
        self.state_dirty = false;
        self.next_state_save = Instant::now() + Duration::from_millis(250);
    }

    fn refresh_more_contrast(&mut self) {
        let Some(more) = crate::contrast::system_increases_contrast() else {
            return;
        };
        self.more_contrast.set(more);
        for run in &mut self.windows {
            let redraw = run.tabs.iter_mut().fold(false, |changed, tab| {
                tab.apps_mut()
                    .into_iter()
                    .fold(changed, |c, app| app.set_more_contrast(more) | c)
            });
            if redraw {
                run.window.request_redraw();
            }
        }
    }

    fn refresh_reduce_motion(&mut self) {
        let Some(reduce) = crate::motion::system_reduces_motion() else {
            return;
        };
        self.reduce_motion.set(reduce);
        for run in &mut self.windows {
            let redraw = run.tabs.iter_mut().fold(false, |changed, tab| {
                tab.apps_mut()
                    .into_iter()
                    .fold(changed, |c, app| app.set_reduce_motion(reduce) | c)
            });
            if redraw {
                run.window.request_redraw();
            }
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, message: String) {
        self.error = Some(message);
        event_loop.exit();
    }

    fn start_app(&self, window: &Window) -> Result<App, String> {
        let raw = self.config.settings.current().look();
        let look = raw.resolve();
        let size = window.inner_size();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a scale factor is a small positive number"
        )]
        let scale = self
            .config
            .scale
            .unwrap_or_else(|| window.scale_factor().round().max(1.0) as u32);
        let (program, arguments) = match self.config.command.split_first() {
            Some((p, a)) => (p.clone(), a.to_vec()),
            None => (
                crate::shell::default_shell(look.shell.as_deref()),
                Vec::new(),
            ),
        };
        let mut app = App::start_with_shell(
            (self.config.layer)(),
            &program,
            &arguments,
            scale,
            size.width,
            size.height,
            crate::shellint::ShellOptions {
                integration: look.shell_integration,
            },
        )
        .map_err(|e| e.to_string())?;
        if self.config.scale.is_some() {
            app.pin_scale();
        }
        app.set_config_notices(crate::settings::notices(
            self.config.policy_notice.as_deref(),
            self.config.settings.current(),
            &look,
        ));
        app.use_settings(self.config.notifications);
        app.use_images(self.config.images);
        app.set_theme_choice(look.theme);
        app.set_copy_on_select(raw.copy_on_select);
        app.set_smooth_scroll(raw.smooth_scroll);
        // The window reads the system's answer when it opens and when it gains
        // focus; a new tab only takes what the window knows.
        app.set_reduce_motion(self.reduce_motion.get());
        app.set_more_contrast(self.more_contrast.get());
        app.set_appearance(appearance(window.theme()));
        if let Err(e) = app.set_fonts(&look.fonts) {
            eprintln!("{}: fonts not used ({e})", crate::APP_NAME);
        }
        let content_height = size.height.saturating_sub(app.cell_height());
        app.resize(size.width, content_height)
            .map_err(|e| e.to_string())?;
        Ok(app)
    }

    fn reload_settings(&mut self) {
        let Some(settings) = self.config.settings.take_change() else {
            return;
        };
        let raw = settings.look();
        let look = raw.resolve();
        for notice in settings.notice().iter().chain(&look.notices) {
            eprintln!("{}: {notice}", crate::APP_NAME);
        }
        let shown = crate::settings::notices(self.config.policy_notice.as_deref(), settings, &look);
        for run in &mut self.windows {
            let mut redraw = false;
            let mut resized = false;
            for tab in run.tabs.iter_mut() {
                for app in tab.apps_mut() {
                    redraw |= app.set_theme_choice(look.theme);
                    match app.set_fonts(&look.fonts) {
                        Ok(changed) => resized |= changed,
                        Err(e) => eprintln!("{}: fonts not used ({e})", crate::APP_NAME),
                    }
                    app.set_copy_on_select(raw.copy_on_select);
                    app.set_smooth_scroll(raw.smooth_scroll);
                    redraw |= app.set_config_notices(shown.clone());
                }
            }
            if resized {
                redraw |= Self::resize_tabs(run, run.window.inner_size());
            }
            if redraw {
                run.window.request_redraw();
            }
            if resized {
                Self::refocus(run);
            }
        }
    }

    fn update_window_title(run: &mut Running) {
        let Some(tab) = run.tabs.active() else { return };
        let title = if run.tabs.len() > 1 {
            format!(
                "{} [{}/{}]",
                tab.title,
                run.tabs.active_index() + 1,
                run.tabs.len()
            )
        } else {
            tab.title.clone()
        };
        if title != run.shown_title {
            run.window.set_title(&title);
            run.shown_title = title;
        }
    }

    /// Tell every pane whether it has the focus: only the focused pane of the
    /// active tab does, and only while the window has it.
    fn set_focus_states(run: &mut Running) {
        let active = run.tabs.active_index();
        let window_focused = run.focused;
        for (index, tab) in run.tabs.iter_mut().enumerate() {
            let here = tab.panes.focus_index();
            for (pane, app) in tab.apps_mut().into_iter().enumerate() {
                let focused = index == active && pane == here;
                app.set_focused(window_focused && focused);
                if !focused || !window_focused {
                    app.clear_preedit();
                }
            }
        }
    }

    fn follow_ime(run: &mut Running) {
        let size = run.window.inner_size();
        let Some(tab) = run.tabs.active() else { return };
        let (left, top) = tab.origin(size);
        let mut sink = WindowSink {
            window: &run.window,
            left,
            top,
        };
        run.ime.follow(tab.app(), &mut sink);
    }

    fn refocus(run: &mut Running) {
        // Another tab or pane set: a divider held until now may not be there.
        run.divider_drag = None;
        Self::set_focus_states(run);
        run.ime.reset();
        Self::follow_ime(run);
        Self::update_window_title(run);
        run.window.request_redraw();
    }

    fn select_tab(&mut self, id: WindowId, index: usize) {
        if let Some(run) = self.index(id).map(|index| &mut self.windows[index])
            && run.tabs.select(index)
        {
            Self::refocus(run);
        }
    }

    fn cycle_tab(&mut self, id: WindowId, next: bool) {
        if let Some(run) = self.index(id).map(|index| &mut self.windows[index]) {
            let changed = if next {
                run.tabs.next()
            } else {
                run.tabs.previous()
            };
            if changed {
                Self::refocus(run);
            }
        }
    }

    fn new_tab(&mut self, id: WindowId) {
        let Some(window) = self
            .index(id)
            .map(|index| self.windows[index].window.clone())
        else {
            return;
        };
        let app = match self.start_app(&window) {
            Ok(app) => app,
            Err(e) => {
                eprintln!("{}: new tab not opened ({e})", crate::APP_NAME);
                return;
            }
        };
        if let Some(index) = self.index(id) {
            let run = &mut self.windows[index];
            run.tabs
                .push(Tab::new(app, crate::title::window_title(None)));
            let size = run.window.inner_size();
            if let Some(tab) = run.tabs.active_mut() {
                tab.resize(size);
            }
            Self::refocus(run);
        }
    }

    fn split(&mut self, id: WindowId, how: Split) {
        let Some(index) = self.index(id) else { return };
        let window = self.windows[index].window.clone();
        let size = window.inner_size();
        let allowed = self.windows[index].tabs.active().is_some_and(|tab| {
            let app = tab.app();
            let area = (
                size.width,
                size.height.saturating_sub(tab.strip()),
                tab.gap(),
            );
            tab.panes
                .can_split(how, area, |w, h| app.pane_has(w, h, MIN_PANE.0, MIN_PANE.1))
        });
        if !allowed {
            return;
        }
        let mut app = match self.start_app(&window) {
            Ok(app) => app,
            Err(e) => {
                eprintln!("{}: pane not opened ({e})", crate::APP_NAME);
                return;
            }
        };
        let run = &mut self.windows[index];
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        // The panes of a tab are drawn at one size.
        let scale = tab.app().scale();
        if app.scale() != scale {
            let _ = app.set_scale(scale);
        }
        tab.panes.split(app, how);
        tab.resize(size);
        Self::refocus(run);
    }

    fn cycle_pane(&mut self, id: WindowId, next: bool) {
        if let Some(run) = self.index(id).map(|index| &mut self.windows[index]) {
            let changed = run.tabs.active_mut().is_some_and(|tab| {
                if next {
                    tab.panes.focus_next()
                } else {
                    tab.panes.focus_previous()
                }
            });
            if changed {
                Self::refocus(run);
            }
        }
    }

    fn close_active(&mut self, event_loop: &ActiveEventLoop, id: WindowId) {
        // The zoom shown until now, before the tab that shows it goes.
        self.track_zoom();
        let empty = self.index(id).is_none_or(|index| {
            let run = &mut self.windows[index];
            let size = run.window.inner_size();
            if let Some(tab) = run.tabs.active_mut()
                && tab.panes.len() > 1
            {
                // Closing a pane gives its space to the pane beside it.
                let at = tab.panes.focus_index();
                tab.panes.remove(at);
                tab.resize(size);
                return false;
            }
            run.tabs.remove_active();
            run.tabs.is_empty()
        });
        if empty {
            self.close_window(event_loop, id);
        } else if let Some(index) = self.index(id) {
            Self::refocus(&mut self.windows[index]);
        }
    }

    fn close_window(&mut self, event_loop: &ActiveEventLoop, id: WindowId) {
        let Some(index) = self.index(id) else { return };
        // The last window to close is still the one to restore, as it was last
        // seen: its placement is taken before it goes.
        self.track_zoom();
        let kept = crate::window_state::after_close(&self.placements(), index);
        self.windows.remove(index);
        if self.active == Some(id) {
            self.active = self
                .windows
                .iter()
                .find(|run| run.focused)
                .or_else(|| self.windows.last())
                .map(|run| run.window.id());
        }
        self.write_window_state(&kept);
        if self.windows.is_empty() {
            event_loop.exit();
        }
    }

    fn resize_tabs(run: &mut Running, size: PhysicalSize<u32>) -> bool {
        let mut changed = false;
        for tab in run.tabs.iter_mut() {
            changed |= tab.resize(size);
        }
        changed
    }

    fn redraw(&mut self, id: WindowId) {
        let Some(index) = self.index(id) else { return };
        let run = &mut self.windows[index];
        let size = run.window.inner_size();
        let (Some(w), Some(h)) = (
            std::num::NonZeroU32::new(size.width),
            std::num::NonZeroU32::new(size.height),
        ) else {
            return;
        };
        if run.surface.resize(w, h).is_err() {
            return;
        }
        let titles: Vec<String> = run.tabs.iter().map(|tab| tab.title.clone()).collect();
        let Some(tab) = run.tabs.active() else { return };
        let view = if tab.panes.len() == 1 {
            tab.app()
                .view_with_tabs(&titles, run.tabs.active_index(), size.height)
        } else {
            let layout = tab.layout(size);
            let views: Vec<Frame> = tab.panes.all().into_iter().map(App::view).collect();
            let content = tab.app().compose_panes(
                (size.width, size.height.saturating_sub(tab.strip())),
                &layout,
                &views,
                tab.panes.focus_index(),
            );
            tab.app()
                .tabs_over(&content, &titles, run.tabs.active_index(), size.height)
        };
        let Ok(mut buffer) = run.surface.buffer_mut() else {
            return;
        };
        let words = to_xrgb(&view);
        let n = buffer.len().min(words.len());
        buffer[..n].copy_from_slice(&words[..n]);
        let _ = buffer.present();
    }
}

fn chord(state: ModifiersState) -> actions::Held {
    actions::Held {
        command: state.super_key(),
        control: state.control_key(),
        alt: state.alt_key(),
        shift: state.shift_key(),
    }
}

impl Host {
    /// What the reader should be told about this window now.
    fn access_snapshot(run: &Running) -> Option<crate::access::Snapshot> {
        let tab = run.tabs.active()?;
        let app = tab.app();
        let view_links = app.view_links();
        let focused_link = view_links.iter().position(|l| l.focused);
        let links = view_links
            .iter()
            .map(|l| crate::access::Link {
                target: l.target().to_owned(),
                text: l.text().to_owned(),
            })
            .collect();
        Some(crate::access::Snapshot {
            title: run.shown_title.clone(),
            rows: app.visible_text(),
            links,
            focused_link,
            question: app
                .link_prompt()
                .map(|p| crate::access::Question { lines: p.lines() }),
            tabs: run.tabs.iter().map(|t| t.title.clone()).collect(),
            active_tab: run.tabs.active_index(),
            pane: (tab.panes.len() > 1).then(|| (tab.panes.focus_index() + 1, tab.panes.len())),
        })
    }

    /// Send the reader what changed since it was last told, if one is connected.
    fn tell_reader(run: &mut Running) {
        if !run.access_on {
            return;
        }
        let Some(now) = Self::access_snapshot(run) else {
            return;
        };
        if run.access_told.as_ref() == Some(&now) {
            return;
        }
        run.access.update_if_active(|| crate::access::build(&now));
        run.access_told = Some(now);
    }

    /// An assistive technology asked something.
    fn on_access(&mut self, event: accesskit_winit::Event) {
        use accesskit_winit::WindowEvent as Ask;
        let Some(index) = self.index(event.window_id) else {
            return;
        };
        let run = &mut self.windows[index];
        match event.window_event {
            Ask::InitialTreeRequested => {
                run.access_on = true;
                run.access_told = None;
                Self::tell_reader(run);
            }
            Ask::AccessibilityDeactivated => {
                run.access_on = false;
                run.access_told = None;
            }
            Ask::ActionRequested(request) => {
                use crate::access::{Target, target_of};
                use accesskit::Action;
                let target = target_of(request.target_node);
                let index_now = run.tabs.active_index();
                let Some(tab) = run.tabs.active_mut() else {
                    return;
                };
                match (request.action, target) {
                    (Action::Click, Target::Link(n)) => {
                        tab.app_mut().ask_about_view_link(n);
                    }
                    (Action::Focus, Target::Link(n)) => {
                        tab.app_mut().focus_view_link(n);
                    }
                    (Action::Click, Target::Open) => {
                        let _ = tab.app_mut().answer_link(true);
                    }
                    (Action::Click, Target::Cancel) => {
                        let _ = tab.app_mut().answer_link(false);
                    }
                    (Action::Click, Target::Tab(n)) if n != index_now => {
                        let id = run.window.id();
                        self.select_tab(id, n);
                        return;
                    }
                    _ => {}
                }
                run.window.request_redraw();
            }
        }
    }

    /// The wheel or trackpad moved: a program that asked for wheel reports gets
    /// them, and otherwise the output scrolls, by whole rows for a wheel and by
    /// the pixels moved for a trackpad.
    fn wheel(&mut self, id: WindowId, delta: &MouseScrollDelta) {
        let Some(index) = self.index(id) else { return };
        let run = &mut self.windows[index];
        let held = Self::held(run);
        let (x, y) = run.pointer;
        let size = run.window.inner_size();
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        let top = tab.strip();
        // The wheel goes to the pane under the pointer, not the focused one.
        let Some((pane, x, y)) = tab.pane_at(size, x, y) else {
            return;
        };
        let Some(app) = tab.panes.get_mut(pane) else {
            return;
        };
        let line = f64::from(top);
        let redraw = if let MouseScrollDelta::PixelDelta(p) = delta {
            app.wheel_pixels(x, y, wheel_pixels(p.y), held)
                .unwrap_or(false)
        } else if let Some((step, n)) = wheel_steps(delta, line) {
            (0..n).fold(false, |moved, _| {
                app.mouse_wheel_glide(x, y, step, held).unwrap_or(false) | moved
            })
        } else {
            false
        };
        if redraw {
            run.window.request_redraw();
        }
    }

    /// Do one of the window's own actions, from a menu item or a shortcut.
    fn act(&mut self, event_loop: &ActiveEventLoop, target: Option<WindowId>, action: Action) {
        match action {
            Action::NewWindow => {
                if let Err(e) = self.open_window(event_loop, None) {
                    eprintln!("{}: new window not opened ({e})", crate::APP_NAME);
                }
                return;
            }
            Action::Settings => {
                self.open_settings(target);
                return;
            }
            Action::Quit => {
                self.save_window_state();
                event_loop.exit();
                return;
            }
            _ => {}
        }
        let Some(id) = target.or_else(|| self.active_id()) else {
            return;
        };
        match action {
            Action::NewTab => return self.new_tab(id),
            Action::NextTab => return self.cycle_tab(id, true),
            Action::PreviousTab => return self.cycle_tab(id, false),
            Action::Close => return self.close_active(event_loop, id),
            Action::SplitRight => return self.split(id, Split::Right),
            Action::SplitDown => return self.split(id, Split::Down),
            Action::NextPane => return self.cycle_pane(id, true),
            Action::PreviousPane => return self.cycle_pane(id, false),
            Action::Quit => {
                return;
            }
            Action::NewWindow | Action::Settings => return,
            _ => {}
        }
        let Some(index) = self.index(id) else { return };
        let run = &mut self.windows[index];
        let size = run.window.inner_size();
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        let zoom = matches!(action, Action::ZoomIn | Action::ZoomOut | Action::ZoomReset);
        if zoom {
            // The panes of a tab are drawn at one size: the others zoom with it.
            let here = tab.panes.focus_index();
            for (pane, app) in tab.apps_mut().into_iter().enumerate() {
                if pane != here {
                    let _ = app.perform(action, &SystemClipboard);
                }
            }
        }
        let (left, top) = tab.origin(size);
        let outcome = perform_action(
            tab.app_mut(),
            action,
            &SystemClipboard,
            &mut run.ime,
            &mut WindowSink {
                window: &run.window,
                left,
                top,
            },
        );
        if zoom {
            tab.resize(size);
            self.state_dirty = true;
        }
        // An open context menu was closed by the action: draw without it.
        let menu_closed = tab.app_mut().take_view_changed();
        match outcome {
            Outcome::Exit => event_loop.exit(),
            Outcome::Redraw => run.window.request_redraw(),
            Outcome::Nothing | Outcome::Refused if menu_closed => run.window.request_redraw(),
            Outcome::Nothing | Outcome::Refused => {}
        }
    }
}

impl Host {
    /// Handle a key press: the input method owns keys while it composes, then
    /// shortcuts, then the key is sent to the program.
    fn on_key(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: &KeyEvent) {
        let Some(index) = self.index(id) else { return };
        let modifiers = self.windows[index].modifiers;
        if self.windows[index]
            .tabs
            .active()
            .is_some_and(|tab| tab.app().preedit().is_some())
        {
            // The input method owns the keys while it composes.
            return;
        }
        if let WinitKey::Character(c) = &event.logical_key
            && let Some(ch) = c.chars().next()
            && let Some(action) = actions::action_for(Platform::current(), ch, chord(modifiers))
        {
            self.act(event_loop, Some(id), action);
            return;
        }
        let mut choice = None;
        let run = &mut self.windows[index];
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        if let Some((key, mods)) = map_key_at(
            &event.logical_key,
            event.text.as_deref(),
            modifiers,
            event.location,
        ) {
            if send_input(tab.app_mut(), Input::Key(key, mods)) {
                run.window.request_redraw();
            }
            choice = tab.app_mut().take_menu_choice();
        } else if let Some(text) = event.text.as_deref()
            && !modifiers.super_key()
            && text.chars().count() > 1
            && send_input(tab.app_mut(), Input::Text(text))
        {
            run.window.request_redraw();
        }
        if let Some(action) = choice {
            self.act(event_loop, Some(id), action);
        }
    }
}

impl Host {
    fn on_scale(&mut self, id: WindowId, scale_factor: f64) {
        let Some(index) = self.index(id) else { return };
        let run = &mut self.windows[index];
        let size = run.window.inner_size();
        let mut changed = false;
        for tab in run.tabs.iter_mut() {
            for app in tab.apps_mut() {
                changed |= app.set_display_scale(scale_factor).unwrap_or(false);
            }
            changed |= tab.resize(size);
        }
        run.ime.reset();
        Self::follow_ime(run);
        if changed {
            run.window.request_redraw();
        }
    }

    fn on_ime(&mut self, id: WindowId, event: &Ime) {
        let Some(index) = self.index(id) else { return };
        let run = &mut self.windows[index];
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        let redraw = ime_event(tab.app_mut(), event);
        Self::follow_ime(run);
        if redraw {
            run.window.request_redraw();
        }
    }

    fn on_focus(&mut self, id: WindowId, focused: bool) {
        // The system setting can change while another window has the focus.
        if focused {
            self.refresh_reduce_motion();
            self.refresh_more_contrast();
        }
        let reduce = self.reduce_motion.get();
        let Some(index) = self.index(id) else { return };
        let run = &mut self.windows[index];
        run.focused = focused;
        for tab in run.tabs.iter_mut() {
            for app in tab.apps_mut() {
                app.set_reduce_motion(reduce);
            }
        }
        Self::set_focus_states(run);
        run.ime.reset();
        Self::follow_ime(run);
        if focused {
            self.active = Some(id);
        }
        run.window.request_redraw();
    }

    fn on_cursor(&mut self, id: WindowId, position: PhysicalPosition<f64>) {
        let Some(index) = self.index(id) else { return };
        let run = &mut self.windows[index];
        run.pointer = pointer_pixels(position.x, position.y);
        let held = Self::held(run);
        let captured = run.mouse_capture.is_some();
        let (x, y) = run.pointer;
        let size = run.window.inner_size();
        let held_in = run.capture_pane;
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        if let Some(divider) = run.divider_drag {
            // A live resize: the panes follow the pointer while it is held.
            if tab.drag_divider(size, divider, x, y) {
                run.window.request_redraw();
            }
            return;
        }
        let hover = if captured {
            None
        } else {
            tab.divider_at(size, x, y).map(|(_, split)| split)
        };
        if hover != run.divider_hover {
            run.divider_hover = hover;
            run.window.set_cursor(match hover {
                Some(Split::Right) => CursorIcon::ColResize,
                Some(Split::Down) => CursorIcon::RowResize,
                None => CursorIcon::Default,
            });
        }
        let target = if captured {
            tab.pane_clamped(size, held_in, x, y)
                .map(|(x, y)| (held_in, x, y))
        } else {
            tab.pane_at(size, x, y)
        };
        let Some((pane, x, y)) = target else { return };
        let Some(app) = tab.panes.get_mut(pane) else {
            return;
        };
        let redraw = app.mouse_move(x, y, held).unwrap_or(false) | app.take_selection_redraw();
        if redraw {
            run.window.request_redraw();
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one press, move or release: the pane it is in, the menus, then the program"
    )]
    fn on_mouse(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        state: ElementState,
        button: MouseButton,
    ) {
        let Some(index) = self.index(id) else { return };
        let held = Self::held(&self.windows[index]);
        let (x, y) = self.windows[index].pointer;
        if state == ElementState::Pressed && button == MouseButton::Left {
            let selected = self.windows.get(index).and_then(|run| {
                let top = run.tabs.active()?.strip();
                (y < top)
                    .then(|| run.tabs.hit(x, run.window.inner_size().width))
                    .flatten()
            });
            if let Some(tab_index) = selected {
                self.select_tab(id, tab_index);
                return;
            }
        }
        let size = self.windows[index].window.inner_size();
        let captured = self.windows[index].mouse_capture == Some(button);
        if button == MouseButton::Left {
            let run = &mut self.windows[index];
            if state == ElementState::Released && run.divider_drag.take().is_some() {
                return;
            }
            // A press on a divider takes hold of it: it neither focuses a pane
            // nor reaches a program.
            if state == ElementState::Pressed
                && run.mouse_capture.is_none()
                && let Some((divider, _)) =
                    run.tabs.active().and_then(|tab| tab.divider_at(size, x, y))
            {
                run.divider_drag = Some(divider);
                return;
            }
        }
        if state == ElementState::Pressed
            && !captured
            && let Some(tab) = self.windows[index].tabs.active_mut()
            && let Some((pane, _, _)) = tab.pane_at(size, x, y)
            && tab.panes.focus(pane)
        {
            // A press in another pane gives it the focus first.
            Self::refocus(&mut self.windows[index]);
        }
        let run = &mut self.windows[index];
        let held_in = run.capture_pane;
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        let target = if captured {
            tab.pane_clamped(size, held_in, x, y)
                .map(|(x, y)| (held_in, x, y))
        } else {
            tab.pane_at(size, x, y)
        };
        let Some((pane, x, y)) = target else {
            return;
        };
        let Some(app) = tab.panes.get_mut(pane) else {
            return;
        };
        #[cfg(any(target_os = "macos", windows))]
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && !app.is_selecting()
            && app.owns_context_click(x, y, held)
        {
            let menu_state = app.context_state(x, y, SystemClipboard.text().is_some());
            if let Ok(menu) = crate::menu::context_menu(menu_state) {
                crate::ffi_context_menu::show(&menu, &*run.window);
            }
            return;
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && !app.is_selecting()
            && app.owns_context_click(x, y, held)
        {
            // There is no system menu here: the terminal draws its own.
            if app.open_context_menu(x, y, SystemClipboard.text().is_some()) {
                run.window.request_redraw();
            }
            return;
        }
        let mut redraw = false;
        let mut taken = false;
        if state == ElementState::Pressed && button == MouseButton::Left {
            redraw = app.click(x, y);
            let link_modifier = if cfg!(target_os = "macos") {
                run.modifiers.super_key()
            } else {
                run.modifiers.control_key()
            };
            if app.link_prompt().is_some() || (link_modifier && app.context_menu().is_none()) {
                taken = app.link_press(x, y);
                redraw |= taken;
            }
        }
        if !taken && let Some(b) = map_button(button) {
            redraw |= match state {
                ElementState::Pressed => app.mouse_press(x, y, b, held),
                ElementState::Released => app.mouse_release(x, y, b, held),
            }
            .unwrap_or(false);
            match state {
                ElementState::Pressed => {
                    run.mouse_capture = Some(button);
                    run.capture_pane = pane;
                }
                ElementState::Released if captured => run.mouse_capture = None,
                ElementState::Released => {}
            }
        }
        redraw |= app.take_selection_redraw();
        let copy = app.take_copy_on_select();
        let choice = app.take_menu_choice();
        if redraw {
            run.window.request_redraw();
        }
        if copy {
            self.act(event_loop, Some(id), Action::Copy);
        }
        if let Some(action) = choice {
            self.act(event_loop, Some(id), action);
        }
    }

    fn on_theme(&mut self, id: WindowId, theme: WinitTheme) {
        let Some(index) = self.index(id) else { return };
        let run = &mut self.windows[index];
        let changed = run.tabs.iter_mut().fold(false, |changed, tab| {
            tab.apps_mut().into_iter().fold(changed, |c, app| {
                app.set_appearance(appearance(Some(theme))) | c
            })
        });
        if changed {
            run.window.request_redraw();
        }
    }

    /// Handle one window event.
    fn handle_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        if let Some(index) = self.index(id) {
            let run = &mut self.windows[index];
            run.access.process_event(&run.window, &event);
        }
        match event {
            WindowEvent::CloseRequested => self.close_window(event_loop, id),
            WindowEvent::ModifiersChanged(m) => {
                if let Some(index) = self.index(id) {
                    self.windows[index].modifiers = m.state();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(index) = self.index(id) {
                    let run = &mut self.windows[index];
                    run.placement.size = (size.width, size.height);
                    self.state_dirty = true;
                    if Self::resize_tabs(run, size) {
                        run.window.request_redraw();
                    }
                }
            }
            WindowEvent::Moved(position) => {
                if let Some(index) = self.index(id) {
                    self.windows[index].placement.position = Some((position.x, position.y));
                    self.state_dirty = true;
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.on_scale(id, scale_factor),
            WindowEvent::Ime(event) => self.on_ime(id, &event),
            WindowEvent::Focused(focused) => self.on_focus(id, focused),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.on_key(event_loop, id, &event);
            }
            WindowEvent::CursorMoved { position, .. } => self.on_cursor(id, position),
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse(event_loop, id, state, button);
            }
            WindowEvent::MouseWheel { delta, .. } => self.wheel(id, &delta),
            WindowEvent::ThemeChanged(theme) => self.on_theme(id, theme),
            WindowEvent::RedrawRequested => self.redraw(id),
            _ => {}
        }
    }

    fn open_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        placement: Option<Placement>,
    ) -> Result<WindowId, String> {
        // Hidden until the accessibility adapter exists: it must be made before
        // the window is first shown.
        let mut attributes = Window::default_attributes()
            .with_title(crate::title::window_title(None))
            .with_visible(false);
        if let Some(placement) = placement {
            attributes =
                attributes.with_inner_size(PhysicalSize::new(placement.size.0, placement.size.1));
            if let Some((x, y)) = placement.position {
                attributes = attributes.with_position(PhysicalPosition::new(x, y));
            }
        } else {
            attributes = attributes.with_inner_size(winit::dpi::LogicalSize::new(
                f64::from(self.config.size.0),
                f64::from(self.config.size.1),
            ));
        }
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("no window: {e}"))?,
        );
        let access = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        window.set_visible(true);
        let context = softbuffer::Context::new(window.clone())
            .map_err(|e| format!("no display context: {e}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|e| format!("no surface: {e}"))?;
        window.set_ime_allowed(true);
        self.refresh_reduce_motion();
        self.refresh_more_contrast();
        let mut app = self.start_app(&window)?;
        // A restored window comes back at its own zoom; the others are not
        // touched, and Zoom Reset still goes back to the display's scale.
        let zoom = placement.map_or(1, |p| p.zoom);
        if zoom > 1
            && let Err(e) = app.set_zoom_level(zoom)
        {
            eprintln!("{}: zoom not restored ({e})", crate::APP_NAME);
        }
        let size = window.inner_size();
        let actual = Placement {
            position: window.outer_position().ok().map(|p| (p.x, p.y)),
            size: (size.width, size.height),
            zoom: app.zoom_level(),
        };
        let id = window.id();
        window.request_redraw();
        let mut tabs = Tabs::new(Tab::new(app, crate::title::window_title(None)));
        if zoom > 1
            && let Some(tab) = tabs.active_mut()
        {
            // Laid out again at the zoomed cell size.
            tab.resize(size);
        }
        self.windows.push(Running {
            window,
            surface,
            tabs,
            shown_title: crate::title::window_title(None),
            modifiers: ModifiersState::default(),
            pointer: (0, 0),
            ime: ImeFollower::default(),
            focused: false,
            mouse_capture: None,
            capture_pane: 0,
            divider_drag: None,
            divider_hover: None,
            placement: actual,
            access,
            access_on: false,
            access_told: None,
        });
        self.active = Some(id);
        self.state_dirty = true;
        Ok(id)
    }
}

impl ApplicationHandler<UserEvent> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        if let Some(notice) = self.window_store.notice() {
            eprintln!("{}: {notice}", crate::APP_NAME);
        }
        #[cfg(target_os = "macos")]
        if self.menu.is_none() {
            // A menu that cannot be built leaves the shortcuts working.
            self.menu = crate::menu::MenuBar::install().ok();
        }
        let restored = self.window_store.take_restored();
        let placements: Vec<Option<Placement>> = if restored.is_empty() {
            vec![None]
        } else {
            restored.into_iter().map(Some).collect()
        };
        for placement in placements {
            if let Err(e) = self.open_window(event_loop, placement) {
                if self.windows.is_empty() {
                    self.fail(event_loop, e);
                    return;
                }
                eprintln!("{}: restored window not opened ({e})", crate::APP_NAME);
            }
        }
        if self.windows.is_empty() {
            self.fail(event_loop, "no window restored".to_owned());
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.handle_window_event(event_loop, id, event);
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: UserEvent) {
        match event {
            #[cfg(target_os = "macos")]
            UserEvent::Wake => {
                let _ = self.frame_wake_handled.send(());
            }
            UserEvent::Access(event) => self.on_access(event),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(limit) = self.config.exit_after
            && self.started.elapsed() >= limit
        {
            self.save_window_state();
            event_loop.exit();
            return;
        }
        // A tab opened, switched to or closed since the last pass may show
        // another zoom; record it before a program's end can close its tab.
        self.track_zoom();
        let now = Instant::now();
        if self.state_dirty && now >= self.next_state_save {
            self.save_window_state();
        }
        if now >= self.next_settings_check {
            self.next_settings_check = now + Duration::from_millis(250);
            self.reload_settings();
            self.check_settings_opening();
        }
        #[cfg(any(target_os = "macos", windows))]
        for action in crate::menu::chosen() {
            self.act(event_loop, None, action);
        }
        let mut close = Vec::new();
        for run in &mut self.windows {
            let active = run.tabs.active_index();
            let mut redraw = false;
            let mut exited = Vec::new();
            let mut refocus = false;
            let mut request_attention = false;
            let mut cancel_attention = false;
            let mut closed_pane = false;
            let size = run.window.inner_size();
            for (index, tab) in run.tabs.iter_mut().enumerate() {
                let mut gone = Vec::new();
                for (pane, app) in tab.panes.all_mut().into_iter().enumerate() {
                    if pump_frame(app) {
                        redraw |= index == active;
                    }
                    if let Some(title) = app.take_window_title() {
                        tab.title = title;
                        redraw = true;
                    }
                    match app.attention_step() {
                        crate::attention::Step::Request => request_attention = true,
                        crate::attention::Step::Cancel => cancel_attention = true,
                        crate::attention::Step::Idle => {}
                    }
                    if app.has_exited() {
                        gone.push(pane);
                    }
                }
                if gone.len() == tab.panes.len() {
                    exited.push(index);
                } else {
                    // A program that ended closes its own pane: the neighbour
                    // takes the space.
                    for pane in gone.into_iter().rev() {
                        tab.panes.remove(pane);
                        closed_pane = true;
                    }
                    if closed_pane {
                        tab.resize(size);
                        redraw |= index == active;
                    }
                }
            }
            refocus |= closed_pane;
            if request_attention {
                run.window
                    .request_user_attention(Some(winit::window::UserAttentionType::Informational));
            } else if cancel_attention {
                run.window.request_user_attention(None);
            }
            for index in exited.into_iter().rev() {
                run.tabs.remove(index);
                refocus = true;
            }
            if run.tabs.is_empty() {
                close.push(run.window.id());
                continue;
            }
            if redraw || refocus {
                run.window.request_redraw();
            }
            if refocus {
                Self::refocus(run);
            }
            Self::update_window_title(run);
        }
        for run in &mut self.windows {
            Self::tell_reader(run);
        }
        for id in close {
            self.close_window(event_loop, id);
        }
        #[cfg(not(target_os = "macos"))]
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME_INTERVAL));
    }
}

/// The appearance for the system's window theme; light when it does not say.
#[must_use]
pub fn appearance(theme: Option<WinitTheme>) -> Appearance {
    match theme {
        Some(WinitTheme::Dark) => Appearance::Dark,
        Some(WinitTheme::Light) | None => Appearance::Light,
    }
}

/// Open the window and run until it closes or the program exits.
///
/// # Errors
///
/// Returns why the event loop, the window or the program could not start.
pub fn run(config: Config) -> Result<(), String> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|e| format!("no event loop: {e}"))?;
    let proxy = event_loop.create_proxy();
    #[cfg(target_os = "macos")]
    let frame_wake_handled = {
        let proxy = proxy.clone();
        start_frame_wakes(move || proxy.send_event(UserEvent::Wake).is_ok())
            .map_err(|e| format!("no window wake thread: {e}"))?
    };
    let window_store = WindowStore::system();
    let mut host = Host {
        config,
        windows: Vec::new(),
        active: None,
        started: Instant::now(),
        proxy,
        error: None,
        window_store,
        state_dirty: false,
        next_state_save: Instant::now(),
        reduce_motion: std::cell::Cell::new(false),
        more_contrast: std::cell::Cell::new(false),
        next_settings_check: Instant::now(),
        settings_opening: None,
        #[cfg(target_os = "macos")]
        frame_wake_handled,
        #[cfg(target_os = "macos")]
        menu: None,
    };
    event_loop
        .run_app(&mut host)
        .map_err(|e| format!("event loop: {e}"))?;
    host.error.map_or(Ok(()), Err)
}

#[cfg(all(test, unix))]
mod pane_tests {
    use super::{Split, Tab};
    use crate::{App, Passthrough};
    use winit::dpi::PhysicalSize;

    fn app() -> App {
        App::start(
            Box::new(Passthrough),
            "/bin/sh",
            &["-c", "sleep 30"],
            1,
            640,
            360,
        )
        .unwrap_or_else(|e| unreachable!("start: {e}"))
    }

    fn split_tab() -> (Tab, PhysicalSize<u32>) {
        let size = PhysicalSize::new(640, 360);
        let mut tab = Tab::new(app(), String::new());
        tab.panes.split(app(), Split::Right);
        tab.resize(size);
        (tab, size)
    }

    #[test]
    fn a_pointer_is_in_the_pane_under_it_and_in_that_panes_own_terms() {
        let (tab, size) = split_tab();
        let strip = tab.strip();
        let layout = tab.layout(size);
        let right = layout.panes[1];
        assert_eq!(tab.pane_at(size, 3, strip + 5), Some((0, 3, 5)));
        assert_eq!(
            tab.pane_at(size, right.x + 7, strip + 9),
            Some((1, 7, 9)),
            "the second pane's pixel is counted from its own corner"
        );
        assert_eq!(
            tab.pane_at(size, 3, strip - 1),
            None,
            "the tab strip is no pane"
        );
        assert_eq!(
            tab.pane_at(size, layout.dividers[0].x, strip + 5),
            None,
            "a divider is no pane"
        );
    }

    #[test]
    fn a_drag_stays_in_its_pane_and_is_held_at_its_edge() {
        let (tab, size) = split_tab();
        let strip = tab.strip();
        let left = tab.layout(size).panes[0];
        // Over the strip: the top row. Far right: the pane's last column.
        assert_eq!(tab.pane_clamped(size, 0, 5, 0), Some((5, 0)));
        assert_eq!(
            tab.pane_clamped(size, 0, 600, strip + 10),
            Some((left.w - 1, 10))
        );
        assert_eq!(tab.pane_clamped(size, 9, 5, 5), None);
    }

    #[test]
    fn every_pane_is_sized_to_its_place() {
        let (tab, size) = split_tab();
        let layout = tab.layout(size);
        let sizes: Vec<(u32, u32)> = tab.panes.all().iter().map(|a| a.size()).collect();
        let places: Vec<(u32, u32)> = layout.panes.iter().map(|r| (r.w, r.h)).collect();
        assert_eq!(sizes, places);
        assert!(places[0].0 < 640 / 2 + 1, "the panes share the width");
    }
}

#[cfg(all(test, unix))]
mod frame_tests {
    use super::{Input, ime_event, pump_frame, send_input, start_frame_wakes};
    use crate::input_line::Mode;
    use crate::{App, Key, Mods, Passthrough};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::{Duration, Instant};
    use winit::event::Ime;

    const TIMEOUT: Duration = Duration::from_secs(10);

    fn start(program: &str, args: &[&str]) -> App {
        App::start(Box::new(Passthrough), program, args, 1, 640, 360)
            .unwrap_or_else(|e| unreachable!("start {program}: {e}"))
    }

    fn frame_wakes() -> (Sender<()>, Receiver<()>) {
        let (wake, events) = channel();
        let handled = start_frame_wakes(move || wake.send(()).is_ok())
            .unwrap_or_else(|e| unreachable!("start frame wakes: {e}"));
        (handled, events)
    }

    #[test]
    fn keyboard_and_ime_input_are_visible_on_wakes_without_pointer_events() {
        let mut app = start("/bin/zsh", &["-f", "-i"]);
        let end = Instant::now() + TIMEOUT;
        while app.input_mode() != Mode::Editing {
            let left = end.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "zsh reaches the input editor");
            app.pump_wait(left);
        }
        let (handled, wakes) = frame_wakes();
        let empty = app.view();
        assert!(send_input(
            &mut app,
            Input::Key(Key::Char('K'), Mods::default())
        ));
        assert!(wakes.recv_timeout(TIMEOUT).is_ok(), "a frame wake arrives");
        let typed = app.view();
        assert!(typed != empty, "the key changed the rendered frame");
        assert_eq!(app.input_text(), "K");

        assert!(ime_event(&mut app, &Ime::Commit("é".into())));
        assert!(handled.send(()).is_ok());
        assert!(
            wakes.recv_timeout(TIMEOUT).is_ok(),
            "the next frame wake arrives"
        );
        assert!(app.view() != typed, "the commit changed the rendered frame");
        assert_eq!(app.input_text(), "Ké");
    }

    #[test]
    fn an_idle_window_repaints_for_pty_output_without_input() {
        let mut app = start("/bin/sh", &["-c", "printf 'PTY repaint'; read -r keep"]);
        let empty = app.view();
        let (handled, wakes) = frame_wakes();
        let end = Instant::now() + TIMEOUT;
        loop {
            assert!(
                wakes
                    .recv_timeout(end.saturating_duration_since(Instant::now()))
                    .is_ok()
            );
            let repaint = pump_frame(&mut app);
            if repaint
                && app
                    .visible_text()
                    .iter()
                    .any(|row| row.contains("PTY repaint"))
            {
                assert!(app.view() != empty, "the PTY changed the rendered frame");
                break;
            }
            assert!(handled.send(()).is_ok());
        }
    }
}
