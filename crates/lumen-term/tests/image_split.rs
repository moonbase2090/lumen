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
use lumen_term::images::{ImageHost, platform};
use lumen_term::{Block, Context, Document, Layer, Renderer};
use lumen_track0::{KIND_IMAGE_DATA, KIND_IMAGE_START};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Decodes to a 2 by 2 image whose pixels are opaque red.
struct Red;
impl Decode for Red {
    fn decode(&self, _: Format, _: (u32, u32), _: &[u8]) -> Outcome {
        Outcome::Image(Image {
            width: 2,
            height: 2,
            rgba: [255, 0, 0, 255].repeat(4),
        })
    }
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
    let doc = Document::with_images(60, 6, ImageHost::new(gate(open), Red, false));
    let mut layer = lumen_arm_b::Layer::new(lumen_arm_b::Config {
        budget: lumen_arm_b::Budget::quarter_of(2000, 8 << 20),
        background_as_text: false,
    });
    Layer::open_container(&mut layer);
    (doc, layer)
}

fn image_bytes() -> Vec<u8> {
    let mut v = PNG.to_vec();
    v.resize(40, 0);
    v
}

/// What the document shows: each rich block as its content and whether its pixels
/// arrived, each text row as the search sees it.
fn snapshot(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for b in doc.blocks() {
        if let Block::Rich(r) = b {
            out.push(format!(
                "rich {:?} image={:?}",
                r.content,
                r.image.as_ref().map(|p| (p.width, p.height))
            ));
        }
    }
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("the face loads: {e}"));
    for row in r.search_rows(doc, 60) {
        out.push(format!("row {} {}: {:?}", row.block, row.row, row.text));
    }
    out
}

/// Feeds `stream` in the pieces `cuts` ends (ascending offsets into it), then
/// ends the stream, waits for the decodes the whole stream is known to produce
/// and returns what the document shows.
fn run(stream: &[u8], cuts: &[usize], decodes: usize) -> Vec<String> {
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

type Stream = (&'static str, Vec<u8>, usize);

/// The streams the document is the same for however they are split, and the one
/// it is not (the last of the four).
fn streams() -> (Vec<Stream>, Stream) {
    let bytes = image_bytes();
    let cat = |id: u32| {
        let mut v = start(id, 40, "a cat");
        v.extend(data(id, 0, &bytes[..25]));
        v.extend(data(id, 25, &bytes[25..]));
        v
    };
    let mut good = b"before\r\n".to_vec();
    good.extend(cat(1));
    good.extend(b"after\r\n");

    let mut gap = b"x\r\n".to_vec();
    gap.extend(start(1, 40, "a cat"));
    gap.extend(data(1, 10, &bytes[..10]));
    gap.extend(data(1, 0, &bytes));
    gap.extend(b"y\r\n");

    let mut twice = cat(1);
    twice.extend(b"mid\r\n");
    twice.extend(cat(1));
    twice.extend(cat(2));

    // A frame that promises more than it carries, cut off by the start of the
    // next frame, which is a whole image.
    let mut nested = b"a\r\n".to_vec();
    let mut head = frame(KIND_IMAGE_START, &[0; 60]);
    head.truncate(head.len() - 50);
    nested.extend(head);
    nested.extend(cat(1));
    nested.extend(b"z\r\n");

    // A frame start that is never finished, then plain text with escapes in it.
    let mut stray = b"\x1b_".to_vec();
    stray.extend_from_slice(&lumen_detector::TAG[..2]);
    stray.extend(b"\x1b\x1b_ text \x1b\\ more\r\n");
    stray.extend(start(3, 40, "cut off"));
    stray.extend(data(3, 0, &bytes[..7]));
    stray.extend(b"tail\r\n");
    let all = vec![
        ("one good image between lines of text", good, 1),
        ("frames out of order", gap, 0),
        ("a repeated id and a second image", twice, 2),
        ("a frame cut off by the next one", nested, 1),
        ("stray escapes and a cut-off image", stray, 0),
    ];
    let mut all = all;
    let split_dependent = all.pop().unwrap_or_else(|| unreachable!("four streams"));
    (all, split_dependent)
}

fn check(streams: &[Stream]) {
    for (name, stream, decodes) in streams {
        let (name, decodes) = (*name, *decodes);
        let stream = stream.as_slice();
        let whole = run(stream, &[], decodes);
        assert!(
            whole.iter().any(|l| l.starts_with("rich")),
            "{name}: the stream makes no image block, so it tests nothing"
        );
        for cut in 1..stream.len() {
            assert_eq!(run(stream, &[cut], decodes), whole, "{name}: cut at {cut}");
        }
        let every: Vec<usize> = (1..stream.len()).collect();
        assert_eq!(
            run(stream, &every, decodes),
            whole,
            "{name}: one byte at a time"
        );
        let mut seed = 0x2545_F491_4F6C_DD1D_u64 ^ stream.len() as u64;
        for round in 0..40 {
            let mut cuts = Vec::new();
            let mut at = 0usize;
            loop {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                at += 1 + (seed % 23) as usize;
                if at >= stream.len() {
                    break;
                }
                cuts.push(at);
            }
            assert_eq!(
                run(stream, &cuts, decodes),
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
fn a_rejected_header_is_shown_the_same_however_the_bytes_are_split() {
    check(&[streams().1]);
}
