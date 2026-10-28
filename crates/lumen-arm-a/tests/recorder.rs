//! Arm A driven through the shared run recorder: one log line per read, kinds
//! and lengths only.

use lumen_arm_a::LayerState;
use lumen_arm_a::pipeline::{Budget, Config, Context, Layer, item_summary};
use lumen_detector::TAG;
use lumen_harness::{HeaderInfo, Recorder};
use lumen_track0::KIND_PARAGRAPH;

fn para(text: &str) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(KIND_PARAGRAPH);
    let body_len = u32::try_from(text.len() + 2).unwrap_or(0);
    v.extend_from_slice(&body_len.to_be_bytes());
    v.extend_from_slice(&u16::try_from(text.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(text.as_bytes());
    v
}

/// Replay `reads` through a layer and return the recorder's log.
fn record(state: LayerState, reads: &[Vec<u8>]) -> String {
    let header = HeaderInfo {
        commit: "test".into(),
        features: "arm-a".into(),
        corpus: "inline".into(),
        sinks: "none".into(),
    };
    let Ok(mut rec) = Recorder::new(Vec::new(), &header) else {
        unreachable!("writing to a Vec cannot fail")
    };
    let mut layer = Layer::new(
        Config {
            budget: Budget::quarter_of(400, 40_000),
        },
        state,
    );
    layer.open_container();
    for (i, read) in reads.iter().enumerate() {
        let items = layer.feed(
            read,
            Context {
                now_ms: u64::try_from(i).unwrap_or(0),
                ..Context::default()
            },
        );
        let pairs: Vec<_> = items.iter().map(item_summary).collect();
        assert!(
            rec.read(u64::try_from(i).unwrap_or(0), read.len(), &pairs)
                .is_ok()
        );
    }
    let Ok(out) = rec.finish() else {
        unreachable!("flushing a Vec cannot fail")
    };
    String::from_utf8(out).unwrap_or_default()
}

#[test]
fn a_sealed_layer_logs_a_block_and_an_unsealed_layer_logs_text() {
    let reads = vec![b"plain ".to_vec(), para("secret words")];
    let sealed = record(LayerState::Sealed, &reads);
    let unsealed = record(LayerState::Unsealed, &reads);
    let kinds = |log: &str| {
        log.lines()
            .skip(1)
            .map(|l| {
                ["output", "block", "text"]
                    .into_iter()
                    .filter(|k| l.contains(&format!("\"kind\":\"{k}\"")))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(kinds(&sealed), vec![vec!["output"], vec!["block"]]);
    assert_eq!(kinds(&unsealed), vec![vec!["output"], vec!["text"]]);
}

#[test]
fn the_log_has_a_header_then_one_line_per_read_and_no_content() {
    let log = record(LayerState::Sealed, &[para("secret words"), b"x".to_vec()]);
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(lines.len(), 3, "{log}");
    assert!(lines[0].starts_with("{\"header\":"), "{log}");
    assert!(!log.contains("secret"), "{log}");
}
