//! Colour query replies: the parser, the fixed reply, the limits, and a real
//! program on a PTY that asks and reads what the terminal writes back.

use lumen_term::reply::{
    ColourQuery, CursorQueryEnds, MAX_PER_READ, MAX_PER_WINDOW, Replies, WINDOW_MS, parse_query,
};
use lumen_term::{Context, Document, Layer, Passthrough, Session, Theme};
#[cfg(unix)]
use std::time::{Duration, Instant};

/// The cursor-position reports the grids of a document hold, as the session
/// takes them.
fn cursor_reports(d: &Document) -> Vec<(u32, u32)> {
    d.blocks()
        .iter()
        .filter_map(|b| match b {
            lumen_term::Block::Grid(g) => Some(g.take_cursor_reports()),
            lumen_term::Block::Rich(_) => None,
        })
        .flatten()
        .collect()
}

fn light() -> Replies {
    Replies::new(Theme::light())
}

fn answer(queries: &[ColourQuery], now_ms: u64) -> Vec<u8> {
    light().answer(queries, now_ms)
}

#[test]
fn only_the_single_question_mark_form_is_a_query() {
    for (body, want) in [
        (&b"10;?"[..], Some(ColourQuery::Special(10))),
        (b"11;?", Some(ColourQuery::Special(11))),
        (b"12;?", Some(ColourQuery::Special(12))),
        (b"17;?", Some(ColourQuery::Special(17))),
        (b"19;?", Some(ColourQuery::Special(19))),
        (b"4;0;?", Some(ColourQuery::Indexed(0))),
        (b"4;255;?", Some(ColourQuery::Indexed(255))),
        // Setting a colour is not a query.
        (b"10;#ff0000", None),
        (b"11;rgb:00/00/00", None),
        (b"4;1;#ff0000", None),
        // More than one question in a sequence gets no reply at all.
        (b"4;1;?;2;?", None),
        (b"10;?;?", None),
        (b"10;?;11;?", None),
        // Numbers and forms the terminal does not answer.
        (b"13;?", None),
        (b"18;?", None),
        (b"110;?", None),
        (b"104;1", None),
        (b"0;?", None),
        (b"52;c;?", None),
        (b"4;256;?", None),
        (b"4;-1;?", None),
        (b"4;;?", None),
        (b"4;01;?", None),
        (b"4;1000;?", None),
        (b"010;?", None),
        (b"10;??", None),
        (b"10;? ", None),
        (b"10;", None),
        (b"10", None),
        (b"4;1", None),
        (b"", None),
        (b";", None),
    ] {
        assert_eq!(
            parse_query(body),
            want,
            "{:?}",
            String::from_utf8_lossy(body)
        );
    }
}

#[test]
fn a_reply_is_the_fixed_form_with_the_themes_colour_in_sixteen_bits() {
    // The light theme: foreground [28, 28, 28], background white, ANSI 1
    // [194, 54, 33]. A channel is its byte times 257.
    assert_eq!(
        answer(&[ColourQuery::Special(10)], 0),
        b"\x1b]10;rgb:1c1c/1c1c/1c1c\x1b\\"
    );
    assert_eq!(
        answer(&[ColourQuery::Special(11)], 0),
        b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\"
    );
    assert_eq!(
        answer(&[ColourQuery::Indexed(1)], 0),
        b"\x1b]4;1;rgb:c2c2/3636/2121\x1b\\"
    );
    // The cube and the grey ramp, as the renderer draws them.
    assert_eq!(
        answer(&[ColourQuery::Indexed(16)], 0),
        b"\x1b]4;16;rgb:0000/0000/0000\x1b\\"
    );
    assert_eq!(
        answer(&[ColourQuery::Indexed(231)], 0),
        b"\x1b]4;231;rgb:ffff/ffff/ffff\x1b\\"
    );
    assert_eq!(
        answer(&[ColourQuery::Indexed(232)], 0),
        b"\x1b]4;232;rgb:0808/0808/0808\x1b\\"
    );
}

#[test]
fn the_theme_has_no_cursor_or_highlight_so_those_use_the_foreground_and_background() {
    let fg = answer(&[ColourQuery::Special(10)], 0);
    let bg = answer(&[ColourQuery::Special(11)], 0);
    let body = |reply: &[u8]| reply[reply.iter().position(|b| *b == b';').unwrap_or(0)..].to_vec();
    assert_eq!(body(&answer(&[ColourQuery::Special(12)], 0)), body(&fg));
    assert_eq!(body(&answer(&[ColourQuery::Special(17)], 0)), body(&fg));
    assert_eq!(body(&answer(&[ColourQuery::Special(19)], 0)), body(&bg));
}

#[test]
fn the_reply_follows_the_theme_the_terminal_is_set_to() {
    let mut dark = Theme::light();
    dark.background = [0, 0, 0];
    let mut replies = Replies::new(Theme::light());
    replies.set_theme(dark);
    assert_eq!(
        replies.answer(&[ColourQuery::Special(11)], 0),
        b"\x1b]11;rgb:0000/0000/0000\x1b\\"
    );
}

/// Every reply, for every query there is, is the fixed form and nothing else.
#[test]
fn every_possible_reply_has_the_fixed_shape_and_only_theme_digits() {
    let mut queries: Vec<ColourQuery> = [10, 11, 12, 17, 19]
        .into_iter()
        .map(ColourQuery::Special)
        .collect();
    queries.extend((0..=255).map(ColourQuery::Indexed));
    for q in queries {
        let reply = answer(&[q], 0);
        let text = String::from_utf8_lossy(&reply).into_owned();
        let head = match q {
            ColourQuery::Special(n) => format!("\x1b]{n};rgb:"),
            ColourQuery::Indexed(i) => format!("\x1b]4;{i};rgb:"),
        };
        let rest = text
            .strip_prefix(&head)
            .and_then(|r| r.strip_suffix("\x1b\\"))
            .unwrap_or_else(|| unreachable!("{q:?}: not the fixed form: {text:?}"));
        let channels: Vec<&str> = rest.split('/').collect();
        assert_eq!(channels.len(), 3, "{q:?}");
        for c in channels {
            assert!(
                c.len() == 4
                    && c.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{q:?}: {c:?}"
            );
        }
        assert!(
            !reply.iter().any(|b| matches!(b, b'\r' | b'\n')),
            "no CR or LF is ever sent"
        );
    }
}

#[test]
fn at_most_eight_colour_replies_for_one_read() {
    let queries = vec![ColourQuery::Special(10); 20];
    let reply = light().answer(&queries, 0);
    assert_eq!(reply.len(), MAX_PER_READ * 25);
}

#[test]
fn at_most_forty_replies_in_a_sliding_second() {
    let mut r = light();
    let queries = vec![ColourQuery::Special(10); 8];
    let mut sent = 0;
    // Six reads at the same instant: 48 asked, 40 answered.
    for _ in 0..6 {
        sent += r.answer(&queries, 100).len() / 25;
    }
    assert_eq!(sent, MAX_PER_WINDOW);
    // The window still holds them just before it has passed...
    assert_eq!(r.answer(&queries, 100 + WINDOW_MS - 1), b"");
    // ...and is empty once it has.
    assert_eq!(r.answer(&queries, 100 + WINDOW_MS).len(), 8 * 25);
}

#[test]
fn the_window_slides_it_does_not_reset_all_at_once() {
    let mut r = light();
    let one = [ColourQuery::Special(11)];
    // 20 at t=0, 20 at t=600: full.
    for _ in 0..20 {
        assert_eq!(r.answer(&one, 0).len(), 25);
    }
    for _ in 0..20 {
        assert_eq!(r.answer(&one, 600).len(), 25);
    }
    assert_eq!(r.answer(&one, 700), b"");
    // At t=1000 the first 20 have left the window and the second 20 have not.
    for _ in 0..20 {
        assert_eq!(r.answer(&one, 1000).len(), 25);
    }
    assert_eq!(r.answer(&one, 1000), b"");
}

fn at(now_ms: u64) -> Context {
    Context {
        now_ms,
        ..Context::default()
    }
}

fn queries_in(chunks: &[&[u8]]) -> Vec<ColourQuery> {
    let mut d = Document::new(40, 4);
    for chunk in chunks {
        d.receive(&mut Passthrough, chunk, at(0));
    }
    d.take_colour_queries()
}

#[test]
fn grid_output_makes_queries_with_either_terminator_and_split_across_reads() {
    assert_eq!(
        queries_in(&[b"\x1b]11;?\x07", b"\x1b]10;?\x1b\\", b"\x1b]4;5;?\x07"]),
        [
            ColourQuery::Special(11),
            ColourQuery::Special(10),
            ColourQuery::Indexed(5)
        ]
    );
    assert_eq!(
        queries_in(&[b"a\x1b", b"]1", b"1;", b"?", b"\x07b"]),
        [ColourQuery::Special(11)]
    );
}

#[test]
fn sequences_that_are_not_a_single_query_make_none() {
    let none: [&[u8]; 9] = [
        b"\x1b]10;#ff0000\x07",
        b"\x1b]4;1;?;2;?\x07",
        b"\x1b]52;c;?\x07",
        b"\x1b]99;?\x07",
        b"\x1b]9;?\x07",
        b"\x1b]777;notify;?\x07",
        b"\x1b]11;?\x1b[2J\x07",
        b"\x1bP11;?\x1b\\",
        b"\x9d11;?\x07",
    ];
    for bytes in none {
        assert_eq!(
            queries_in(&[bytes]),
            [],
            "{:?}",
            String::from_utf8_lossy(bytes)
        );
    }
}

#[test]
fn a_query_in_track_0_text_or_a_notification_is_not_a_query() {
    // The query bytes inside a Track 0 paragraph are text of a block, so they
    // never reach the isolator as a sequence.
    let text = "\x1b]11;?\x07";
    let mut body = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    body.extend_from_slice(text.as_bytes());
    let mut frame = b"\x1b_".to_vec();
    frame.extend_from_slice(&lumen_detector::TAG);
    frame.push(0x80);
    frame.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    frame.extend(body);
    let mut d = Document::new(40, 4);
    let mut l = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    Layer::open_container(&mut l);
    d.receive(&mut l, &frame, at(0));
    assert_eq!(d.take_colour_queries(), []);
}

#[test]
fn the_document_keeps_a_bounded_number_of_unanswered_queries() {
    let mut d = Document::new(40, 4);
    let flood = b"\x1b]11;?\x07".repeat(1000);
    d.receive(&mut Passthrough, &flood, at(0));
    assert_eq!(d.take_colour_queries().len(), 64);
    assert_eq!(d.take_colour_queries(), [], "taking empties the queue");
}

#[test]
fn a_cursor_position_query_makes_a_report_of_where_the_cursor_is_when_it_asks() {
    let mut d = Document::new(40, 6);
    // The cursor is at row 2, column 4 when the query arrives, and the program
    // goes on to move it before the read ends: the report is for the query.
    d.receive(&mut Passthrough, b"ab\r\n  x\x1b[6n\x1b[5;9Hmore", at(0));
    assert_eq!(cursor_reports(&d), [(2, 4)]);
    assert_eq!(cursor_reports(&d), [], "taking empties the queue");
}

#[test]
fn queries_other_than_cursor_position_make_no_report() {
    let mut d = Document::new(40, 6);
    // Device status, primary attributes and the size reports all have answers
    // in the emulator; the terminal sends none of them.
    d.receive(
        &mut Passthrough,
        b"\x1b[5n\x1b[c\x1b[14t\x1b[16t\x1b[18t",
        at(0),
    );
    assert_eq!(cursor_reports(&d), []);
}

#[test]
fn the_document_keeps_a_bounded_number_of_unanswered_cursor_queries() {
    let mut d = Document::new(40, 4);
    d.receive(&mut Passthrough, &b"\x1b[6n".repeat(1000), at(0));
    assert_eq!(cursor_reports(&d).len(), 64);
}

#[test]
fn a_cursor_report_is_the_fixed_form_and_shares_the_reply_limits() {
    let mut replies = light();
    let reports: Vec<(u32, u32)> = (1..=20).map(|n| (n, n + 1)).collect();
    let got = replies.answer_cursor(&reports, 0);
    let want: Vec<u8> = (1..=8_u32)
        .flat_map(|n| format!("\x1b[{n};{}R", n + 1).into_bytes())
        .collect();
    assert_eq!(MAX_PER_READ, 8);
    assert_eq!(got, want, "at most eight for one read");
    // Four more full reads fill the window (forty replies); a colour query in
    // it then gets no answer, and one after the window has slid does.
    for _ in 0..4 {
        assert_eq!(replies.answer_cursor(&reports, 10).len(), want.len());
    }
    assert_eq!(replies.answer_cursor(&reports, 20), []);
    assert_eq!(replies.answer(&[ColourQuery::Special(11)], 30), []);
    assert_eq!(
        replies
            .answer(&[ColourQuery::Special(11)], WINDOW_MS + 30)
            .first(),
        Some(&0x1b)
    );
}

#[test]
fn the_pieces_of_one_read_share_its_eight_answers() {
    let mut replies = light();
    let mut left = MAX_PER_READ;
    let cursor: Vec<(u32, u32)> = (1..=6).map(|n| (n, n)).collect();
    let colour = [ColourQuery::Special(11); 6];
    let first = replies.answer_cursor_within(&cursor, 0, &mut left);
    assert_eq!(first.len(), "\x1b[1;1R".len() * 6);
    assert_eq!(left, 2);
    let second = replies.answer_within(&colour, 0, &mut left);
    assert_eq!(second.windows(2).filter(|w| w == b"\x1b]").count(), 2);
    assert_eq!(left, 0);
    assert_eq!(replies.answer_cursor_within(&cursor, 0, &mut left), []);
    // The next read has its own eight.
    let mut next = MAX_PER_READ;
    assert_eq!(
        replies.answer_cursor_within(&cursor, 1, &mut next).len(),
        "\x1b[1;1R".len() * 6
    );
}

#[test]
fn the_end_of_a_cursor_query_is_found_across_reads_and_only_for_that_query() {
    let ends = |chunks: &[&[u8]]| {
        let mut scan = CursorQueryEnds::default();
        chunks
            .iter()
            .map(|c| c.iter().filter(|b| scan.step(**b)).count())
            .collect::<Vec<_>>()
    };
    assert_eq!(ends(&[b"x\x1b[6ny\x1b[6n"]), [2]);
    assert_eq!(ends(&[b"\x1b[", b"6", b"n"]), [0, 0, 1]);
    assert_eq!(ends(&[b"\x1b\x1b[6n"]), [1], "a stray ESC does not hide it");
    assert_eq!(ends(&[b"\x1b[5n\x1b[16n\x1b[?6n\x1b[6;1n"]), [0]);
}

#[cfg(all(unix, feature = "colour-replies"))]
mod pty {
    use super::*;

    fn session(layer: Box<dyn Layer + Send>, script: &str) -> Session {
        Session::start(layer, 60, 6, "/bin/sh", &["-c", script])
            .unwrap_or_else(|e| unreachable!("sh starts: {e}"))
    }

    fn b_layer() -> Box<dyn Layer + Send> {
        Box::new(lumen_arm_b::Layer::new(lumen_arm_b::Config {
            budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
            background_as_text: false,
        }))
    }

    fn grid_text(s: &Session) -> String {
        s.document()
            .blocks()
            .iter()
            .filter_map(|b| match b {
                lumen_term::Block::Grid(g) => Some(g.text_rows().join("\n")),
                lumen_term::Block::Rich(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn pump_until(s: &mut Session, done: impl Fn(&Session) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end {
            s.pump_wait(Duration::from_millis(100));
            if done(s) {
                return true;
            }
        }
        false
    }

    /// Run a program that prints `queries` and then reads, in raw mode, for a
    /// second and a half, and return what it received.
    fn ask(layer: Box<dyn Layer + Send>, name: &str, queries: &[u8]) -> Vec<u8> {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
        let q = dir.join(format!("{name}.queries"));
        let out = dir.join(format!("{name}.received"));
        assert!(std::fs::write(&q, queries).is_ok());
        let _ = std::fs::remove_file(&out);
        let script = format!(
            "stty raw -echo; cat '{q}'; stty min 0 time 15; \
             dd bs=1 count=4096 of='{out}' 2>/dev/null; echo DONE",
            q = q.display(),
            out = out.display()
        );
        let mut s = session(layer, &script);
        assert!(
            pump_until(&mut s, |s| grid_text(s).contains("DONE")),
            "{name}: the program never finished; the grid shows {:?}",
            grid_text(&s)
        );
        std::fs::read(&out).unwrap_or_default()
    }

    /// A program that asks after the look switched to dark is answered from
    /// the dark palette: replies follow the live switch, not the start.
    #[test]
    fn replies_follow_a_live_switch_of_the_look() {
        use lumen_term::App;
        use lumen_term::design::{Appearance, Palette};
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
        let out = dir.join("reply-live-switch.received");
        let _ = std::fs::remove_file(&out);
        let script = format!(
            "sleep 1; stty raw -echo; printf '\\033]11;?\\033\\\\'; stty min 0 time 15; \
             dd bs=1 count=4096 of='{out}' 2>/dev/null; echo DONE",
            out = out.display()
        );
        let mut app = App::start(
            Box::new(Passthrough),
            "/bin/sh",
            &["-c", &script],
            1,
            600,
            300,
        )
        .unwrap_or_else(|e| unreachable!("sh starts: {e}"));
        assert!(app.set_appearance(Appearance::Dark));
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end && !grid_text(app.session()).contains("DONE") {
            app.pump();
            std::thread::sleep(Duration::from_millis(20));
        }
        let [r, g, b] = Palette::dark().canvas;
        let want = format!("\x1b]11;rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x1b\\");
        assert_eq!(
            String::from_utf8_lossy(&std::fs::read(&out).unwrap_or_default()),
            want
        );
    }

    #[test]
    fn a_program_that_asks_gets_the_themes_colours_back_on_every_layer() {
        for (label, layer) in [
            (
                "passthrough",
                Box::new(Passthrough) as Box<dyn Layer + Send>,
            ),
            ("arm-b", b_layer()),
        ] {
            let got = ask(
                layer,
                &format!("reply-{label}"),
                b"\x1b]11;?\x1b\\\x1b]4;2;?\x07",
            );
            // The terminal's theme (the design tokens): a white canvas and
            // ANSI 2 of [24, 128, 56].
            assert_eq!(
                got, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\\x1b]4;2;rgb:1818/8080/3838\x1b\\",
                "{label}"
            );
        }
    }

    #[test]
    fn a_program_that_sets_or_batches_colours_gets_no_reply() {
        let got = ask(
            Box::new(Passthrough),
            "reply-none",
            b"\x1b]10;#ff0000\x07\x1b]4;1;?;2;?\x07\x1b]13;?\x07\x1b]52;c;?\x07\x1b]99;?\x07",
        );
        assert_eq!(got, b"");
    }

    #[test]
    fn a_flood_of_queries_is_answered_within_the_limit_and_the_terminal_keeps_running() {
        let flood = b"\x1b]11;?\x07".repeat(500);
        let got = ask(Box::new(Passthrough), "reply-flood", &flood);
        let reply = b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\";
        assert!(!got.is_empty(), "some were answered");
        assert_eq!(got.len() % reply.len(), 0, "only whole replies");
        assert!(
            got.len() <= MAX_PER_WINDOW * reply.len(),
            "{} bytes",
            got.len()
        );
        assert!(got.chunks(reply.len()).all(|c| c == reply));
    }

    /// A program that asks for colours for as long as it runs and never reads its
    /// input must not stop typed input from being submitted, however long the
    /// replies have been filling its PTY. Every send is timed.
    #[test]
    fn a_program_that_asks_continuously_and_never_reads_does_not_stall_typed_input() {
        let script = "stty raw -echo; i=0; \
                      while [ $i -lt 240 ]; do printf '\\033]11;?\\033\\\\'; \
                      printf 'tick%s\\n' $i; i=$((i+1)); sleep 0.05; done; echo FINISHED";
        let mut s = session(Box::new(Passthrough), script);
        let mut slowest = Duration::ZERO;
        let mut sends = 0;
        // Enough per send to fill what a PTY holds in a few seconds, as a paste
        // into a program that is not reading would.
        let typed = vec![b'k'; 256];
        let end = Instant::now() + Duration::from_secs(60);
        while Instant::now() < end && !grid_text(&s).contains("FINISHED") {
            s.pump_wait(Duration::from_millis(50));
            let start = Instant::now();
            let result = s.send(&typed);
            slowest = slowest.max(start.elapsed());
            sends += 1;
            if let Err(e) = result {
                assert!(
                    matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::BrokenPipe
                    ),
                    "a send failed for a reason other than an unread queue: {e:?}"
                );
            }
        }
        eprintln!("{sends} sends, slowest {slowest:?}");
        assert!(
            grid_text(&s).contains("FINISHED"),
            "the terminal stopped taking output; the grid shows {:?}",
            grid_text(&s)
        );
        assert!(sends > 100, "{sends} sends");
        assert!(
            slowest < Duration::from_millis(500),
            "a typed send took {slowest:?} while the program asked and never read"
        );
    }
}
