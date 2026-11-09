//! The image path in the terminal: locked shows the alt text and keeps nothing,
//! unlocked decodes out of process and draws the pixels, and frames out of order
//! change nothing but a line of text.

#![allow(missing_docs)]

use std::time::{Duration, Instant};

use lumen_decoder::images::{Decode, Gate};
use lumen_decoder::protocol::Format;
use lumen_decoder::runner::{Image, Outcome};
use lumen_decoder::unlock::{GateRecord, Identities};
use lumen_term::images::{ImageHost, build_gate, parse_records, platform};
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

fn wait_for_image(doc: &mut Document) {
    let end = Instant::now() + Duration::from_secs(5);
    while !doc.poll_images() {
        assert!(Instant::now() < end, "the decode did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn rich(doc: &Document) -> Vec<&lumen_term::RichBlock> {
    doc.blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Rich(r) => Some(r),
            Block::Grid(_) => None,
        })
        .collect()
}

fn image_bytes() -> Vec<u8> {
    let mut v = PNG.to_vec();
    v.resize(40, 0);
    v
}

#[test]
fn a_build_without_gate_records_is_locked() {
    let g = build_gate(true);
    assert_eq!(g.records, Vec::new());
    assert!(g.identities.decoder_build.starts_with("absent"));
    // Nothing in the environment of the build unlocks it.
    let (mut doc, mut layer) = setup(false);
    doc.receive(&mut layer, &start(1, 40, "a cat"), Context::default());
    doc.receive(&mut layer, &data(1, 0, &image_bytes()), Context::default());
    assert!(!doc.poll_images());
    let blocks = rich(&doc);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].image.is_none());
}

#[test]
fn gate_records_parse_and_a_malformed_line_leaves_images_locked() {
    let r = parse_records("linux d p pass pass\nnot a record\nmacos d p pass fail\n");
    assert_eq!(r.len(), 2);
    assert!(r[0].decoder_gate_passed && r[0].memory_gate_passed);
    assert!(!r[1].memory_gate_passed);
    assert_eq!(parse_records("linux d p pass"), Vec::new());
}

#[test]
fn unlocked_an_image_is_decoded_and_its_pixels_are_drawn() {
    let (mut doc, mut layer) = setup(true);
    let bytes = image_bytes();
    doc.receive(&mut layer, &start(1, 40, "a cat"), Context::default());
    doc.receive(&mut layer, &data(1, 0, &bytes[..25]), Context::default());
    // Not complete: the alt text still stands in.
    assert!(!doc.poll_images());
    doc.receive(&mut layer, &data(1, 25, &bytes[25..]), Context::default());
    wait_for_image(&mut doc);
    let blocks = rich(&doc);
    let pixels = blocks[0].image.as_ref().map(|p| (p.width, p.height));
    assert_eq!(pixels, Some((2, 2)));
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("the face loads: {e}"));
    let frame = r.render(&doc);
    let (_, cell_h) = r.cell();
    // The image is drawn at its own size, below the container label and inside the border.
    let (cell_w, _) = r.cell();
    assert_eq!(frame.pixel(cell_w, cell_h + 1), Some([255, 0, 0]));
    assert_eq!(frame.pixel(cell_w + 1, cell_h + 2), Some([255, 0, 0]));
    assert_ne!(frame.pixel(cell_w + 2, cell_h + 1), Some([255, 0, 0]));
}

#[test]
fn a_large_image_is_reduced_to_the_width_and_never_enlarged() {
    let r = Renderer::light(1).unwrap_or_else(|e| unreachable!("the face loads: {e}"));
    let (cell_w, cell_h) = r.cell();
    let p = |w: u32, h: u32| lumen_term::images::Pixels {
        width: w,
        height: h,
        rgba: std::sync::Arc::new(Vec::new()),
    };
    assert_eq!(r.image_size(&p(10, 10), 60), (10, 10));
    let max_w = 58 * cell_w;
    let (w, h) = r.image_size(&p(max_w * 4, max_w * 2), 60);
    assert_eq!((w, h), (max_w, max_w / 2));
    // A very tall image is held to the row limit.
    let (w, h) = r.image_size(&p(100, 65_535), 60);
    assert!(h <= 40 * cell_h && w >= 1);
}

#[test]
fn frames_out_of_order_change_nothing_and_show_one_line_of_text() {
    let (mut doc, mut layer) = setup(true);
    let bytes = image_bytes();
    doc.receive(&mut layer, &start(1, 40, "a cat"), Context::default());
    // A gap: the image ends, the frame is text.
    doc.receive(&mut layer, &data(1, 10, &bytes[..10]), Context::default());
    doc.receive(&mut layer, &data(1, 0, &bytes), Context::default());
    assert!(!doc.poll_images());
    assert!(rich(&doc)[0].image.is_none());
    let grids = doc
        .blocks()
        .iter()
        .filter(|b| matches!(b, Block::Grid(_)))
        .count();
    assert_eq!(grids, 1, "the refused frames are shown in a text block");
}

#[test]
fn a_command_that_ends_mid_image_drops_what_was_received() {
    let (mut doc, mut layer) = setup(true);
    let bytes = image_bytes();
    doc.receive(&mut layer, &start(1, 40, "a cat"), Context::default());
    doc.receive(&mut layer, &data(1, 0, &bytes[..20]), Context::default());
    doc.begin_prompt_cycle(&mut layer);
    // The rest arrives in the next command: it belongs to no image.
    doc.receive(&mut layer, &data(1, 20, &bytes[20..]), Context::default());
    assert!(!doc.poll_images());
    assert!(rich(&doc)[0].image.is_none());
}
