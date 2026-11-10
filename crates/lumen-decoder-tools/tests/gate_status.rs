//! The campaign's view of a format must agree with the judge that decides the
//! gate, or the campaign would stop on something the judge then fails. The gate
//! is a floor of 250 CPU-hours and no open crash or confirmed hang.

#![allow(missing_docs)]

use lumen_decoder_tools::gate::status;
use lumen_decoder_tools::ledger;
use lumen_evidence::decoder::judge;
use serde_json::{Value, json};

fn samples(points: &[(f64, f64)]) -> Vec<Value> {
    points
        .iter()
        .map(|(h, e)| json!({"at_cpu_hour": h, "edges": e}))
        .collect()
}

fn crashes(at: &[f64]) -> Vec<Value> {
    at.iter()
        .map(|h| json!({"at_cpu_hour": h, "kind": "panic"}))
        .collect()
}

/// Records of a raw timeout that a replay under the production limits cleared.
fn cleared(at: &[f64]) -> Vec<Value> {
    at.iter()
        .map(|h| json!({"at_cpu_hour": h, "kind": "hang", "cleared": true}))
        .collect()
}

/// What the judge says about one format with these samples and crashes.
fn judged(samples: &[Value], crashes: &[Value]) -> bool {
    let Ok(entry) = ledger::format_entry("png", samples, crashes) else {
        return false;
    };
    let ledger = ledger::ledger("fz-1", "d", "p", &[entry], "se", 1, 1);
    judge(&ledger.to_string()).is_ok_and(|report| report.passes())
}

#[test]
fn a_format_at_the_floor_with_no_open_finding_holds() {
    let s = samples(&[(100.0, 950.0), (260.0, 950.0)]);
    let st = status(&s, &crashes(&[])).unwrap_or_else(|_| unreachable_status());
    assert!(st.holds);
    assert_eq!(st.cpu_hours, 260.0);
    assert_eq!((st.crashes, st.open), (0, 0));
    assert_eq!(st.hours_since_new_crash, 260.0, "no crash: all of them");
    assert_eq!(
        st.hours_since_coverage_gain, 160.0,
        "coverage last grew at 100"
    );
}

fn unreachable_status() -> lumen_decoder_tools::gate::Status {
    lumen_decoder_tools::gate::Status {
        cpu_hours: -1.0,
        edges: 0.0,
        crashes: 0,
        open: 0,
        over_cap: false,
        hours_since_new_crash: 0.0,
        hours_since_coverage_gain: 0.0,
        plateau: false,
        holds: false,
    }
}

#[test]
fn only_the_floor_and_open_findings_decide() {
    let hold = |p: &[(f64, f64)], c: Vec<Value>| status(&samples(p), &c).is_ok_and(|st| st.holds);
    let growing = [(100.0, 900.0), (400.0, 950.0), (600.0, 951.0)];
    assert!(
        hold(&growing, vec![]),
        "coverage still growing: not a condition"
    );
    assert!(
        !hold(&[(50.0, 950.0), (249.0, 950.0)], vec![]),
        "249 CPU-hours, under the floor"
    );
    assert!(
        hold(&[(50.0, 950.0), (250.0, 950.0)], vec![]),
        "the floor is enough"
    );
    assert!(
        hold(&[(50.0, 950.0), (1500.0, 950.0)], vec![]),
        "past the budget cap"
    );
    assert!(!hold(&growing, crashes(&[500.0])), "an open crash fails");
    assert!(
        !hold(&growing, crashes(&[40.0])),
        "a crash long ago still fails"
    );
    assert!(
        hold(&growing, cleared(&[40.0, 500.0])),
        "raw timeouts replayed clean under the production limits are not open"
    );
    let mut mixed = cleared(&[40.0]);
    mixed.extend(crashes(&[500.0]));
    assert!(
        !hold(&growing, mixed),
        "one open record among cleared ones fails"
    );
    let st = status(&samples(&growing), &cleared(&[40.0, 500.0]));
    assert!(st.is_ok_and(|st| st.crashes == 2 && st.open == 0));
}

#[test]
fn the_campaign_agrees_with_the_judge_on_the_boundaries() {
    for (points, records) in [
        (vec![(80.0, 950.0), (249.5, 950.0)], crashes(&[])), // just under the floor
        (vec![(82.0, 950.0), (250.0, 950.0)], crashes(&[])), // at the floor
        (vec![(100.0, 900.0), (1000.0, 951.0)], crashes(&[])), // growing at the end
        (vec![(100.0, 950.0), (1200.5, 950.0)], crashes(&[])), // past the budget cap
        (vec![(999.9, 950.0), (500.0, 950.0)], crashes(&[])), // unordered samples
        (vec![(100.0, 950.0), (300.0, 950.0)], crashes(&[300.0])), // an open crash at the end
        (
            vec![(100.0, 950.0), (300.0, 950.0)],
            cleared(&[10.0, 300.0]),
        ), // all cleared
    ] {
        let s = samples(&points);
        let mine = status(&s, &records).is_ok_and(|st| st.holds);
        assert_eq!(mine, judged(&s, &records), "{points:?} {records:?}");
    }
}

#[test]
fn the_campaign_agrees_with_the_judge_on_many_generated_runs() {
    // A small deterministic generator, so a failure can be reproduced.
    let mut state = 0x2545_F491_u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let (mut holding, mut total) = (0, 0);
    for _ in 0..3000 {
        let end = 150 + next() % 1200;
        let mut points = Vec::new();
        let (mut hour, mut edges) = (0_u32, 800_u32);
        while hour < end {
            hour += 1 + next() % 90;
            if next() % 4 == 0 {
                edges += next() % 5;
            }
            points.push((f64::from(hour.min(end)), f64::from(edges)));
        }
        let record_count = next() % 3;
        let mut records = Vec::new();
        for _ in 0..record_count {
            let at = f64::from(next() % (end + 1));
            // About half of the records were replayed clean.
            records.push(json!({"at_cpu_hour": at, "kind": "hang", "cleared": next() % 2 == 0}));
        }
        let samples = samples(&points);
        let mine = status(&samples, &records).is_ok_and(|st| st.holds);
        assert_eq!(mine, judged(&samples, &records), "{points:?} {records:?}");
        total += 1;
        holding += i32::from(mine);
    }
    // The generator must reach both verdicts, or the agreement above says little.
    assert!(
        holding > 100 && holding < total - 100,
        "{holding} of {total} hold"
    );
}
