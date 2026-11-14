//! The window's own actions, the same whether they come from a menu item or a
//! shortcut. Nothing here needs a window: the table says which key chord asks
//! for which action, and [`crate::App::perform`] does it.
//!
//! On macOS the chords are the usual Command ones and the menu bar carries the
//! same items. Elsewhere there is no menu and the chords are Control+Shift, so
//! a program never loses a key it uses (Control+V, Control+W, Control+C and the
//! like go to the program as always).

/// Something the window does for the user, apart from typing into the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Paste the system clipboard's text into the program.
    Paste,
    /// Draw the text larger.
    ZoomIn,
    /// Draw the text smaller.
    ZoomOut,
    /// Draw the text at the display's own size.
    ZoomReset,
    /// Close the window and end the session.
    Close,
    /// Quit the application.
    Quit,
}

/// Which family of chords applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Command chords, with a menu bar.
    Mac,
    /// Control+Shift chords, no menu.
    Other,
}

impl Platform {
    /// The platform this build runs on.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Other
        }
    }
}

/// The modifier keys held with a chord.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a chord is four independent keys"
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Held {
    /// Command (the Super key).
    pub command: bool,
    /// Control.
    pub control: bool,
    /// Option or Alt.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
}

/// The action a chord asks for, if any. `key` is the character the key makes
/// without shift applied for a letter, and as typed for a symbol: `=` and `+`
/// both ask for zoom in, because `+` is what Shift+`=` types.
#[must_use]
pub fn action_for(platform: Platform, key: char, held: Held) -> Option<Action> {
    let key = key.to_ascii_lowercase();
    match platform {
        Platform::Mac => {
            if !held.command || held.control || held.alt {
                return None;
            }
            match (key, held.shift) {
                ('v', false) => Some(Action::Paste),
                ('=' | '+', _) => Some(Action::ZoomIn),
                ('-', false) => Some(Action::ZoomOut),
                ('0', false) => Some(Action::ZoomReset),
                ('w', false) => Some(Action::Close),
                ('q', false) => Some(Action::Quit),
                _ => None,
            }
        }
        Platform::Other => {
            if !(held.control && held.shift) || held.command || held.alt {
                return None;
            }
            match key {
                'v' => Some(Action::Paste),
                '=' | '+' => Some(Action::ZoomIn),
                '-' | '_' => Some(Action::ZoomOut),
                '0' | ')' => Some(Action::ZoomReset),
                _ => None,
            }
        }
    }
}

/// The text size steps: a whole render scale, 1 to 4.
pub const MIN_SCALE: u32 = 1;
/// The largest render scale.
pub const MAX_SCALE: u32 = 4;

/// The scale after zooming one step in the direction `action` names, from
/// `current`, with `base` the display's own scale. `None` when `action` is not
/// a zoom or the scale would not change.
#[must_use]
pub fn zoomed(action: Action, current: u32, base: u32) -> Option<u32> {
    let next = match action {
        Action::ZoomIn => current.saturating_add(1).min(MAX_SCALE),
        Action::ZoomOut => current.saturating_sub(1).max(MIN_SCALE),
        Action::ZoomReset => base.clamp(MIN_SCALE, MAX_SCALE),
        _ => return None,
    };
    (next != current).then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: Held = Held {
        command: true,
        control: false,
        alt: false,
        shift: false,
    };
    const CTRL_SHIFT: Held = Held {
        command: false,
        control: true,
        alt: false,
        shift: true,
    };

    #[test]
    fn mac_chords_are_command_and_no_other_modifier() {
        assert_eq!(action_for(Platform::Mac, 'v', CMD), Some(Action::Paste));
        assert_eq!(action_for(Platform::Mac, 'q', CMD), Some(Action::Quit));
        assert_eq!(action_for(Platform::Mac, 'w', CMD), Some(Action::Close));
        assert_eq!(action_for(Platform::Mac, '=', CMD), Some(Action::ZoomIn));
        assert_eq!(action_for(Platform::Mac, '-', CMD), Some(Action::ZoomOut));
        assert_eq!(action_for(Platform::Mac, '0', CMD), Some(Action::ZoomReset));
        let with_control = Held {
            control: true,
            ..CMD
        };
        assert_eq!(action_for(Platform::Mac, 'v', with_control), None);
        assert_eq!(action_for(Platform::Mac, 'v', Held::default()), None);
    }

    #[test]
    fn other_platforms_never_take_a_chord_a_program_uses() {
        let ctrl = Held {
            control: true,
            ..Held::default()
        };
        for key in ['v', 'w', 'c', 'q', '=', '-', '0'] {
            assert_eq!(action_for(Platform::Other, key, ctrl), None, "{key}");
        }
        assert_eq!(
            action_for(Platform::Other, 'v', CTRL_SHIFT),
            Some(Action::Paste)
        );
        assert_eq!(action_for(Platform::Other, 'q', CTRL_SHIFT), None);
        assert_eq!(action_for(Platform::Other, 'w', CTRL_SHIFT), None);
    }

    #[test]
    fn zoom_steps_stay_in_range_and_reset_goes_to_the_display_scale() {
        assert_eq!(zoomed(Action::ZoomIn, 2, 2), Some(3));
        assert_eq!(zoomed(Action::ZoomIn, MAX_SCALE, 2), None);
        assert_eq!(zoomed(Action::ZoomOut, MIN_SCALE, 2), None);
        assert_eq!(zoomed(Action::ZoomOut, 3, 2), Some(2));
        assert_eq!(zoomed(Action::ZoomReset, 4, 2), Some(2));
        assert_eq!(zoomed(Action::ZoomReset, 2, 2), None);
        assert_eq!(zoomed(Action::Paste, 2, 2), None);
    }
}
