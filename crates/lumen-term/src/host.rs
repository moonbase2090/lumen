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
use crate::raster::Frame;
use crate::settings::Watcher as SettingsWatcher;
use crate::tabs::Tabs;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, KeyLocation, ModifiersState, NamedKey};
use winit::window::{Theme as WinitTheme, Window, WindowId};

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

/// A pointer row in the active tab's content. A captured press keeps receiving
/// motion and its release at the top content row while the pointer is over the
/// terminal-owned tab strip.
fn content_pointer_y(y: u32, strip_height: u32, captured: bool) -> Option<u32> {
    y.checked_sub(strip_height)
        .or_else(|| captured.then_some(0))
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
    top: u32,
}

impl ImeSink for WindowSink<'_> {
    fn set_area(&mut self, (x, y, w, h): ImeRect) {
        self.window.set_ime_cursor_area(
            PhysicalPosition::new(x, y.saturating_add(self.top)),
            PhysicalSize::new(w, h),
        );
    }
}

/// Send `input` to the program. Returns whether the window must draw again
/// because the user now has something to see: when the input was not sent, the
/// app has put a notice on screen and the window must show it.
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
}

struct Tab {
    app: App,
    title: String,
}

struct Running {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    tabs: Tabs<Tab>,
    shown_title: String,
    #[cfg(target_os = "macos")]
    _menu: Option<crate::menu::MenuBar>,
}

struct Host {
    config: Config,
    state: Option<Running>,
    modifiers: ModifiersState,
    started: Instant,
    error: Option<String>,
    /// The pointer's last position in pixels.
    pointer: (u32, u32),
    /// Keeps the system's candidate rectangle with the app.
    ime: ImeFollower,
    /// Whether the operating system says this window owns the focus.
    focused: bool,
    /// The last answer the system gave about reduced motion, kept when a later read gives none.
    reduce_motion: std::cell::Cell<bool>,
    /// A button whose press began in the active tab's content. Its drag and
    /// release stay with the content when the pointer crosses the tab strip.
    mouse_capture: Option<MouseButton>,
    /// The next time the small settings file is read for a change.
    next_settings_check: Instant,
}

impl Host {
    /// The modifier keys held now, as a mouse report counts them.
    fn held(&self) -> Held {
        Held {
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
            ctrl: self.modifiers.control_key(),
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
        app.use_settings(self.config.notifications);
        app.use_images(self.config.images);
        app.set_theme_choice(look.theme);
        app.set_copy_on_select(raw.copy_on_select);
        app.set_smooth_scroll(raw.smooth_scroll);
        if let Some(reduce) = crate::motion::system_reduces_motion() {
            self.reduce_motion.set(reduce);
        }
        app.set_reduce_motion(self.reduce_motion.get());
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
        let Some(run) = &mut self.state else { return };
        let mut redraw = false;
        let mut resized = false;
        for tab in run.tabs.iter_mut() {
            redraw |= tab.app.set_theme_choice(look.theme);
            match tab.app.set_fonts(&look.fonts) {
                Ok(changed) => resized |= changed,
                Err(e) => eprintln!("{}: fonts not used ({e})", crate::APP_NAME),
            }
            tab.app.set_copy_on_select(raw.copy_on_select);
            tab.app.set_smooth_scroll(raw.smooth_scroll);
        }
        if resized {
            redraw |= Self::resize_tabs(run, run.window.inner_size());
        }
        if redraw {
            run.window.request_redraw();
        }
        if resized {
            self.refocus();
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

    fn refocus(&mut self) {
        let Some(run) = &mut self.state else { return };
        let active = run.tabs.active_index();
        for (index, tab) in run.tabs.iter_mut().enumerate() {
            tab.app.set_focused(self.focused && index == active);
            if index != active {
                tab.app.clear_preedit();
            }
        }
        self.ime.reset();
        let Some(tab) = run.tabs.active() else { return };
        let mut sink = WindowSink {
            window: &run.window,
            top: tab.app.cell_height(),
        };
        self.ime.follow(&tab.app, &mut sink);
        Self::update_window_title(run);
        run.window.request_redraw();
    }

    fn select_tab(&mut self, index: usize) {
        let changed = self
            .state
            .as_mut()
            .is_some_and(|run| run.tabs.select(index));
        if changed {
            self.refocus();
        }
    }

    fn cycle_tab(&mut self, next: bool) {
        let changed = self.state.as_mut().is_some_and(|run| {
            if next {
                run.tabs.next()
            } else {
                run.tabs.previous()
            }
        });
        if changed {
            self.refocus();
        }
    }

    fn new_tab(&mut self) {
        let Some(window) = self.state.as_ref().map(|run| run.window.clone()) else {
            return;
        };
        let app = match self.start_app(&window) {
            Ok(app) => app,
            Err(e) => {
                eprintln!("{}: new tab not opened ({e})", crate::APP_NAME);
                return;
            }
        };
        if let Some(run) = &mut self.state {
            run.tabs.push(Tab {
                app,
                title: crate::title::window_title(None),
            });
        }
        self.refocus();
    }

    fn close_active(&mut self, event_loop: &ActiveEventLoop) {
        let empty = self.state.as_mut().is_none_or(|run| {
            run.tabs.remove_active();
            run.tabs.is_empty()
        });
        if empty {
            event_loop.exit();
        } else {
            self.refocus();
        }
    }

    fn resize_tabs(run: &mut Running, size: PhysicalSize<u32>) -> bool {
        let mut changed = false;
        for tab in run.tabs.iter_mut() {
            let height = size.height.saturating_sub(tab.app.cell_height());
            changed |= tab.app.resize(size.width, height).is_ok();
        }
        changed
    }

    fn redraw(&mut self) {
        let Some(run) = &mut self.state else { return };
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
        let view = tab
            .app
            .view_with_tabs(&titles, run.tabs.active_index(), size.height);
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
    /// The wheel or trackpad moved: a program that asked for wheel reports gets
    /// them, and otherwise the output scrolls, by whole rows for a wheel and by
    /// the pixels moved for a trackpad.
    fn wheel(&mut self, delta: &MouseScrollDelta) {
        let held = self.held();
        let (x, y) = self.pointer;
        let Some(run) = &mut self.state else { return };
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        let top = tab.app.cell_height();
        let Some(y) = y.checked_sub(top) else { return };
        let line = f64::from(top);
        let redraw = if let MouseScrollDelta::PixelDelta(p) = delta {
            tab.app
                .wheel_pixels(x, y, wheel_pixels(p.y), held)
                .unwrap_or(false)
        } else if let Some((step, n)) = wheel_steps(delta, line) {
            (0..n).fold(false, |moved, _| {
                tab.app.mouse_wheel_glide(x, y, step, held).unwrap_or(false) | moved
            })
        } else {
            false
        };
        if redraw {
            run.window.request_redraw();
        }
    }

    /// Do one of the window's own actions, from a menu item or a shortcut.
    fn act(&mut self, event_loop: &ActiveEventLoop, action: Action) {
        match action {
            Action::NewTab => return self.new_tab(),
            Action::NextTab => return self.cycle_tab(true),
            Action::PreviousTab => return self.cycle_tab(false),
            Action::Settings => {
                #[cfg(target_os = "macos")]
                if let Err(e) = crate::settings_command::open() {
                    eprintln!("{}: settings not opened ({e})", crate::APP_NAME);
                }
                return;
            }
            Action::Close => return self.close_active(event_loop),
            Action::Quit => {
                event_loop.exit();
                return;
            }
            _ => {}
        }
        let Some(run) = &mut self.state else { return };
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        let top = tab.app.cell_height();
        let outcome = perform_action(
            &mut tab.app,
            action,
            &SystemClipboard,
            &mut self.ime,
            &mut WindowSink {
                window: &run.window,
                top,
            },
        );
        if matches!(action, Action::ZoomIn | Action::ZoomOut | Action::ZoomReset) {
            let size = run.window.inner_size();
            let height = size.height.saturating_sub(tab.app.cell_height());
            let _ = tab.app.resize(size.width, height);
        }
        // An open context menu was closed by the action: draw without it.
        let menu_closed = tab.app.take_view_changed();
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
    fn on_key(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) {
        let Some(run) = &mut self.state else { return };
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        if tab.app.preedit().is_some() {
            // The input method owns the keys while it composes.
            return;
        }
        if let WinitKey::Character(c) = &event.logical_key
            && let Some(ch) = c.chars().next()
            && let Some(action) =
                actions::action_for(Platform::current(), ch, chord(self.modifiers))
        {
            self.act(event_loop, action);
            return;
        }
        if let Some((key, mods)) = map_key_at(
            &event.logical_key,
            event.text.as_deref(),
            self.modifiers,
            event.location,
        ) {
            if send_input(&mut tab.app, Input::Key(key, mods)) {
                run.window.request_redraw();
            }
            if let Some(action) = tab.app.take_menu_choice() {
                self.act(event_loop, action);
            }
        } else if let Some(text) = event.text.as_deref()
            && !self.modifiers.super_key()
            && text.chars().count() > 1
            && send_input(&mut tab.app, Input::Text(text))
        {
            run.window.request_redraw();
        }
    }
}

impl Host {
    fn on_scale(&mut self, scale_factor: f64) {
        let Some(run) = &mut self.state else { return };
        let size = run.window.inner_size();
        let mut changed = false;
        for tab in run.tabs.iter_mut() {
            changed |= tab.app.set_display_scale(scale_factor).unwrap_or(false);
            let height = size.height.saturating_sub(tab.app.cell_height());
            changed |= tab.app.resize(size.width, height).is_ok();
        }
        self.ime.reset();
        if let Some(tab) = run.tabs.active() {
            let mut sink = WindowSink {
                window: &run.window,
                top: tab.app.cell_height(),
            };
            self.ime.follow(&tab.app, &mut sink);
        }
        if changed {
            run.window.request_redraw();
        }
    }

    fn on_ime(&mut self, event: &Ime) {
        let Some(run) = &mut self.state else { return };
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        let redraw = ime_event(&mut tab.app, event);
        let top = tab.app.cell_height();
        let mut sink = WindowSink {
            window: &run.window,
            top,
        };
        self.ime.follow(&tab.app, &mut sink);
        if redraw {
            run.window.request_redraw();
        }
    }

    fn on_focus(&mut self, focused: bool) {
        self.focused = focused;
        // The system setting can change while another window has the focus.
        if focused && let Some(reduce) = crate::motion::system_reduces_motion() {
            self.reduce_motion.set(reduce);
        }
        let reduce = self.reduce_motion.get();
        let Some(run) = &mut self.state else { return };
        let active = run.tabs.active_index();
        for (index, tab) in run.tabs.iter_mut().enumerate() {
            tab.app.set_reduce_motion(reduce);
            tab.app.set_focused(focused && index == active);
            if !focused {
                tab.app.clear_preedit();
            }
        }
        self.ime.reset();
        if let Some(tab) = run.tabs.active() {
            let mut sink = WindowSink {
                window: &run.window,
                top: tab.app.cell_height(),
            };
            self.ime.follow(&tab.app, &mut sink);
        }
        run.window.request_redraw();
    }

    fn on_cursor(&mut self, position: PhysicalPosition<f64>) {
        self.pointer = pointer_pixels(position.x, position.y);
        let held = self.held();
        let captured = self.mouse_capture.is_some();
        let Some(run) = &mut self.state else { return };
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        let redraw = content_pointer_y(self.pointer.1, tab.app.cell_height(), captured)
            .is_some_and(|y| tab.app.mouse_move(self.pointer.0, y, held).unwrap_or(false))
            | tab.app.take_selection_redraw();
        if redraw {
            run.window.request_redraw();
        }
    }

    fn on_mouse(&mut self, event_loop: &ActiveEventLoop, state: ElementState, button: MouseButton) {
        let held = self.held();
        let (x, y) = self.pointer;
        if state == ElementState::Pressed && button == MouseButton::Left {
            let selected = self.state.as_ref().and_then(|run| {
                let top = run.tabs.active()?.app.cell_height();
                (y < top)
                    .then(|| run.tabs.hit(x, run.window.inner_size().width))
                    .flatten()
            });
            if let Some(index) = selected {
                self.select_tab(index);
                return;
            }
        }
        let Some(run) = &mut self.state else { return };
        let Some(tab) = run.tabs.active_mut() else {
            return;
        };
        let captured = self.mouse_capture == Some(button);
        let Some(y) = content_pointer_y(y, tab.app.cell_height(), captured) else {
            return;
        };
        #[cfg(any(target_os = "macos", windows))]
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && !tab.app.is_selecting()
            && tab.app.owns_context_click(x, y, held)
        {
            let menu_state = crate::menu::ContextState {
                can_copy: tab.app.has_selection(),
                can_paste: SystemClipboard.text().is_some(),
            };
            if let Ok(menu) = crate::menu::context_menu(menu_state) {
                crate::ffi_context_menu::show(&menu, &*run.window);
            }
            return;
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        if state == ElementState::Pressed
            && button == MouseButton::Right
            && !tab.app.is_selecting()
            && tab.app.owns_context_click(x, y, held)
        {
            // There is no system menu here: the terminal draws its own.
            if tab
                .app
                .open_context_menu(x, y, SystemClipboard.text().is_some())
            {
                run.window.request_redraw();
            }
            return;
        }
        let mut redraw = false;
        let mut taken = false;
        if state == ElementState::Pressed && button == MouseButton::Left {
            redraw = tab.app.click(x, y);
            let link_modifier = if cfg!(target_os = "macos") {
                self.modifiers.super_key()
            } else {
                self.modifiers.control_key()
            };
            if tab.app.link_prompt().is_some()
                || (link_modifier && tab.app.context_menu().is_none())
            {
                taken = tab.app.link_press(x, y);
                redraw |= taken;
            }
        }
        if !taken && let Some(b) = map_button(button) {
            redraw |= match state {
                ElementState::Pressed => tab.app.mouse_press(x, y, b, held),
                ElementState::Released => tab.app.mouse_release(x, y, b, held),
            }
            .unwrap_or(false);
            match state {
                ElementState::Pressed => self.mouse_capture = Some(button),
                ElementState::Released if captured => self.mouse_capture = None,
                ElementState::Released => {}
            }
        }
        redraw |= tab.app.take_selection_redraw();
        let copy = tab.app.take_copy_on_select();
        let choice = tab.app.take_menu_choice();
        if redraw {
            run.window.request_redraw();
        }
        if copy {
            self.act(event_loop, Action::Copy);
        }
        if let Some(action) = choice {
            self.act(event_loop, action);
        }
    }

    fn on_theme(&mut self, theme: WinitTheme) {
        let Some(run) = &mut self.state else { return };
        let changed = run.tabs.iter_mut().fold(false, |changed, tab| {
            tab.app.set_appearance(appearance(Some(theme))) | changed
        });
        if changed {
            run.window.request_redraw();
        }
    }

    /// Handle one window event.
    fn handle_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::Resized(size) => {
                if let Some(run) = &mut self.state
                    && Self::resize_tabs(run, size)
                {
                    run.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.on_scale(scale_factor),
            WindowEvent::Ime(event) => self.on_ime(&event),
            WindowEvent::Focused(focused) => self.on_focus(focused),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.on_key(event_loop, &event);
            }
            WindowEvent::CursorMoved { position, .. } => self.on_cursor(position),
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse(event_loop, state, button);
            }
            WindowEvent::MouseWheel { delta, .. } => self.wheel(&delta),
            WindowEvent::ThemeChanged(theme) => self.on_theme(theme),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(crate::title::window_title(None))
            .with_inner_size(winit::dpi::LogicalSize::new(
                f64::from(self.config.size.0),
                f64::from(self.config.size.1),
            ));
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Rc::new(w),
            Err(e) => return self.fail(event_loop, format!("no window: {e}")),
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => return self.fail(event_loop, format!("no display context: {e}")),
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => return self.fail(event_loop, format!("no surface: {e}")),
        };
        window.set_ime_allowed(true);
        match self.start_app(&window) {
            Ok(app) => {
                window.request_redraw();
                // A menu that cannot be built leaves the shortcuts working.
                #[cfg(target_os = "macos")]
                let menu = crate::menu::MenuBar::install().ok();
                self.state = Some(Running {
                    window,
                    surface,
                    tabs: Tabs::new(Tab {
                        app,
                        title: crate::title::window_title(None),
                    }),
                    shown_title: crate::title::window_title(None),
                    #[cfg(target_os = "macos")]
                    _menu: menu,
                });
            }
            Err(e) => self.fail(event_loop, e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.handle_window_event(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(limit) = self.config.exit_after
            && self.started.elapsed() >= limit
        {
            event_loop.exit();
            return;
        }
        let now = Instant::now();
        if now >= self.next_settings_check {
            self.next_settings_check = now + Duration::from_millis(250);
            self.reload_settings();
        }
        #[cfg(any(target_os = "macos", windows))]
        for action in crate::menu::chosen() {
            self.act(event_loop, action);
        }
        let mut refocus = false;
        if let Some(run) = &mut self.state {
            let active = run.tabs.active_index();
            let mut redraw = false;
            let mut exited = Vec::new();
            let mut request_attention = false;
            let mut cancel_attention = false;
            for (index, tab) in run.tabs.iter_mut().enumerate() {
                let expired = tab.app.tick();
                if tab.app.pump() > 0 || expired {
                    redraw |= index == active;
                }
                if let Some(title) = tab.app.take_window_title() {
                    tab.title = title;
                    redraw = true;
                }
                match tab.app.attention_step() {
                    crate::attention::Step::Request => request_attention = true,
                    crate::attention::Step::Cancel => cancel_attention = true,
                    crate::attention::Step::Idle => {}
                }
                if tab.app.has_exited() {
                    exited.push(index);
                }
            }
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
                event_loop.exit();
                return;
            }
            if redraw || refocus {
                run.window.request_redraw();
            }
            Self::update_window_title(run);
        }
        if refocus {
            self.refocus();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
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
    let event_loop = EventLoop::new().map_err(|e| format!("no event loop: {e}"))?;
    let mut host = Host {
        config,
        state: None,
        modifiers: ModifiersState::default(),
        started: Instant::now(),
        error: None,
        pointer: (0, 0),
        ime: ImeFollower::default(),
        focused: false,
        reduce_motion: std::cell::Cell::new(false),
        mouse_capture: None,
        next_settings_check: Instant::now(),
    };
    event_loop
        .run_app(&mut host)
        .map_err(|e| format!("event loop: {e}"))?;
    host.error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::content_pointer_y;

    #[test]
    fn a_captured_pointer_keeps_the_top_content_row_over_the_tab_strip() {
        assert_eq!(content_pointer_y(25, 20, false), Some(5));
        assert_eq!(content_pointer_y(10, 20, false), None);
        assert_eq!(content_pointer_y(10, 20, true), Some(0));
    }
}
