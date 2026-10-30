//! The stop rule's view of a format must agree with the judge that decides the
//! gate, or the campaign would stop on something the judge then fails.

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

/// What the judge says about one format with these samples and crashes.
fn judged(samples: &[Value], crashes: &[Value]) -> bool {
    let Ok(entry) = ledger::format_entry("png", samples, crashes) else {
        return false;
    };
    let ledger = ledger::ledger("fz-1", "d", "p", &[entry], "se", 1, 1);
    judge(&ledger.to_string()).is_ok_and(|report| report.passes())
}

#[test]
fn a_format_that_meets_every_part_holds() {
    let s = samples(&[(100.0, 900.0), (830.0, 950.0), (1000.0, 950.0)]);
    let st = status(&s, &crashes(&[])).unwrap_or_else(|_| unreachable_status());
    assert!(st.holds);
    assert_eq!(st.cpu_hours, 1000.0);
    assert!(!st.over_cap);
    assert!(st.plateau);
    assert_eq!(st.hours_since_new_crash, 1000.0, "no crash: all of them");
    assert_eq!(
        st.hours_since_coverage_gain, 170.0,
        "coverage last grew at 830"
    );
}

fn unreachable_status() -> lumen_decoder_tools::gate::Status {
    lumen_decoder_tools::gate::Status {
        cpu_hours: -1.0,
        edges: 0.0,
        crashes: 0,
        over_cap: false,
        hours_since_new_crash: 0.0,
        hours_since_coverage_gain: 0.0,
        plateau: false,
        holds: false,
    }
}

#[test]
fn each_part_of_the_gate_can_fail_it() {
    let flat = [(100.0, 900.0), (500.0, 950.0), (1000.0, 950.0)];
    let hold =
        |p: &[(f64, f64)], c: &[f64]| status(&samples(p), &crashes(c)).is_ok_and(|st| st.holds);
    assert!(hold(&flat, &[]));
    assert!(
        !hold(&[(50.0, 950.0), (249.0, 950.0)], &[]),
        "249 CPU-hours, under the floor"
    );
    assert!(
        hold(&[(50.0, 950.0), (250.0, 950.0)], &[]),
        "the floor is enough: the earlier 1000 no longer applies"
    );
    assert!(!hold(&flat, &[900.0]), "a crash inside the window");
    assert!(!hold(&flat, &[40.0]), "a crash long ago still fails");
    assert!(
        !hold(&[(50.0, 950.0), (1201.0, 950.0)], &[]),
        "past the cap"
    );
    assert!(hold(&[(50.0, 950.0), (1200.0, 950.0)], &[]), "at the cap");
    assert!(
        !hold(
            &[
                (100.0, 900.0),
                (500.0, 950.0),
                (900.0, 951.0),
                (1000.0, 951.0)
            ],
            &[]
        ),
        "coverage grew inside the window"
    );
    assert!(
        !hold(&[(900.0, 950.0), (1000.0, 950.0)], &[]),
        "no reading from before the window to compare with"
    );
}

#[test]
fn the_stop_rule_agrees_with_the_judge_on_the_boundaries() {
    // Hours are chosen so that the window starts at exactly 832.
    for (points, crash_at) in [
        (vec![(832.0, 950.0), (1000.0, 950.0)], vec![832.0]), // a crash exactly 168 before the end: still a crash
        (vec![(832.0, 950.0), (1000.0, 950.0)], vec![832.5]), // just inside
        (vec![(832.0, 950.0), (1000.0, 950.0)], vec![]),      // reading exactly at the window start
        (vec![(832.5, 950.0), (1000.0, 950.0)], vec![]),      // reading just after it
        (
            vec![(100.0, 900.0), (832.0, 950.0), (1000.0, 951.0)],
            vec![],
        ), // grew at the end
        (vec![(100.0, 900.0), (1000.0, 950.0)], vec![]), // grew, then reading before window is lower
        (vec![(999.9, 950.0), (500.0, 950.0)], vec![]),  // unordered samples
        (vec![(100.0, 950.0), (1200.0, 950.0)], vec![]), // at the cap
        (vec![(100.0, 950.0), (1200.5, 950.0)], vec![]), // just past it
        (vec![(80.0, 950.0), (249.5, 950.0)], vec![]),   // just under the floor
        (vec![(82.0, 950.0), (250.0, 950.0)], vec![]),   // at the floor
    ] {
        let (s, c) = (samples(&points), crashes(&crash_at));
        let mine = status(&s, &c).is_ok_and(|st| st.holds);
        assert_eq!(mine, judged(&s, &c), "{points:?} {crash_at:?}");
    }
}

#[test]
fn the_stop_rule_agrees_with_the_judge_on_many_generated_runs() {
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
            // Coverage mostly flat, sometimes a gain.
            if next() % 4 == 0 {
                edges += next() % 5;
            }
            points.push((f64::from(hour.min(end)), f64::from(edges)));
        }
        let crash_count = next() % 3;
        let crash_hours: Vec<f64> = (0..crash_count)
            .map(|_| f64::from(next() % (end + 1)))
            .collect();
        let (samples, crashes) = (samples(&points), crashes(&crash_hours));
        let mine = status(&samples, &crashes).is_ok_and(|st| st.holds);
        assert_eq!(
            mine,
            judged(&samples, &crashes),
            "{points:?} {crash_hours:?}"
        );
        total += 1;
        holding += i32::from(mine);
    }
    // The generator must reach both verdicts, or the agreement above says little.
    assert!(
        holding > 100 && holding < total - 100,
        "{holding} of {total} hold"
    );
}
