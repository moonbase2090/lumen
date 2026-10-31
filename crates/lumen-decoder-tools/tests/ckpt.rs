//! Console checkpoints: the block survives noise, the console window and damage,
//! and a block that is not whole and verified is never used.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::cast_possible_truncation
)]

use lumen_decoder_tools::ckpt::{
    CHUNK_CHARS, EncodeError, MAX_BLOCK_TEXT, MAX_BYTES, MAX_GAP, MAX_WORD, WINDOW, base64_decode,
    base64_encode, decode_all, decode_newest, encode,
};

fn sample(n: usize, salt: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt))
        .collect()
}

fn text(lines: &[String]) -> String {
    lines.join("\n") + "\n"
}

#[test]
fn base64_round_trips_every_length_and_refuses_bad_input() {
    for n in 0..70 {
        let d = sample(n, 3);
        assert_eq!(base64_decode(&base64_encode(&d)), Some(d));
    }
    assert_eq!(base64_encode(b"Man"), "TWFu");
    assert_eq!(base64_encode(b"Ma"), "TWE=");
    assert_eq!(base64_decode("TWE"), None);
    assert_eq!(base64_decode("TW!="), None);
    assert_eq!(base64_decode("TWE=TWFu"), None, "padding only at the end");
}

#[test]
fn a_block_round_trips_through_noisy_console_text() {
    let data = sample(5000, 1);
    let lines = encode("LTM51-STATUS", "c1", 7, "hourly", &data).unwrap();
    assert!(lines.iter().all(|l| l.len() < CHUNK_CHARS + 80));
    let mut noisy = String::from("[    0.1] kernel: boot\nLTM51-STATUS png cpu=1\n");
    for (i, l) in lines.iter().enumerate() {
        noisy += &format!("[ {i:>6}.5] {l}\n");
        if i % 3 == 0 {
            noisy += "random console chatter 12345\n";
        }
    }
    let b = decode_newest(&noisy).expect("block");
    assert_eq!((b.id.as_str(), b.seq, b.kind.as_str()), ("c1", 7, "hourly"));
    assert_eq!(b.data, data);
}

#[test]
fn the_newest_complete_block_wins_and_a_cut_off_one_is_skipped() {
    let a = text(&encode("P", "a", 1, "hourly", &sample(3000, 1)).unwrap());
    let b = text(&encode("P", "b", 2, "hourly", &sample(3000, 2)).unwrap());
    let c_lines = encode("P", "c", 3, "hourly", &sample(3000, 3)).unwrap();
    // The newest block lost its last chunk and its end line.
    let c_cut = text(&c_lines[..c_lines.len() - 2]);
    let all = format!("{a}{b}{c_cut}");
    let got = decode_newest(&all).unwrap();
    assert_eq!((got.id.as_str(), got.seq), ("b", 2));
    // With the end line only missing it is also skipped.
    let c_noend = text(&c_lines[..c_lines.len() - 1]);
    assert_eq!(decode_newest(&format!("{a}{b}{c_noend}")).unwrap().seq, 2);
    // Whole, it wins.
    assert_eq!(
        decode_newest(&format!("{a}{b}{}", text(&c_lines)))
            .unwrap()
            .seq,
        3
    );
}

#[test]
fn a_damaged_block_never_verifies() {
    let data = sample(4000, 9);
    let lines = encode("P", "x", 1, "hourly", &data).unwrap();
    // Flip one character of a data line.
    let mut bad = lines.clone();
    let l = &mut bad[2];
    let i = l.len() - 10;
    let ch = if l.as_bytes()[i] == b'A' { 'B' } else { 'A' };
    l.replace_range(i..=i, &ch.to_string());
    assert_eq!(decode_all(&text(&bad)).len(), 0);
    // A missing middle chunk.
    let mut gap = lines.clone();
    gap.remove(2);
    assert_eq!(decode_all(&text(&gap)).len(), 0);
    // Two copies of one chunk that disagree.
    let mut twice = lines.clone();
    let mut other = lines[2].clone();
    other.replace_range(other.len() - 5.., "AAAAA");
    twice.insert(3, other);
    assert_eq!(decode_all(&text(&twice)).len(), 0);
}

#[test]
fn a_repeated_snapshot_is_read_once_cleanly() {
    let lines = encode("P", "a", 4, "final", &sample(2500, 4)).unwrap();
    let doubled = format!("{}{}", text(&lines), text(&lines));
    let all = decode_all(&doubled);
    assert!(all.iter().all(|b| b.seq == 4 && b.data == sample(2500, 4)));
    assert_ne!(all.len(), 0);
}

#[test]
fn the_size_limits_are_enforced_on_the_whole_block() {
    assert_eq!(
        encode("P", "a", 1, "k", &sample(MAX_BYTES + 1, 0)),
        Err(EncodeError::TooBig(MAX_BYTES + 1))
    );
    assert_eq!(encode("P x", "a", 1, "k", b""), Err(EncodeError::BadWord));
    assert_eq!(encode("P", "a b", 1, "k", b""), Err(EncodeError::BadWord));
    let long = "x".repeat(MAX_WORD + 1);
    assert_eq!(encode(&long, "a", 1, "k", b""), Err(EncodeError::BadWord));
    assert_eq!(encode("P", &long, 1, "k", b""), Err(EncodeError::BadWord));
    // The largest payload with the longest words still fits the text limit.
    let w = "w".repeat(MAX_WORD);
    let lines = encode(&w, &w, u64::MAX, &w, &sample(MAX_BYTES, 1)).unwrap();
    let text: usize = lines.iter().map(|l| l.len() + 1).sum();
    assert!(text <= MAX_BLOCK_TEXT, "{text}");
}

#[test]
fn a_block_that_lies_about_its_size_is_not_read() {
    let lines = encode("P", "a", 1, "k", &sample(3000, 2)).unwrap();
    // More chunks claimed than sent, fewer than sent, and a size over the limit.
    for (from, to) in [
        ("chunks=5", "chunks=6"),
        ("chunks=5", "chunks=4"),
        ("bytes=3000", "bytes=999999"),
        ("bytes=3000", "bytes=18446744073709551615"),
        ("chunks=5", "chunks=18446744073709551615"),
    ] {
        let mut bad = lines.clone();
        assert!(bad[0].contains(from), "{}", bad[0]);
        bad[0] = bad[0].replace(from, to);
        assert_eq!(decode_all(&text(&bad)).len(), 0, "{from} -> {to}");
    }
    // A chunk line out of range of the declared count.
    let mut extra = lines.clone();
    extra.insert(extra.len() - 1, "P CKPT-DATA id=a n=9 AAAA".to_string());
    assert_eq!(decode_all(&text(&extra)).len(), 0);
}

/// Console text of `count` maximum-size blocks with `gap` bytes of other output
/// before each; returns the text and each block's (start, end, seq).
fn console_of(count: u64, gap: usize) -> (String, Vec<(usize, usize, u64)>) {
    let mut console = String::new();
    let mut spans = Vec::new();
    for seq in 1..=count {
        console += &"g".repeat(gap.saturating_sub(1));
        console.push('\n');
        let start = console.len() + "LTM51-STATUS ".len(); // where the marker begins
        console += &text(
            &encode(
                "LTM51-STATUS",
                &format!("h{seq}"),
                seq,
                "hourly",
                &sample(MAX_BYTES, seq as u8),
            )
            .unwrap(),
        );
        spans.push((start, console.len(), seq));
    }
    (console, spans)
}

#[test]
fn with_at_most_the_stated_gap_every_64_kib_window_holds_a_whole_block() {
    // Maximum-size blocks and the largest allowed gap between them. For EVERY
    // window end from one full window to the end of the text, a block lies whole
    // inside (a plain check on the block spans). The reader itself is run on the
    // windows around every block edge (two bytes either side) and on every 251st
    // end; there it must read a block no older than the newest whole one, intact.
    let (console, spans) = console_of(3, MAX_GAP);
    assert!(spans.iter().all(|s| s.1 - s.0 <= MAX_BLOCK_TEXT));
    let mut edges = std::collections::BTreeSet::new();
    for s in &spans {
        for d in 0..=4usize {
            for base in [s.0 + WINDOW, s.1] {
                edges.insert((base + d).saturating_sub(2));
            }
        }
    }
    for end in WINDOW..=console.len() {
        let start = end - WINDOW;
        let want = spans
            .iter()
            .filter(|s| s.0 >= start && s.1 <= end)
            .map(|s| s.2)
            .max();
        assert!(
            want.is_some(),
            "no whole block in the window ending at {end}"
        );
        if end % 251 != 0 && !edges.contains(&end) {
            continue;
        }
        // The reader may even use a block cut a little at either end (the prefix of
        // its first line, the newline of its last), so it never finds less.
        let got = decode_newest(&console[start..end]).expect("a block is read");
        assert!(
            Some(got.seq) >= want,
            "window ending at {end}: {} < {want:?}",
            got.seq
        );
        assert_eq!(got.data, sample(MAX_BYTES, got.seq as u8));
    }
}

#[test]
fn with_more_output_between_blocks_a_window_can_hold_none_and_nothing_is_read() {
    // The claim is narrow on purpose: past the gap a window may hold no whole block.
    let (console, spans) = console_of(3, MAX_GAP + 8000);
    let empty = (WINDOW..=console.len()).find(|&end| {
        let start = end - WINDOW;
        !spans.iter().any(|s| s.0 >= start && s.1 <= end)
    });
    let end = empty.expect("a window with no whole block exists past the stated gap");
    assert!(decode_newest(&console[end - WINDOW..end]).is_none());
}

#[test]
fn nothing_found_in_text_without_blocks() {
    assert!(decode_newest("just a log\nCKPT-END id=zzz\nCKPT-DATA id=q n=0 AAAA\n").is_none());
    assert!(decode_newest("").is_none());
}
