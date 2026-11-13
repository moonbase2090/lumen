//! The input modes a program sets, followed from its output, and the keys they
//! change, with a real program on a real PTY.

use lumen_term::{KeyModes, ModeTracker};

fn after(chunks: &[&[u8]]) -> KeyModes {
    let mut t = ModeTracker::new();
    for c in chunks {
        t.feed(c);
    }
    t.modes()
}

const CURSOR: KeyModes = KeyModes {
    cursor_app: true,
    keypad_app: false,
};
const KEYPAD: KeyModes = KeyModes {
    cursor_app: false,
    keypad_app: true,
};

#[test]
fn set_and_clear_each_mode() {
    assert_eq!(after(&[b"\x1b[?1h"]), CURSOR);
    assert_eq!(after(&[b"\x1b[?1h\x1b[?1l"]), KeyModes::default());
    assert_eq!(after(&[b"\x1b="]), KEYPAD);
    assert_eq!(after(&[b"\x1b=\x1b>"]), KeyModes::default());
    // Several modes in one sequence.
    assert_eq!(after(&[b"\x1b[?25;1h"]), CURSOR);
}

#[test]
fn sequences_split_across_reads() {
    assert_eq!(after(&[b"text\x1b", b"[?", b"1", b"h"]), CURSOR);
    assert_eq!(after(&[b"\x1b", b"="]), KEYPAD);
}

#[test]
fn other_modes_and_look_alikes_change_nothing() {
    // 1049 (alternate screen), 11 and 12 are not DECCKM.
    assert_eq!(
        after(&[b"\x1b[?1049h\x1b[?11h\x1b[?12h"]),
        KeyModes::default()
    );
    // The same final bytes without the private marker are other sequences.
    assert_eq!(after(&[b"\x1b[1h\x1b[=1h"]), KeyModes::default());
    // `=` and `>` as text, or inside a CSI or OSC, are not DECKPAM or DECKPNM.
    assert_eq!(after(&[b"a=b>c"]), KeyModes::default());
    assert_eq!(after(&[b"\x1b[>c\x1b[=1u"]), KeyModes::default());
    assert_eq!(
        after(&[b"\x1b]0;\x1b=title\x07"]),
        KEYPAD,
        "ESC ends the string and is read"
    );
    assert_eq!(after(&[b"\x1b]0;x=y>z\x07"]), KeyModes::default());
    // Entering and leaving the alternate screen keeps both modes.
    assert_eq!(
        after(&[b"\x1b[?1h\x1b=\x1b[?1049h\x1b[?1049l"]),
        KeyModes {
            cursor_app: true,
            keypad_app: true
        }
    );
}

#[test]
fn soft_and_full_reset_return_to_normal() {
    let both = b"\x1b[?1h\x1b=";
    assert_eq!(after(&[both, b"\x1b[!p"]), KeyModes::default());
    assert_eq!(after(&[both, b"\x1bc"]), KeyModes::default());
}

#[cfg(unix)]
mod on_a_pty {
    use lumen_term::{App, Block, Key, Mods, Passthrough};
    use std::time::{Duration, Instant};

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

    fn wait_for(app: &mut App, want: &str) {
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end {
            app.pump();
            if text(app).contains(want) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        unreachable!("never saw {want:?}; screen was:\n{}", text(app));
    }

    /// A program sets both modes, reads the bytes the keys send, clears the
    /// modes, and reads them again. Raw bytes are shown in hex.
    const PROGRAM: &str = r#"stty -echo -icanon -icrnl min 1
printf '\033[?1h\033=READY1\r\n'
printf 'APP %s\r\n' "$(dd bs=1 count=12 2>/dev/null | od -An -tx1 | tr -d ' \n')"
printf '\033[?1l\033>READY2\r\n'
printf 'NORMAL %s\r\n' "$(dd bs=1 count=5 2>/dev/null | od -An -tx1 | tr -d ' \n')"
"#;

    #[test]
    fn keys_follow_the_modes_the_program_sets() {
        let mut app = App::start(
            Box::new(Passthrough),
            "/bin/sh",
            &["-c", PROGRAM],
            1,
            800,
            400,
        )
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
        let none = Mods::default();
        wait_for(&mut app, "READY1");
        assert!(app.session().document().key_modes().cursor_app);
        assert!(app.session().document().key_modes().keypad_app);
        for key in [Key::Up, Key::Home, Key::Keypad('5'), Key::KeypadEnter] {
            assert!(app.key(key, none).is_ok());
        }
        wait_for(&mut app, "APP 1b4f411b4f481b4f751b4f4d");

        wait_for(&mut app, "READY2");
        assert_eq!(
            app.session().document().key_modes(),
            lumen_term::KeyModes::default()
        );
        for key in [Key::Up, Key::Keypad('5'), Key::KeypadEnter] {
            assert!(app.key(key, none).is_ok());
        }
        wait_for(&mut app, "NORMAL 1b5b41350d");
    }
}
