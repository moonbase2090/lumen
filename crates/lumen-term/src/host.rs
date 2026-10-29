//! The window: winit for the window and its events, softbuffer to put the
//! frame on it. Everything it does is a call on [`App`], which is tested
//! without a window; this file holds only what turns an OS event into one.

use crate::app::App;
use crate::keys::{Key, Mods};
use crate::layer::Layer;
use crate::raster::Frame;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

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

/// A window key to the key the program gets, with its modifiers. `None` for a
/// key that sends nothing, or that belongs to the operating system (Super).
///
/// `text` is what the key produced, which carries shift and layout; with Control
/// held the logical key is used instead, since the text is a control code then.
#[must_use]
pub fn map_key(key: &WinitKey, text: Option<&str>, state: ModifiersState) -> Option<(Key, Mods)> {
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
            .with_title("lumen")
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
                if let Some((key, mods)) =
                    map_key(&event.logical_key, event.text.as_deref(), self.modifiers)
                {
                    let _ = run.app.key(key, mods);
                } else if let Some(text) = event.text.as_deref()
                    && !self.modifiers.super_key()
                    && text.chars().count() > 1
                {
                    let _ = run.app.text(text);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = pointer_pixels(position.x, position.y);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(run) = &mut self.state
                    && run.app.click(self.pointer.0, self.pointer.1)
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
