//! The macOS menu bar. Each item that does something the terminal itself does
//! carries an [`Action`] and the usual Command chord; the items the system
//! performs for every application (hide, hide others, show all, minimise, zoom
//! the window, full screen, about) are the system's own. There is no menu on
//! other platforms.

use crate::actions::Action;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

use crate::APP_NAME;

/// The menu id of an action's item.
#[must_use]
pub const fn id_of(action: Action) -> &'static str {
    match action {
        Action::Paste => "paste",
        Action::ZoomIn => "zoom-in",
        Action::ZoomOut => "zoom-out",
        Action::ZoomReset => "zoom-reset",
        Action::Close => "close",
        Action::Quit => "quit",
    }
}

/// The action an item id stands for.
#[must_use]
pub fn action_of(id: &str) -> Option<Action> {
    [
        Action::Paste,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ZoomReset,
        Action::Close,
        Action::Quit,
    ]
    .into_iter()
    .find(|a| id_of(*a) == id)
}

fn item(action: Action, text: &str, code: Code) -> MenuItem {
    MenuItem::with_id(
        id_of(action),
        text,
        true,
        Some(Accelerator::new(Modifiers::META, code)),
    )
}

/// The menu bar, kept alive for as long as the window runs.
pub struct MenuBar {
    _menu: Menu,
}

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
        let edit = Submenu::with_items("Edit", true, &[&item(Action::Paste, "Paste", Code::KeyV)])
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
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::maximize(None),
                &PredefinedMenuItem::separator(),
                &item(Action::Close, "Close Window", Code::KeyW),
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
        let mut out = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = action_of(event.id().as_ref()) {
                out.push(action);
            }
        }
        out
    }
}
