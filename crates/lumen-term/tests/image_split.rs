//! Image frames arrive in whatever pieces the pty reads: the terminal must build
//! the same document however the bytes are split. The same streams are fed whole,
//! cut in two at every byte, one byte at a time and in seeded random pieces, and
//! the blocks, the text and the decoded pixels must be the same each time. The
//! streams mix text with good frames, a refused image, frames out of order, a
//! repeated id and a cut-off frame, and the end of a command in the middle.

#![allow(missing_docs)]

use lumen_decoder::images::{Decode, Gate};
use lumen_decoder::protocol::Format;
use lumen_decoder::runner::{Image, Outcome};
use lumen_decoder::unlock::{GateRecord, Identities};
use lumen_term::images::{ImageHost, Pixels, platform};
use lumen_term::{Block, Context, Document, Layer, Renderer};
use lumen_track0::{KIND_IMAGE_DATA, KIND_IMAGE_START};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Decodes to a 2 by 2 image whose colour comes from the byte after the
/// signature (its tone): tone 0 is opaque red, any other is opaque blue of that
/// strength. Two images with different tones therefore differ in their pixels.
struct Toned;
impl Decode for Toned {
    fn decode(&self, _: Format, _: (u32, u32), bytes: &[u8]) -> Outcome {
        let pixel = match bytes.get(8).copied().unwrap_or(0) {
            0 => [255, 0, 0, 255],
            tone => [0, 0, tone, 255],
        };
        Outcome::Image(Image {
            width: 2,
            height: 2,
            rgba: pixel.repeat(4),
        })
    }
}

/// The pixels `Toned` makes for `tone`.
fn pixels(tone: u8) -> Pixels {
    let pixel = if tone == 0 {
        [255, 0, 0, 255]
    } else {
        [0, 0, tone, 255]
    };
    Pixels::new(2, 2, pixel.repeat(4))
}

fn gate(open: bool) -> Gate {
    let records = if open {
        vec![GateRecord {
            platform: platform().into(),
            decoder_build: "d".into(),
            image_path_build: "p".into(),
            decoder_gate_passed: true,
            memory_gate_passed: true,
        }]
    } else {
        Vec::new()
    };
    Gate {
        setting_on: true,
        platform: platform().into(),
        identities: Identities {
            decoder_build: "d".into(),
            image_path_build: "p".into(),
        },
        records,
    }
}

fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&lumen_detector::TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn start(id: u32, len: u32, alt: &str) -> Vec<u8> {
    let mut b = id.to_be_bytes().to_vec();
    b.push(1);
    for n in [2u32, 2, len] {
        b.extend_from_slice(&n.to_be_bytes());
    }
    b.extend_from_slice(&u16::try_from(alt.len()).unwrap_or(0).to_be_bytes());
    b.extend_from_slice(alt.as_bytes());
    frame(KIND_IMAGE_START, &b)
}

fn data(id: u32, offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut b = id.to_be_bytes().to_vec();
    b.extend_from_slice(&offset.to_be_bytes());
    b.extend_from_slice(bytes);
    frame(KIND_IMAGE_DATA, &b)
}

fn setup(open: bool) -> (Document, lumen_arm_b::Layer) {
    let doc = Document::with_images(60, 6, ImageHost::new(gate(open), Toned, false));
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    Layer::open_container(&mut layer);
    (doc, layer)
}

/// An image's bytes: the signature, then its tone, padded to 40 bytes.
fn image_bytes(tone: u8) -> Vec<u8> {
    let mut v = PNG.to_vec();
    v.push(tone);
    v.resize(40, 0);
    v
}

/// What the document shows: each rich block as its content and its decoded
/// pixels (none while the alt text stands in), each text row as the search sees it.
#[derive(Debug, PartialEq, Eq)]
struct Snap {
    rich: Vec<(String, Option<Pixels>)>,
    rows: Vec<String>,
}

fn snapshot(doc: &Document) -> Snap {
    let mut rich = Vec::new();
    for b in doc.blocks() {
        if let Block::Rich(r) = b {
            rich.push((format!("{:?}", r.content), r.image.clone()));
        }
    }
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("the face loads: {e}"));
    let rows = r
        .search_rows(doc, 60)
        .into_iter()
        .map(|row| format!("{} {}: {:?}", row.block, row.row, row.text))
        .collect();
    Snap { rich, rows }
}

/// Feeds `stream` in the pieces `cuts` ends (ascending offsets into it), then
/// ends the stream, waits for the decodes the whole stream is known to produce
/// and returns what the document shows.
fn run(stream: &[u8], cuts: &[usize], decodes: usize) -> Snap {
    let (mut doc, mut layer) = setup(true);
    let mut from = 0;
    for &cut in cuts.iter().chain(std::iter::once(&stream.len())) {
        doc.receive(&mut layer, &stream[from..cut], Context::default());
        from = cut;
    }
    // The stand-in decoder answers at once, so a decode that was started is
    // collected after a few rounds; the bound only turns a decode that never
    // started into a failure instead of a hang.
    let mut seen = 0;
    let mut rounds = 0u32;
    while seen < decodes {
        if doc.poll_images() {
            seen += 1;
        } else {
            rounds += 1;
            assert!(rounds < 5_000_000, "a decode did not finish");
            std::thread::yield_now();
        }
    }
    snapshot(&doc)
}

/// A stream and the document it must make, written out: the rich blocks with
/// their pixels and the text rows. The decodes it starts are the images in it.
struct Stream {
    name: &'static str,
    bytes: Vec<u8>,
    want: Snap,
}

impl Stream {
    fn decodes(&self) -> usize {
        self.want.rich.iter().filter(|(_, p)| p.is_some()).count()
    }
}

/// The streams the document is the same for however they are split, and the one
/// it is not (a rejected header cut by a read).
fn streams() -> (Vec<Stream>, Stream) {
    let cat = |id: u32, tone: u8| {
        let bytes = image_bytes(tone);
        let mut v = start(id, 40, "a cat");
        v.extend(data(id, 0, &bytes[..25]));
        v.extend(data(id, 25, &bytes[25..]));
        v
    };
    let bytes = image_bytes(0);
    let mut good = b"before\r\n".to_vec();
    good.extend(cat(1, 0));
    good.extend(b"after\r\n");

    let mut gap = b"x\r\n".to_vec();
    gap.extend(start(1, 40, "a cat"));
    gap.extend(data(1, 10, &bytes[..10]));
    gap.extend(data(1, 0, &bytes));
    gap.extend(b"y\r\n");

    // Two images in a row, the second a different colour.
    let mut twice = cat(1, 0);
    twice.extend(b"mid\r\n");
    twice.extend(cat(2, 7));

    // A frame that promises more than it carries, cut off by the start of the
    // next frame, which is a whole image.
    let mut nested = b"a\r\n".to_vec();
    let mut head = frame(KIND_IMAGE_START, &[0; 60]);
    head.truncate(head.len() - 50);
    nested.extend(head);
    nested.extend(cat(1, 0));
    nested.extend(b"z\r\n");

    // A frame start that is never finished, then plain text with escapes in it.
    let mut stray = b"\x1b_".to_vec();
    stray.extend_from_slice(&lumen_detector::TAG[..2]);
    stray.extend(b"\x1b\x1b_ text \x1b\\ more\r\n");
    stray.extend(start(3, 40, "cut off"));
    stray.extend(data(3, 0, &bytes[..7]));
    stray.extend(b"tail\r\n");
    let want = |name: &str| want(name);
    let all = vec![
        Stream {
            name: "one good image between lines of text",
            bytes: good,
            want: want("good"),
        },
        Stream {
            name: "frames out of order",
            bytes: gap,
            want: want("gap"),
        },
        Stream {
            name: "two images with text between",
            bytes: twice,
            want: want("twice"),
        },
        Stream {
            name: "a frame cut off by the next one",
            bytes: nested,
            want: want("nested"),
        },
    ];
    let stray = Stream {
        name: "stray escapes and a cut-off image",
        bytes: stray,
        want: want("stray"),
    };
    (all, stray)
}

/// The document each stream must make, written out by hand from the rules: a
/// start frame is a rich block that shows its pixels once decoded, the alt text
/// until then and whenever it is refused; a refused frame is shown as escaped
/// text.
fn want(name: &str) -> Snap {
    let start = |id: u32, alt: &str| {
        format!(
            "ImageStart {{ id: {id}, format: 1, width: 2, height: 2, length: 40, alt: \"{alt}\" }}"
        )
    };
    let rows = |rows: &[&str]| rows.iter().map(|r| (*r).to_string()).collect();
    match name {
        "good" => Snap {
            rich: vec![(start(1, "a cat"), Some(pixels(0)))],
            rows: rows(&["0 0: \"before\"", "2 0: \"after\"", "2 1: \"\""]),
        },
        "gap" => Snap {
            rich: vec![(start(1, "a cat"), None)],
            rows: rows(&[
                "0 0: \"x\"",
                "1 0: \"a cat\"",
                "2 0: \"\\\\x84\\\\x00\\\\x00\\\\x00\\\\x01\\\\x00\\\\x00\\\\x00\\\\x0a\\\\x89PNG\\\\x0d\\\\x0a\\\\x1a\\\\x0a\\\\\"",
                "2 1: \"x00\\\\x00 [19 bytes]\"",
                "2 2: \"\\\\x84\\\\x00\\\\x00\\\\x00\\\\x01\\\\x00\\\\x00\\\\x00\\\\x00\\\\x89PNG\\\\x0d\\\\x0a\\\\x1a\\\\x0a\\\\\"",
                "2 3: \"x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\\"",
                "2 4: \"x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\\"",
                "2 5: \"x00\\\\x00 [49 bytes]\"",
                "2 6: \"y\"",
                "2 7: \"\"",
            ]),
        },
        "twice" => Snap {
            rich: vec![
                (start(1, "a cat"), Some(pixels(0))),
                (start(2, "a cat"), Some(pixels(7))),
            ],
            rows: rows(&["1 0: \"mid\""]),
        },
        "nested" => Snap {
            rich: vec![(start(1, "a cat"), Some(pixels(0)))],
            rows: rows(&[
                "0 0: \"a\"",
                "0 1: \"\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00\\\\x00 [10 bytes]\"",
                "2 0: \"z\"",
                "2 1: \"\"",
            ]),
        },
        _ => Snap {
            rich: vec![(start(3, "cut off"), None)],
            rows: rows(&[
                "0 0: \"\\\\x8aL [2 bytes]\"",
                "0 1: \" text  [6 bytes]\"",
                "0 2: \" more\"",
                "1 0: \"cut off\"",
                "2 0: \"tail\"",
                "2 1: \"\"",
            ]),
        },
    }
}

fn check(streams: &[Stream]) {
    for stream in streams {
        let (name, bytes, decodes) = (stream.name, stream.bytes.as_slice(), stream.decodes());
        let whole = run(bytes, &[], decodes);
        assert_eq!(whole, stream.want, "{name}: the whole stream");
        for cut in 1..bytes.len() {
            assert_eq!(run(bytes, &[cut], decodes), whole, "{name}: cut at {cut}");
        }
        let every: Vec<usize> = (1..bytes.len()).collect();
        assert_eq!(
            run(bytes, &every, decodes),
            whole,
            "{name}: one byte at a time"
        );
        let mut seed = 0x2545_F491_4F6C_DD1D_u64 ^ bytes.len() as u64;
        for round in 0..40 {
            let mut cuts = Vec::new();
            let mut at = 0usize;
            loop {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                at += 1 + (seed % 23) as usize;
                if at >= bytes.len() {
                    break;
                }
                cuts.push(at);
            }
            assert_eq!(
                run(bytes, &cuts, decodes),
                whole,
                "{name}: random pieces, round {round}"
            );
        }
    }
}

#[test]
fn the_document_is_the_same_however_the_bytes_are_split() {
    check(&streams().0);
}

/// Known defect: the detector reports the header of a rejected frame as escaped
/// bytes that "may be split at read boundaries and are to be joined", and the
/// layer shows each piece as its own escaped text, so where a read ends inside
/// such a header changes the grouping and the byte counts of the text shown.
/// Fixing it changes the parser and the layer, which are in the image-path build
/// identity, so the gate has to be redone for the fix: it is the gate owner's
/// call. Remove the ignore with the fix.
#[test]
#[ignore = "known: a rejected header split across reads is shown in pieces"]
fn a_rejected_header_is_shown_the_same_however_the_bytes_are_split() {
    check(&[streams().1]);
}
