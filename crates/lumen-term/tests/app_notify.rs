//! The window's notifications: the notifier, the clock and the sink as `App`
//! drives them, with a real program behind a real PTY.

#![cfg(unix)]
// Short names for frames, rectangles and cells keep the comparisons readable.
#![allow(clippy::many_single_char_names)]

use lumen_term::app::{PANE_LABEL, cells_for, place_of, window_for};
use lumen_term::notify::{OsNotification, OsSink, Place, Settings};
use lumen_term::{App, Event, Frame, Passthrough, Rect, Renderer, layout, notification_area};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SIZE: (u32, u32) = (640, 444);

/// Keeps what it was given, where the test can read it.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<OsNotification>>>);

impl OsSink for Shared {
    fn send(&mut self, n: &OsNotification) {
        if let Ok(mut v) = self.0.lock() {
            v.push(n.clone());
        }
    }
}

impl Shared {
    fn count(&self) -> usize {
        self.0.lock().map_or(0, |v| v.len())
    }
}

fn start(script: &str) -> App {
    App::start(
        Box::new(Passthrough),
        "/bin/sh",
        &["-c", script],
        1,
        SIZE.0,
        SIZE.1,
    )
    .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
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

fn unsigned_on() -> Settings {
    Settings {
        allow_unsigned: true,
        ..Settings::default()
    }
}

/// The OSC 9 text a program prints, then it waits so the window is still there.
const NOTIFY: &str = "printf '\\033]9;hello\\007'; sleep 30";

fn area() -> (Renderer, Rect, lumen_term::Layout) {
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("face loads: {e}"));
    let (c, rows) = cells_for(SIZE.0, SIZE.1, r.cell());
    let w = window_for(c, rows);
    let l = layout(w.cols, w.rows, w.editor_row, w.editor_rows, None);
    let a = notification_area(&l);
    (r, a, l)
}

fn changed(r: &Renderer, a: &Frame, b: &Frame, rect: &Rect) -> usize {
    let (cw, ch) = r.cell();
    let mut n = 0;
    for y in rect.y * ch..(rect.y + rect.h) * ch {
        for x in rect.x * cw..(rect.x + rect.w) * cw {
            if a.pixel(x, y) != b.pixel(x, y) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn by_default_a_program_that_asks_for_a_notification_shows_nothing() {
    let mut app = start(NOTIFY);
    assert!(pump_until(&mut app, |a| a.notifier().dropped() > 0));
    assert_eq!(app.notifier().shown().len(), 0, "unsigned toasts are off");
    // Even with the place stated, the setting still decides.
    let mut app = start(NOTIFY);
    app.configure_notifications(
        Settings::default(),
        Place::Local,
        Box::new(Shared::default()),
    );
    assert!(pump_until(&mut app, |a| a.notifier().dropped() > 0));
    assert_eq!(app.notifier().shown().len(), 0);
}

#[test]
fn with_the_setting_and_a_local_layer_an_unsigned_toast_is_drawn_in_the_area_only() {
    let (r, a, l) = area();
    let mut app = start(NOTIFY);
    app.configure_notifications(unsigned_on(), Place::Local, Box::new(Shared::default()));
    let before = app.view();
    assert!(pump_until(&mut app, |x| x.notifier().shown().len() == 1));
    let toast = &app.notifier().shown()[0];
    assert!(toast.unsigned, "unsigned output is labelled as such");
    assert!(toast.text.contains(PANE_LABEL), "{}", toast.text);
    let after = app.view();
    assert!(changed(&r, &before, &after, &a) > 0, "the toast is drawn");
    for m in &l.margin {
        assert_eq!(changed(&r, &before, &after, m), 0, "margin untouched");
    }
    assert_eq!(
        changed(&r, &before, &after, &l.editor),
        0,
        "editor untouched"
    );
}

#[test]
fn an_unknown_remote_or_multiplexed_place_draws_no_unsigned_toast() {
    for place in [
        Place::Unknown,
        Place::Ssh,
        Place::MultiplexerPane,
        Place::PossiblyRemote,
    ] {
        let mut app = start(NOTIFY);
        app.configure_notifications(unsigned_on(), place, Box::new(Shared::default()));
        assert!(
            pump_until(&mut app, |x| x.notifier().dropped() > 0),
            "{place:?}"
        );
        assert_eq!(app.notifier().shown().len(), 0, "{place:?}");
    }
}

#[test]
fn a_toast_goes_away_when_its_time_is_up() {
    let mut app = start(NOTIFY);
    app.configure_notifications(unsigned_on(), Place::Local, Box::new(Shared::default()));
    assert!(pump_until(&mut app, |x| x.notifier().shown().len() == 1));
    assert!(!app.tick(), "nothing expires at once");
    let end = Instant::now() + Duration::from_secs(20);
    let mut gone = false;
    while Instant::now() < end && !gone {
        std::thread::sleep(Duration::from_millis(200));
        gone = app.tick();
    }
    assert!(
        gone,
        "tick reports the toast removed, so the window draws again"
    );
    assert_eq!(app.notifier().shown().len(), 0);
}

fn string(text: &str) -> Vec<u8> {
    let mut v = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    v.extend_from_slice(text.as_bytes());
    v
}

fn grant() -> Event {
    Event::Sealed {
        kind: 0x20,
        body: vec![1, 8],
    }
}

fn notification(text: &str) -> Event {
    Event::Sealed {
        kind: 0x22,
        body: string(text),
    }
}

#[test]
fn the_sink_is_called_only_when_the_setting_is_on_and_the_window_is_unfocused() {
    let on = Settings {
        os_notifications: true,
        os_text: true,
        ..Settings::default()
    };
    // Setting off: a granted sealed notification is a toast, never an OS one.
    let sink = Shared::default();
    let mut app = start("sleep 30");
    app.configure_notifications(Settings::default(), Place::Local, Box::new(sink.clone()));
    app.set_focused(false);
    app.route(&[grant(), notification("done")]);
    assert_eq!(sink.count(), 0, "off by default");
    assert_eq!(app.notifier().shown().len(), 1, "it fell back to a toast");

    // Setting on, window focused: still a toast.
    let sink = Shared::default();
    let mut app = start("sleep 30");
    app.configure_notifications(on, Place::Local, Box::new(sink.clone()));
    app.route(&[grant(), notification("done")]);
    assert_eq!(sink.count(), 0, "a focused window shows a toast");

    // Setting on, window unfocused, local: one OS notification with the text.
    let sink = Shared::default();
    let mut app = start("sleep 30");
    app.configure_notifications(on, Place::Local, Box::new(sink.clone()));
    app.set_focused(false);
    app.route(&[grant(), notification("done <now>")]);
    assert_eq!(sink.count(), 1);
    let sent = sink.0.lock().map(|v| v[0].text.clone()).unwrap_or_default();
    assert!(sent.starts_with("Program in pane 1 says:"), "{sent}");
    assert!(
        sent.contains("done &lt;now&gt;"),
        "escaped for the API: {sent}"
    );
    assert_eq!(app.notifier().shown().len(), 0, "no toast beside it");

    // Regaining the focus puts the next one back to a toast.
    app.set_focused(true);
    app.route(&[notification("later")]);
    assert_eq!(sink.count(), 1);

    // Not local: never an OS notification.
    let sink = Shared::default();
    let mut app = start("sleep 30");
    app.configure_notifications(on, Place::Unknown, Box::new(sink.clone()));
    app.set_focused(false);
    app.route(&[grant(), notification("done")]);
    assert_eq!(sink.count(), 0, "an unknown place is not local");
}

#[test]
fn the_place_is_never_local_whatever_the_environment_holds() {
    let vars = [
        "SSH_CONNECTION",
        "SSH_TTY",
        "SSH_CLIENT",
        "TMUX",
        "STY",
        "LUMEN_LOCAL",
    ];
    assert_eq!(place_of(|_| false), Place::Unknown);
    for mask in 0..(1u32 << vars.len()) {
        let set = |name: &str| {
            vars.iter()
                .position(|v| *v == name)
                .is_some_and(|i| mask & (1 << i) != 0)
        };
        assert_ne!(place_of(set), Place::Local, "mask {mask:b}");
    }
    for var in ["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT"] {
        assert_eq!(place_of(|n| n == var), Place::Ssh, "{var}");
    }
    for var in ["TMUX", "STY"] {
        assert_eq!(place_of(|n| n == var), Place::MultiplexerPane, "{var}");
    }
}

#[test]
fn a_session_started_with_the_settings_on_is_not_local_so_it_shows_no_unsigned_toast() {
    let mut app = start(NOTIFY);
    app.use_settings(Settings {
        allow_unsigned: true,
        os_notifications: true,
        os_text: true,
    });
    assert!(pump_until(&mut app, |x| x.notifier().dropped() > 0));
    assert_eq!(
        app.notifier().shown().len(),
        0,
        "no evidence of a local layer, no toast"
    );
}
