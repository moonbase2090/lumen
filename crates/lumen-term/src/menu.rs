//! The macOS menu bar. Each item that does something the terminal itself does
//! carries an [`Action`] and the usual Command chord; the items the system
//! performs for every application (hide, hide others, show all, minimise, zoom
//! the window, full screen, about) are the system's own. There is no menu bar
//! on other platforms.
//!
//! The context menu, shown by a right click, is built here for macOS and
//! Windows. It carries the same actions as the menu bar and the shortcuts, with
//! an item turned off when its action is not available. Showing it needs the
//! platform's own call, which lives in [`crate::ffi_context_menu`]. Linux has no
//! context menu yet.

use crate::actions::Action;
#[cfg(target_os = "macos")]
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuEvent, MenuItem};
#[cfg(target_os = "macos")]
use muda::{PredefinedMenuItem, Submenu};

#[cfg(target_os = "macos")]
use crate::APP_NAME;

/// The menu id of an action's item.
#[must_use]
pub const fn id_of(action: Action) -> &'static str {
    match action {
        Action::NewWindow => "new-window",
        Action::NewTab => "new-tab",
        Action::NextTab => "next-tab",
        Action::PreviousTab => "previous-tab",
        Action::Copy => "copy",
        Action::Paste => "paste",
        Action::SplitRight => "split-right",
        Action::SplitDown => "split-down",
        Action::NextPane => "next-pane",
        Action::PreviousPane => "previous-pane",
        Action::SelectAll => "select-all",
        Action::ZoomIn => "zoom-in",
        Action::ZoomOut => "zoom-out",
        Action::ZoomReset => "zoom-reset",
        Action::Find => "find",
        Action::FindNext => "find-next",
        Action::FindPrevious => "find-previous",
        Action::NextLink => "next-link",
        Action::PreviousLink => "previous-link",
        Action::OpenLink => "open-link",
        Action::CopyLinkAddress => "copy-link-address",
        Action::Settings => "settings",
        Action::Close => "close",
        Action::Quit => "quit",
    }
}

/// The action an item id stands for.
#[must_use]
pub fn action_of(id: &str) -> Option<Action> {
    [
        Action::NewWindow,
        Action::NewTab,
        Action::NextTab,
        Action::PreviousTab,
        Action::Copy,
        Action::Paste,
        Action::SplitRight,
        Action::SplitDown,
        Action::NextPane,
        Action::PreviousPane,
        Action::SelectAll,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ZoomReset,
        Action::Find,
        Action::FindNext,
        Action::FindPrevious,
        Action::NextLink,
        Action::PreviousLink,
        Action::OpenLink,
        Action::CopyLinkAddress,
        Action::Settings,
        Action::Close,
        Action::Quit,
    ]
    .into_iter()
    .find(|a| id_of(*a) == id)
}

#[cfg(target_os = "macos")]
fn item(action: Action, text: &str, code: Code) -> MenuItem {
    item_with(action, text, Modifiers::META, code)
}

#[cfg(target_os = "macos")]
fn item_with(action: Action, text: &str, modifiers: Modifiers, code: Code) -> MenuItem {
    MenuItem::with_id(
        id_of(action),
        text,
        true,
        Some(Accelerator::new(modifiers, code)),
    )
}

/// The menu bar, kept alive for as long as the window runs.
#[cfg(target_os = "macos")]
pub struct MenuBar {
    _menu: Menu,
}

#[cfg(target_os = "macos")]
impl MenuBar {
    /// Build the menu bar and make it the application's. Must run on the main
    /// thread, once the application exists.
    ///
    /// # Errors
    ///
    /// Returns why a menu could not be built.
    pub fn install() -> Result<Self, String> {
        let err = |e: muda::Error| e.to_string();
        let menu = Menu::new();

        let app = Submenu::with_items(
            APP_NAME,
            true,
            &[
                &PredefinedMenuItem::about(None, None),
                &item(Action::Settings, "Settings…", Code::Comma),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &item(Action::Quit, &format!("Quit {APP_NAME}"), Code::KeyQ),
            ],
        )
        .map_err(err)?;
        let edit = Submenu::with_items(
            "Edit",
            true,
            &[
                &item(Action::Copy, "Copy", Code::KeyC),
                &item(Action::Paste, "Paste", Code::KeyV),
                &item(Action::SelectAll, "Select All", Code::KeyA),
                &PredefinedMenuItem::separator(),
                &item(Action::Find, "Find…", Code::KeyF),
                &item(Action::FindNext, "Find Next", Code::KeyG),
                &item_with(
                    Action::FindPrevious,
                    "Find Previous",
                    Modifiers::META | Modifiers::SHIFT,
                    Code::KeyG,
                ),
                &PredefinedMenuItem::separator(),
                &item(Action::NextLink, "Next Link", Code::KeyL),
                &item_with(
                    Action::PreviousLink,
                    "Previous Link",
                    Modifiers::META | Modifiers::SHIFT,
                    Code::KeyL,
                ),
            ],
        )
        .map_err(err)?;
        let view = Submenu::with_items(
            "View",
            true,
            &[
                &item(Action::ZoomIn, "Zoom In", Code::Equal),
                &item(Action::ZoomOut, "Zoom Out", Code::Minus),
                &item(Action::ZoomReset, "Actual Size", Code::Digit0),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::fullscreen(None),
            ],
        )
        .map_err(err)?;
        let window = Submenu::with_items(
            "Window",
            true,
            &[
                &item(Action::NewWindow, "New Window", Code::KeyN),
                &item(Action::NewTab, "New Tab", Code::KeyT),
                &item_with(
                    Action::PreviousTab,
                    "Previous Tab",
                    Modifiers::META | Modifiers::SHIFT,
                    Code::BracketLeft,
                ),
                &item_with(
                    Action::NextTab,
                    "Next Tab",
                    Modifiers::META | Modifiers::SHIFT,
                    Code::BracketRight,
                ),
                &PredefinedMenuItem::separator(),
                &item(Action::SplitRight, "Split Right", Code::KeyD),
                &item_with(
                    Action::SplitDown,
                    "Split Down",
                    Modifiers::META | Modifiers::SHIFT,
                    Code::KeyD,
                ),
                &item(Action::NextPane, "Next Pane", Code::BracketRight),
                &item(Action::PreviousPane, "Previous Pane", Code::BracketLeft),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::maximize(None),
                &PredefinedMenuItem::separator(),
                &item(Action::Close, "Close Pane", Code::KeyW),
            ],
        )
        .map_err(err)?;
        menu.append_items(&[&app, &edit, &view, &window])
            .map_err(err)?;
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        Ok(Self { _menu: menu })
    }

    /// The actions chosen from the menu since the last call.
    #[must_use]
    pub fn chosen() -> Vec<Action> {
        chosen()
    }
}

/// The actions chosen from a menu (the bar or a context menu) since the last
/// call.
#[must_use]
pub fn chosen() -> Vec<Action> {
    let mut out = Vec::new();
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        if let Some(action) = action_of(event.id().as_ref()) {
            out.push(action);
        }
    }
    out
}

pub use crate::contextmenu::{ContextState, context_items};

/// Build the context menu for `state`. A new one is built for each right click,
/// so what is turned off is always what is true now.
///
/// # Errors
///
/// Returns why a menu could not be built.
pub fn context_menu(state: ContextState) -> Result<Menu, String> {
    let menu = Menu::new();
    for entry in context_items(state) {
        match entry {
            Some((action, text, enabled)) => menu
                .append(&MenuItem::with_id(id_of(action), text, enabled, None))
                .map_err(|e| e.to_string())?,
            None => menu
                .append(&muda::PredefinedMenuItem::separator())
                .map_err(|e| e.to_string())?,
        }
    }
    Ok(menu)
}
