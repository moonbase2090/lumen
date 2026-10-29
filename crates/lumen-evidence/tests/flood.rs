//! Flood mode: a short run per arm stays within the unauthenticated share.

use lumen_evidence::Arm;
use lumen_evidence::flood::run_flood;
use std::time::Duration;

#[test]
fn a_flood_stays_within_the_budget_for_every_arm() {
    for arm in [Arm::B, Arm::ASealed, Arm::AUnsealed] {
        let mut log = Vec::new();
        let Ok((report, header)) = run_flood(arm, "testsha", Duration::from_millis(300), &mut log)
        else {
            unreachable!("writing to a Vec succeeds")
        };
        let summary = report.summary(&header);
        assert!(report.reads > 0, "{summary}");
        assert!(report.within_budget(), "{summary}");
        assert!(report.kinds.get("block").copied().unwrap_or(0) <= report.max_blocks);
        assert!(report.memory_ok() != Some(false), "{summary}");
        for line in [
            "2.6 frame_rate=not measured",
            "2.6 lying_decoder=not measured",
            "UI stall not measured",
        ] {
            assert!(summary.contains(line), "{line}: {summary}");
        }
        let text = String::from_utf8(log).unwrap_or_default();
        let first = text.lines().next().unwrap_or_default();
        assert!(first.contains("\"corpus\":\"none\""), "{first}");
        assert!(first.contains("\"session_matrix\":\"none\""), "{first}");
        assert!(text.contains("{\"case\":\"flood\"}"));
    }
}

#[test]
fn the_flood_reaches_the_budget_and_gives_one_notice() {
    let Ok((report, _)) = run_flood(Arm::B, "testsha", Duration::from_millis(300), Vec::new())
    else {
        unreachable!("writing to a Vec succeeds")
    };
    assert_eq!(report.kinds.get("block"), Some(&report.max_blocks));
    assert_eq!(report.kinds.get("degraded"), Some(&1));
}
