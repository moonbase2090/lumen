//! The window: winit for the window and its events, softbuffer to put the
//! frame on it. Everything it does is a call on [`App`], which is tested
//! without a window; this file holds only what turns an OS event into one.

use crate::app::App;
use crate::design::Appearance;
use crate::keys::{Key, Mods};
use crate::layer::Layer;
use crate::mouse::{Button, Held, Wheel};
use crate::raster::Frame;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
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
    let (mapped, mods) = map_standard(key, text, state)?;
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

fn map_standard(key: &WinitKey, text: Option<&str>, state: ModifiersState) -> Option<(Key, Mods)> {
    if state.super_key() {
        return None;
    }
    let mods = Mods {
        ctrl: state.control_key(),
        alt: state.alt_key(),
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

/// Send `input` to the program. Returns whether the window must draw again
/// because the user now has something to see: when the input was not sent, the
/// app has put a notice on screen and the window must show it.
pub fn send_input(app: &mut App, input: Input<'_>) -> bool {
    let sent = match input {
        Input::Key(key, mods) => app.key(key, mods),
        Input::Text(text) => app.text(text),
    };
    sent.is_err()
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
}

struct Running {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    app: App,
}

struct Host {
    config: Config,
    state: Option<Running>,
    modifiers: ModifiersState,
    started: Instant,
    error: Option<String>,
    /// The pointer's last position in pixels.
    pointer: (u32, u32),
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
        let view = run.app.view();
        let Ok(mut buffer) = run.surface.buffer_mut() else {
            return;
        };
        let words = to_xrgb(&view);
        let n = buffer.len().min(words.len());
        buffer[..n].copy_from_slice(&words[..n]);
        let _ = buffer.present();
    }
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(crate::APP_NAME)
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
            None => (default_shell(), Vec::new()),
        };
        match App::start(
            (self.config.layer)(),
            &program,
            &arguments,
            scale,
            size.width,
            size.height,
        ) {
            Ok(mut app) => {
                app.use_settings(self.config.notifications);
                app.use_images(self.config.images);
                // The system's appearance, if it says; light until it does.
                app.set_appearance(appearance(window.theme()));
                window.request_redraw();
                self.state = Some(Running {
                    window,
                    surface,
                    app,
                });
            }
            Err(e) => self.fail(event_loop, e.to_string()),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::Resized(PhysicalSize { width, height }) => {
                if let Some(run) = &mut self.state {
                    let resized = run.app.resize(width, height).is_ok();
                    if resized {
                        run.window.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let Some(run) = &mut self.state else { return };
                if let Some((key, mods)) = map_key_at(
                    &event.logical_key,
                    event.text.as_deref(),
                    self.modifiers,
                    event.location,
                ) {
                    if send_input(&mut run.app, Input::Key(key, mods)) {
                        run.window.request_redraw();
                    }
                } else if let Some(text) = event.text.as_deref()
                    && !self.modifiers.super_key()
                    && text.chars().count() > 1
                    && send_input(&mut run.app, Input::Text(text))
                {
                    run.window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = pointer_pixels(position.x, position.y);
                let held = self.held();
                if let Some(run) = &mut self.state
                    && run
                        .app
                        .mouse_move(self.pointer.0, self.pointer.1, held)
                        .unwrap_or(false)
                {
                    run.window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let held = self.held();
                let (x, y) = self.pointer;
                let Some(run) = &mut self.state else { return };
                let mut redraw = false;
                if state == ElementState::Pressed && button == MouseButton::Left {
                    redraw = run.app.click(x, y);
                }
                if let Some(b) = map_button(button) {
                    redraw |= match state {
                        ElementState::Pressed => run.app.mouse_press(x, y, b, held),
                        ElementState::Released => run.app.mouse_release(x, y, b, held),
                    }
                    .unwrap_or(false);
                }
                if redraw {
                    run.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let held = self.held();
                let (x, y) = self.pointer;
                if let Some(run) = &mut self.state {
                    let line = f64::from(run.app.cell_height());
                    if let Some((step, n)) = wheel_steps(&delta, line) {
                        for _ in 0..n {
                            if run.app.mouse_wheel(x, y, step, held).unwrap_or(false) {
                                run.window.request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::ThemeChanged(theme) => {
                if let Some(run) = &mut self.state
                    && run.app.set_appearance(appearance(Some(theme)))
                {
                    run.window.request_redraw();
                }
            }
            WindowEvent::Focused(focused) => {
                if let Some(run) = &mut self.state {
                    run.app.set_focused(focused);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(limit) = self.config.exit_after
            && self.started.elapsed() >= limit
        {
            event_loop.exit();
            return;
        }
        if let Some(run) = &mut self.state {
            let expired = run.app.tick();
            if run.app.pump() > 0 || expired {
                run.window.request_redraw();
            }
            if run.app.has_exited() {
                event_loop.exit();
                return;
            }
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

fn default_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned())
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
    };
    event_loop
        .run_app(&mut host)
        .map_err(|e| format!("event loop: {e}"))?;
    host.error.map_or(Ok(()), Err)
}
