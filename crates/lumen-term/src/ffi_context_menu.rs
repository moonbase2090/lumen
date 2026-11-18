//! The one place the terminal calls the platform's own code to show a context
//! menu in its window (macOS and Windows; Linux has no context menu yet).
//!
//! The menu library's call is `unsafe` because it takes a raw window pointer
//! and cannot check it. This module is the only unsafe code in the crate and
//! does one thing: it takes the pointer from the window's own handle, which the
//! window system guarantees is valid for as long as the handle is borrowed, and
//! passes it, with the menu, to that call, on the thread that owns the window
//! (the event loop's). It reads nothing from the program's output and takes no
//! pointer from anywhere else.

use muda::{ContextMenu, Menu};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Show `menu` at the pointer in `window` and return when it closes. Returns
/// whether an item was chosen (the choice itself arrives as a menu event), and
/// `false` if the window has no usable handle or the menu was dismissed.
///
/// Must be called on the thread that owns the window.
pub fn show(menu: &Menu, window: &impl HasWindowHandle) -> bool {
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    match handle.as_raw() {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(h) => {
            // SAFETY: `h.ns_view` is the window's own NSView, valid while
            // `handle` borrows the window, and this runs on the main thread.
            unsafe { menu.show_context_menu_for_nsview(h.ns_view.as_ptr().cast_const(), None) }
        }
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => {
            // SAFETY: `h.hwnd` is the window's own HWND, valid while `handle`
            // borrows the window, and this runs on the window's thread.
            unsafe { menu.show_context_menu_for_hwnd(h.hwnd.get(), None) }
        }
        _ => false,
    }
}
