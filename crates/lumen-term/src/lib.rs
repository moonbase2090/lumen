//! The headless core of Bright: blocks, and the Lumen arm layer in front of them.
//!
//! A [`GridBlock`] is a region of terminal cells with its own emulator and,
//! when it runs a program, its own PTY sized to the block. A [`RichBlock`] is a
//! Track 0 block the arm layer let through. The [`Document`] turns the pieces
//! a [`Layer`] yields for the bytes of a stream into a list of blocks.
//!
//! [`raster`] paints blocks to an RGBA frame with one bundled face, in the
//! colours of the [`design`] tokens. Nothing
//! here opens a window. The terminal is built on Arm B (Track 0 rich on every
//! layer, with the sealed upgrade): [`Layer`] is implemented for its layer, and
//! the plain [`Passthrough`] layer is for a stream with no arm in front of it.
//! Arm A is kept as the evidence record in its own crate and is not used here.

#![deny(unsafe_code)]

pub mod actions;
/// The terminal's user-facing name.
pub const APP_NAME: &str = "Bright";

#[cfg(feature = "window")]
pub mod access;
pub mod app;
pub mod attention;
pub mod boundary;
pub mod cli;
#[cfg(feature = "links-clipboard")]
pub mod clipboard;
pub mod contextmenu;
pub mod contrast;
pub mod copy;
pub mod design;
pub mod document;
#[cfg(all(feature = "window", any(target_os = "macos", windows)))]
#[allow(
    unsafe_code,
    reason = "the platform's call that shows a context menu in a window; see the module docs"
)]
pub mod ffi_context_menu;
pub mod frame;
pub mod glide;
pub mod grid;
pub mod guard;
#[cfg(feature = "window")]
pub mod host;
pub mod image_draw;
pub mod images;
pub mod input_line;
pub mod isolate;
pub mod keys;
pub mod layer;
pub mod layout;
#[cfg(feature = "links-clipboard")]
pub mod link;
#[cfg(feature = "links-clipboard")]
pub mod linkhit;
#[cfg(feature = "links-clipboard")]
pub mod linkprompt;
pub mod marks;
#[cfg(all(feature = "window", any(target_os = "macos", windows)))]
pub mod menu;
pub mod modes;
pub mod motion;
pub mod mouse;
pub mod notify;
pub mod os_backend;
#[cfg(feature = "links-clipboard")]
pub mod paste;
pub mod policy;
mod pty;
pub mod raster;
pub mod reply;
pub mod scroll;
#[cfg(target_os = "linux")]
pub mod seal;
pub mod search;
pub mod select;
pub mod session;
pub mod settings;
#[cfg(all(feature = "window", target_os = "macos"))]
mod settings_command;
pub mod shell;
pub mod shellint;
pub mod strip;
#[cfg(feature = "window")]
mod tabs;
pub mod theme_file;
pub mod title;
pub mod typed;
#[cfg(feature = "window")]
mod window_state;
mod writer;

pub use app::App;
pub use document::{Block, Document, Event, RichBlock};
pub use grid::{CursorShape, CursorView, GridBlock, Line};
pub use keys::{Key, Mods};
pub use layer::{Context, Layer, Notice, Passthrough, Piece};
pub use layout::{
    GUTTER_COLUMNS, HEADER_ROWS, Layout, Rect, StripPlacement, layout, notification_area,
};
pub use modes::{Changes, KeyModes, ModeTracker};
pub use pty::SpawnError;
pub use raster::{CursorState, Frame, Renderer, Theme, Window, wrap_text};
pub use scroll::Scroll;
pub use search::{Find, Hit, Mark, Overlay, SearchRow};
pub use session::Session;
pub use strip::Strip;
