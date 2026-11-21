//! Input methods, dead keys and compose, from the window event to the program's
//! input. A real program on a real PTY reads what is sent; no window is opened.

#![cfg(all(unix, feature = "window"))]

use lumen_term::actions::Action;
use lumen_term::app::{cells_for, window_for};
use lumen_term::clipboard::Clipboard;
use lumen_term::host::{
    ImeFollower, ImeRect, ImeSink, Input, apply_window_event, ime_event, map_key_for,
    perform_action, send_input,
};
use lumen_term::raster::GUTTER_PAD_PX;
use lumen_term::{App, Block, Key, Mods, Passthrough};
use std::time::{Duration, Instant};
use winit::dpi::PhysicalSize;
use winit::event::{Ime, WindowEvent};
use winit::keyboard::{Key as W, ModifiersState, NamedKey};

fn text(app: &App) -> String {
    app.session()
        .document()
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Grid(g) => Some(g.text_rows().join("\n")),
            Block::Rich(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn pump_until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        app.pump();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A program that prints each line it reads as hex bytes, so what reached it is
/// checked exactly, not as the terminal drew it.
fn start_hex() -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &[
            "-c",
            "read x; printf '%s' \"$x\" | od -An -tx1 | tr -d ' \\n' | sed 's/^/hex:/'; echo",
        ],
        1,
        640,
        380,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
}

fn enter(app: &mut App) {
    assert!(app.key(Key::Enter, Mods::default()).is_ok());
}

#[test]
fn composition_is_shown_but_not_sent_until_it_is_committed() {
    let mut app = start_hex();
    assert!(!ime_event(&mut app, &Ime::Enabled));
    // Pinyin typed into the input method: preedit only.
    assert!(ime_event(
        &mut app,
        &Ime::Preedit("ni".into(), Some((2, 2)))
    ));
    assert_eq!(app.preedit(), Some(("ni", Some((2, 2)))));
    assert!(ime_event(
        &mut app,
        &Ime::Preedit("你".into(), Some((3, 3)))
    ));
    // Only what is committed later reaches the program; the preedit never does.
    assert!(ime_event(&mut app, &Ime::Commit("x".into())));
    enter(&mut app);
    assert!(
        pump_until(&mut app, |a| text(a).contains("hex:78")),
        "{}",
        text(&app)
    );
    assert!(
        !text(&app).contains("e4bda0"),
        "preedit must not reach the program"
    );
}

#[test]
fn the_committed_text_is_sent_once_and_ends_the_composition() {
    let mut app = start_hex();
    ime_event(&mut app, &Ime::Preedit("ni".into(), None));
    assert!(ime_event(&mut app, &Ime::Commit("你好".into())));
    assert_eq!(app.preedit(), None, "commit ends the composition");
    // The input method sends an empty preedit after a commit; it changes nothing.
    assert!(!ime_event(&mut app, &Ime::Preedit(String::new(), None)));
    enter(&mut app);
    assert!(pump_until(&mut app, |a| text(a).contains("hex:e4bda0e5a5bd")));
}

#[test]
fn turning_the_input_method_off_drops_the_unfinished_text() {
    let mut app = start_hex();
    ime_event(&mut app, &Ime::Preedit("han".into(), None));
    assert!(ime_event(&mut app, &Ime::Disabled));
    assert_eq!(app.preedit(), None);
    app.text("ok").ok();
    enter(&mut app);
    assert!(
        pump_until(&mut app, |a| text(a).contains("hex:6f6b")),
        "{}",
        text(&app)
    );
}

#[test]
fn a_committed_string_cannot_carry_an_escape_sequence() {
    let mut app = start_hex();
    assert!(app.commit("a\u{1b}[31mb\r").is_ok());
    enter(&mut app);
    assert!(
        pump_until(&mut app, |a| text(a).contains("hex:615b33316d62")),
        "{}",
        text(&app)
    );
}

/// Where the candidate window must sit for `app`: the first editor row, at the
/// text column, which starts a gutter padding right of the gutter's cells and
/// the box's prompt mark and the cell after it (three cells).
fn expected_anchor(app: &App, cell_w: u32) -> (u32, u32) {
    let cell_h = app.cell_height();
    let pad = GUTTER_PAD_PX * app.scale();
    // An empty gutter is drawn two cells wide; the layout counts four.
    let (columns, rows) = cells_for(640 - pad + 2 * cell_w, 380, (cell_w, cell_h));
    (
        2 * cell_w + pad + 3 * cell_w,
        window_for(columns, rows).editor_row * cell_h,
    )
}

#[test]
fn the_candidate_window_is_placed_at_the_editor_text_column_and_covers_the_preedit() {
    let mut app = start_hex();
    let (x0, y0, cell_w, h) = app.ime_area();
    assert_eq!((x0, y0), expected_anchor(&app, cell_w));
    assert_eq!(h, app.cell_height());
    app.set_preedit("你好", None);
    let (x1, y1, w1, _) = app.ime_area();
    assert_eq!((x0, y0), (x1, y1));
    assert_eq!(w1, cell_w * 4, "two wide characters are four cells");
    // The padding scales with the render scale, so the anchor must follow it.
    assert!(app.set_scale(2).is_ok());
    let (x2, y2, w2, _) = app.ime_area();
    assert_eq!((x2, y2), expected_anchor(&app, w2 / 4));
    assert!(
        x2 > x1,
        "bigger cells and padding move the text column right"
    );
}

/// What the system's input method has been told: every rectangle the host gave it.
#[derive(Default)]
struct Told(Vec<ImeRect>);

impl ImeSink for Told {
    fn set_area(&mut self, area: ImeRect) {
        self.0.push(area);
    }
}

struct NoClipboard;

impl Clipboard for NoClipboard {
    fn text(&self) -> Option<String> {
        None
    }
}

#[test]
fn the_system_is_told_the_candidate_rectangle_again_when_events_move_it() {
    let mut app = start_hex();
    let (mut ime, mut told) = (ImeFollower::default(), Told::default());
    let mut event = |app: &mut App, told: &mut Told, e: WindowEvent| {
        apply_window_event(app, &e, &mut ime, told)
    };
    event(&mut app, &mut told, WindowEvent::Focused(true));
    assert_eq!(told.0, vec![app.ime_area()], "the first rectangle is given");
    let first = app.ime_area();

    let size = |w, h| WindowEvent::Resized(PhysicalSize::new(w, h));
    event(&mut app, &mut told, size(800, 500));
    assert_eq!(told.0.len(), 2, "a resize moves the editor row");
    assert_eq!(told.0[1], app.ime_area());
    assert_ne!(told.0[1], first);
    event(&mut app, &mut told, size(800, 500));
    assert_eq!(told.0.len(), 2, "the same size again moves nothing");

    event(
        &mut app,
        &mut told,
        WindowEvent::Ime(Ime::Preedit("あ".into(), None)),
    );
    assert_eq!(told.0.len(), 3, "the composing text widens the area");
    assert_eq!(told.0[2], app.ime_area());
    event(&mut app, &mut told, WindowEvent::Focused(false));
    assert_eq!(told.0.len(), 4, "losing focus drops the composition");
    assert_eq!(told.0[3].2, told.0[1].2, "back to one cell wide");
}

#[test]
fn a_zoom_tells_the_system_where_the_candidate_window_moved() {
    let mut app = start_hex();
    let (mut ime, mut told) = (ImeFollower::default(), Told::default());
    apply_window_event(&mut app, &WindowEvent::Focused(true), &mut ime, &mut told);
    let before = app.scale();
    perform_action(&mut app, Action::ZoomIn, &NoClipboard, &mut ime, &mut told);
    assert!(app.scale() > before, "the zoom changed the scale");
    assert_eq!(told.0.len(), 2);
    assert_eq!(told.0[1], app.ime_area());
    assert!(
        told.0[1].0 > told.0[0].0,
        "a bigger gutter and padding move the text column right"
    );
}

#[test]
fn a_preedit_is_capped_and_its_caret_must_sit_on_a_character() {
    let mut app = start_hex();
    app.set_preedit(&"あ".repeat(10_000), Some((0, 3)));
    let (kept, _) = app.preedit().unwrap_or_else(|| unreachable!("a preedit"));
    assert_eq!(kept.chars().count(), lumen_term::app::PREEDIT_MAX_CHARS);
    app.set_preedit("あ", Some((1, 1)));
    assert_eq!(
        app.preedit(),
        Some(("あ", None)),
        "byte 1 is inside a character"
    );
}

#[test]
fn a_dead_key_sends_nothing_and_the_composed_letter_arrives_as_the_letter() {
    let none = ModifiersState::empty();
    // The dead key itself (acute accent) produces no text and no key.
    assert_eq!(map_key_for(&W::Dead(Some('´')), None, none, false), None);
    // The next key arrives as the composed letter.
    let (key, mods) = map_key_for(&W::Character("é".into()), Some("é"), none, false)
        .unwrap_or_else(|| unreachable!("a composed letter is a key"));
    assert_eq!((key, mods), (Key::Char('é'), Mods::default()));
    let mut app = start_hex();
    assert!(!send_input(&mut app, Input::Key(key, mods)));
    enter(&mut app);
    assert!(
        pump_until(&mut app, |a| text(a).contains("hex:c3a9")),
        "{}",
        text(&app)
    );
}

#[test]
fn option_composes_on_a_mac_and_is_an_escape_prefix_elsewhere() {
    let option = ModifiersState::ALT;
    // Option+e then e gives an accented letter on macOS: no Escape prefix.
    let mac = map_key_for(&W::Character("é".into()), Some("é"), option, true)
        .unwrap_or_else(|| unreachable!("a key"));
    assert_eq!(mac, (Key::Char('é'), Mods::default()));
    // Elsewhere Alt+letter is Meta, as before.
    let other = map_key_for(&W::Character("f".into()), Some("f"), option, false)
        .unwrap_or_else(|| unreachable!("a key"));
    assert!(other.1.alt);
    // Option with a named key is still a modified key on macOS.
    let word = map_key_for(&W::Named(NamedKey::ArrowLeft), None, option, true)
        .unwrap_or_else(|| unreachable!("a key"));
    assert!(word.1.alt);
}

#[test]
fn a_compose_sequence_arrives_as_text_and_is_sent_whole() {
    // A compose key sequence can produce a string of more than one character.
    let none = ModifiersState::empty();
    assert_eq!(
        map_key_for(
            &W::Character("a".into()),
            Some("\u{e6}\u{301}"),
            none,
            false
        ),
        None
    );
    let mut app = start_hex();
    assert!(!send_input(&mut app, Input::Text("\u{e6}\u{301}")));
    enter(&mut app);
    assert!(
        pump_until(&mut app, |a| text(a).contains("hex:c3a6cc81")),
        "{}",
        text(&app)
    );
}
