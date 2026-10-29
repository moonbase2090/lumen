//! The pane corpus (criteria 7.5): scripts for the multiplexer placement
//! comparison.
//!
//! A pane corpus is a directory of `*.json` files read in name order, with the
//! same rules as the corpus of 5.1: every file is `{"revision": "..",
//! "cases": [..]}`, every file gives the same revision, and ids are unique.
//! This module loads and validates the format. It runs nothing: the
//! multiplexer models that play a script come later.
//!
//! A script says what the multiplexer is told and what its panes write, not
//! how the sealed frames are encoded, so a case does not depend on the payload
//! layout of the layout and pane-container frames. Bytes a pane writes
//! (including forged frames) are given as hex or fill, as in 5.1.

use crate::{Read, read_of};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

/// The outcomes a case may forbid. They cover five of the metrics of 7.2; the
/// others (trust-margin pixels, introducer fuzzing, key custody, pane reach)
/// are measured another way.
pub const FORBID_KINDS: [&str; 7] = [
    "draw_outside_rect",
    "draw_hidden_pane",
    "forged_container_drawn",
    "stale_generation_drawn",
    "key_reuse",
    "unmarked_replay",
    "replayed_frame_accepted",
];

/// A pane's place in a layout, in cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRect {
    /// The opaque pane id.
    pub id: u32,
    /// Left column. May be negative: a malformed rectangle is a case.
    pub x: i64,
    /// Top row. May be negative.
    pub y: i64,
    /// Width in columns.
    pub w: u64,
    /// Height in rows.
    pub h: u64,
    /// Whether the pane is visible (default true).
    pub visible: bool,
}

/// One step of a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The multiplexer sends a layout.
    Layout {
        /// The layout generation.
        generation: u32,
        /// The panes and their rectangles.
        panes: Vec<PaneRect>,
    },
    /// A pane writes bytes. The pane id may be one the layout does not name.
    Output {
        /// The pane that wrote.
        pane: u32,
        /// The bytes and their time.
        read: Read,
    },
    /// The outer window is resized.
    Resize {
        /// New width in columns.
        cols: u32,
        /// New height in rows.
        rows: u32,
    },
    /// A pane is zoomed, or the zoom is cleared with `None`.
    Zoom(Option<u32>),
    /// The multiplexer sends a pane container exactly as given, to test the
    /// terminal's checks (a wrong generation, rectangle or pane id).
    Container {
        /// The pane id in the frame.
        pane: u32,
        /// The generation in the frame.
        generation: u32,
        /// The rectangle in the frame.
        rect: PaneRect,
        /// The content type byte.
        kind: u8,
        /// The content payload.
        content: Vec<u8>,
    },
    /// The multiplexer sends the previous sealed frame again, byte for byte.
    Replay,
    /// The multiplexer sends a sealed frame of a type its keys do not cover.
    Foreign {
        /// The sealed type byte.
        kind: u8,
    },
    /// The client detaches.
    Detach,
    /// The client reattaches and gets fresh keys.
    Reattach,
    /// Something happens to a pane's process.
    Process {
        /// The pane.
        pane: u32,
        /// What happened.
        what: ProcessEvent,
    },
    /// A check the terminal's classification of the pane's Track 0 must meet
    /// at this point.
    Expect {
        /// The pane.
        pane: u32,
        /// How its Track 0 must be classified.
        track0: Classification,
        /// Which frames the check is about.
        scope: Scope,
    },
}

impl Event {
    /// Whether the step is about a pane's process, which a model of the
    /// multiplexer and the outer terminal does not have.
    #[must_use]
    pub fn is_process_trust(&self) -> bool {
        matches!(self, Event::Process { .. } | Event::Expect { .. })
    }
}

/// What happens to a pane's process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    /// The pane's first command, by absolute path. `listed` says whether that
    /// program is one of the listed shells.
    Spawn {
        /// The program's absolute path.
        program: String,
        /// Whether it is a listed shell.
        listed: bool,
    },
    /// The foreground leader executes another program.
    Exec {
        /// The new program's absolute path.
        program: String,
    },
    /// The pid the terminal recorded at spawn now belongs to another process
    /// (or has another start time).
    PidReuse,
    /// Job control in the pane's shell is turned on or off.
    JobControl {
        /// Whether it is on.
        on: bool,
    },
    /// The number of processes in the foreground process group.
    Group {
        /// How many.
        members: u32,
    },
    /// The foreground process is this program.
    Foreground {
        /// The program's absolute path.
        program: String,
    },
}

/// How a pane's Track 0 is classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// Possibly remote. In v0 this is every pane's Track 0.
    PossiblyRemote,
    /// Local. No v0 pane case expects it: the local signal for panes is not
    /// part of v0.
    Local,
}

/// Which frames an expectation is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Frames the pane writes from now on.
    Now,
    /// Frames already read earlier in the script, which keep the more remote of
    /// their read-time and emit-time states.
    FramesReadEarlier,
}

/// One pane-corpus case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// Case id.
    pub id: String,
    /// Case category.
    pub category: String,
    /// Window width in columns at the start.
    pub cols: u32,
    /// Window height in rows at the start.
    pub rows: u32,
    /// The script.
    pub script: Vec<Event>,
    /// Outcomes that must not happen.
    pub forbid: Vec<String>,
}

/// A loaded pane corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneCorpus {
    /// The corpus revision.
    pub revision: String,
    /// All cases, in file then case order.
    pub cases: Vec<Case>,
}

fn whole(v: &Value, what: &str) -> Result<u32, String> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("{what} must be a whole number"))
}

fn rect_of(v: &Value) -> Result<PaneRect, String> {
    let signed = |k: &str| {
        v[k].as_i64()
            .ok_or_else(|| format!("a pane's {k} must be a whole number"))
    };
    let size = |k: &str| {
        v[k].as_u64()
            .ok_or_else(|| format!("a pane's {k} must be a non-negative whole number"))
    };
    Ok(PaneRect {
        id: whole(&v["id"], "a pane id")?,
        x: signed("x")?,
        y: signed("y")?,
        w: size("w")?,
        h: size("h")?,
        visible: match &v["visible"] {
            Value::Null => true,
            Value::Bool(b) => *b,
            _ => return Err("a pane's visible must be true or false".into()),
        },
    })
}

#[allow(clippy::too_many_lines, reason = "one arm per kind of step")]
fn event_of(v: &Value) -> Result<Event, String> {
    let op = v["op"].as_str().ok_or("a step needs an op")?;
    match op {
        "layout" => {
            let panes = v["panes"]
                .as_array()
                .ok_or("a layout needs a panes list")?
                .iter()
                .map(rect_of)
                .collect::<Result<Vec<_>, _>>()?;
            let mut seen = BTreeSet::new();
            if let Some(p) = panes.iter().find(|p| !seen.insert(p.id)) {
                return Err(format!("a layout names pane {} twice", p.id));
            }
            Ok(Event::Layout {
                generation: whole(&v["generation"], "a layout generation")?,
                panes,
            })
        }
        "output" => Ok(Event::Output {
            pane: whole(&v["pane"], "an output's pane")?,
            read: read_of(v)?,
        }),
        "resize" => Ok(Event::Resize {
            cols: whole(&v["cols"], "resize cols")?,
            rows: whole(&v["rows"], "resize rows")?,
        }),
        "zoom" => Ok(Event::Zoom(match &v["pane"] {
            Value::Null => None,
            p => Some(whole(p, "a zoom's pane")?),
        })),
        "container" => {
            let mut fields = v["rect"].clone();
            fields["id"] = v["pane"].clone();
            let rect = rect_of(&fields)?;
            let kind = u8::try_from(whole(&v["kind"], "a container's kind")?)
                .map_err(|_| "a container's kind must fit one byte".to_owned())?;
            let content = if v["hex"].is_null() {
                Vec::new()
            } else {
                read_of(v)?.bytes
            };
            Ok(Event::Container {
                pane: rect.id,
                generation: whole(&v["generation"], "a container's generation")?,
                rect,
                kind,
                content,
            })
        }
        "replay" => Ok(Event::Replay),
        "foreign" => Ok(Event::Foreign {
            kind: u8::try_from(whole(&v["kind"], "a foreign frame's kind")?)
                .map_err(|_| "a foreign frame's kind must fit one byte".to_owned())?,
        }),
        "process" => {
            let program = |v: &Value| -> Result<String, String> {
                let p = v["program"]
                    .as_str()
                    .ok_or("a process step needs a program")?;
                if p.starts_with('/') {
                    Ok(p.to_owned())
                } else {
                    Err(format!("program {p:?} must be an absolute path"))
                }
            };
            let what = match v["what"].as_str().ok_or("a process step needs a what")? {
                "spawn" => ProcessEvent::Spawn {
                    program: program(v)?,
                    listed: v["listed"]
                        .as_bool()
                        .ok_or("spawn needs listed true or false")?,
                },
                "exec" => ProcessEvent::Exec {
                    program: program(v)?,
                },
                "pid_reuse" => ProcessEvent::PidReuse,
                "job_control" => ProcessEvent::JobControl {
                    on: v["on"]
                        .as_bool()
                        .ok_or("job_control needs on true or false")?,
                },
                "group" => {
                    let members = whole(&v["members"], "a group's members")?;
                    if members == 0 {
                        return Err("a group has at least one member".into());
                    }
                    ProcessEvent::Group { members }
                }
                "foreground" => ProcessEvent::Foreground {
                    program: program(v)?,
                },
                other => {
                    return Err(format!(
                        "unknown process step {other:?}; use spawn, exec, pid_reuse, job_control, group or foreground"
                    ));
                }
            };
            Ok(Event::Process {
                pane: whole(&v["pane"], "a process step's pane")?,
                what,
            })
        }
        "expect" => Ok(Event::Expect {
            pane: whole(&v["pane"], "an expectation's pane")?,
            track0: match v["track0"].as_str() {
                Some("possibly_remote") => Classification::PossiblyRemote,
                Some("local") => Classification::Local,
                _ => return Err("expect needs track0 possibly_remote or local".into()),
            },
            scope: match v["scope"].as_str() {
                None | Some("now") => Scope::Now,
                Some("frames_read_earlier") => Scope::FramesReadEarlier,
                Some(other) => {
                    return Err(format!(
                        "unknown scope {other:?}; use now or frames_read_earlier"
                    ));
                }
            },
        }),
        "detach" => Ok(Event::Detach),
        "reattach" => Ok(Event::Reattach),
        other => Err(format!(
            "unknown op {other:?}; use layout, output, container, replay, foreign, resize, zoom, detach, reattach, process or expect"
        )),
    }
}

fn case_of(v: &Value) -> Result<Case, String> {
    let id = v["id"].as_str().ok_or("case without id")?.to_owned();
    let ctx = |e: String| format!("{id}: {e}");
    let category = v["category"]
        .as_str()
        .ok_or_else(|| ctx("no category".into()))?
        .to_owned();
    let script = v["script"]
        .as_array()
        .ok_or_else(|| ctx("no script".into()))?
        .iter()
        .map(event_of)
        .collect::<Result<Vec<_>, _>>()
        .map_err(ctx)?;
    let forbid = match &v["forbid"] {
        Value::Null => Vec::new(),
        Value::Array(a) => a
            .iter()
            .map(|k| match k.as_str() {
                Some(k) if FORBID_KINDS.contains(&k) => Ok(k.to_owned()),
                Some(k) => Err(format!(
                    "{id}: unknown forbid kind {k:?}; known kinds are {FORBID_KINDS:?}"
                )),
                None => Err(format!("{id}: forbid entries must be strings")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(format!("{id}: forbid must be a list")),
    };
    Ok(Case {
        cols: whole(&v["screen"]["cols"], "screen.cols").map_err(ctx)?,
        rows: whole(&v["screen"]["rows"], "screen.rows").map_err(ctx)?,
        id,
        category,
        script,
        forbid,
    })
}

/// Parse one pane-corpus file's text into its revision and cases.
///
/// # Errors
///
/// Returns a message naming the case for a malformed one.
pub fn parse_file(text: &str) -> Result<(String, Vec<Case>), String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let revision = v["revision"]
        .as_str()
        .filter(|r| !r.is_empty())
        .ok_or("no revision")?
        .to_owned();
    let cases = v["cases"]
        .as_array()
        .ok_or("no cases list")?
        .iter()
        .map(case_of)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((revision, cases))
}

/// Load every `*.json` file in `dir`, in name order.
///
/// # Errors
///
/// Returns a message for an unreadable or malformed file, files that disagree
/// on the revision, a repeated case id, or an empty directory.
pub fn load_panes(dir: &Path) -> Result<PaneCorpus, String> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut revision: Option<String> = None;
    let mut cases: Vec<Case> = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        let (rev, mut more) = parse_file(&text).map_err(|e| format!("{}: {e}", f.display()))?;
        match &revision {
            Some(r) if *r != rev => {
                return Err(format!("{}: revision {rev} differs from {r}", f.display()));
            }
            _ => revision = Some(rev),
        }
        cases.append(&mut more);
    }
    let mut seen = BTreeSet::new();
    if let Some(c) = cases.iter().find(|c| !seen.insert(c.id.clone())) {
        return Err(format!("case id {} is used twice", c.id));
    }
    let revision = revision.ok_or_else(|| format!("{}: no corpus files", dir.display()))?;
    Ok(PaneCorpus { revision, cases })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(cases: &Value) -> String {
        json!({"revision": "pc-1", "cases": cases}).to_string()
    }

    fn case(script: &Value) -> Value {
        json!({"id": "c", "category": "t", "screen": {"cols": 80, "rows": 24}, "script": script})
    }

    fn parsed(cases: &Value) -> Result<(String, Vec<Case>), String> {
        parse_file(&file(cases))
    }

    #[test]
    fn every_op_loads_with_its_fields() {
        let c = case(&json!([
            {"op": "layout", "generation": 3, "panes": [
                {"id": 1, "x": 0, "y": 0, "w": 40, "h": 24},
                {"id": 2, "x": -1, "y": 0, "w": 40, "h": 24, "visible": false}]},
            {"op": "output", "pane": 1, "hex": "6869", "t_ms": 5},
            {"op": "output", "pane": 9, "fill": "41", "count": 3},
            {"op": "resize", "cols": 100, "rows": 30},
            {"op": "zoom", "pane": 2},
            {"op": "zoom"},
            {"op": "detach"},
            {"op": "reattach"}
        ]));
        let Ok((rev, cases)) = parsed(&json!([c])) else {
            unreachable!("loads")
        };
        assert_eq!(rev, "pc-1");
        let s = &cases[0].script;
        assert_eq!(s.len(), 8);
        assert!(
            matches!(&s[0], Event::Layout { generation: 3, panes } if panes.len() == 2
            && panes[1].x == -1 && !panes[1].visible && panes[0].visible)
        );
        assert!(
            matches!(&s[1], Event::Output { pane: 1, read } if read.bytes == b"hi" && read.t_ms == 5)
        );
        assert!(matches!(&s[2], Event::Output { pane: 9, read } if read.bytes == b"AAA"));
        assert_eq!(
            s[3],
            Event::Resize {
                cols: 100,
                rows: 30
            }
        );
        assert_eq!(s[4], Event::Zoom(Some(2)));
        assert_eq!(s[5], Event::Zoom(None));
        assert_eq!((cases[0].cols, cases[0].rows), (80, 24));
    }

    #[test]
    fn malformed_scripts_are_refused_with_the_case_id() {
        for bad in [
            json!([{"op": "explode"}]),
            json!([{"op": "layout", "generation": 1}]),
            json!([{"op": "layout", "generation": -1, "panes": []}]),
            json!([{"op": "layout", "generation": 1, "panes": [
                {"id": 1, "x": 0, "y": 0, "w": 1, "h": 1},
                {"id": 1, "x": 2, "y": 0, "w": 1, "h": 1}]}]),
            json!([{"op": "layout", "generation": 1, "panes": [{"id": 1, "x": 0, "y": 0, "w": -4, "h": 1}]}]),
            json!([{"op": "output", "pane": 1}]),
            json!([{"op": "resize", "cols": 1}]),
        ] {
            let Err(e) = parsed(&json!([case(&bad)])) else {
                unreachable!("{bad} must be refused")
            };
            assert!(e.starts_with("c: "), "{e}");
        }
    }

    #[test]
    fn process_and_expect_steps_load_with_their_fields() {
        let c = case(&json!([
            {"op": "process", "pane": 1, "what": "spawn", "program": "/bin/zsh", "listed": true},
            {"op": "process", "pane": 1, "what": "exec", "program": "/usr/bin/ssh"},
            {"op": "process", "pane": 1, "what": "pid_reuse"},
            {"op": "process", "pane": 1, "what": "job_control", "on": false},
            {"op": "process", "pane": 1, "what": "group", "members": 2},
            {"op": "process", "pane": 1, "what": "foreground", "program": "/usr/bin/ssh"},
            {"op": "expect", "pane": 1, "track0": "possibly_remote"},
            {"op": "expect", "pane": 1, "track0": "local", "scope": "frames_read_earlier"}
        ]));
        let Ok((_, cases)) = parsed(&json!([c])) else {
            unreachable!("loads")
        };
        let s = &cases[0].script;
        assert_eq!(s.len(), 8);
        assert!(
            matches!(&s[0], Event::Process { pane: 1, what: ProcessEvent::Spawn { program, listed: true } } if program == "/bin/zsh")
        );
        assert!(
            matches!(&s[1], Event::Process { what: ProcessEvent::Exec { program }, .. } if program == "/usr/bin/ssh")
        );
        assert!(matches!(
            &s[2],
            Event::Process {
                what: ProcessEvent::PidReuse,
                ..
            }
        ));
        assert!(matches!(
            &s[3],
            Event::Process {
                what: ProcessEvent::JobControl { on: false },
                ..
            }
        ));
        assert!(matches!(
            &s[4],
            Event::Process {
                what: ProcessEvent::Group { members: 2 },
                ..
            }
        ));
        assert!(matches!(
            &s[5],
            Event::Process {
                what: ProcessEvent::Foreground { .. },
                ..
            }
        ));
        assert!(matches!(
            &s[6],
            Event::Expect {
                track0: Classification::PossiblyRemote,
                scope: Scope::Now,
                ..
            }
        ));
        assert!(matches!(
            &s[7],
            Event::Expect {
                track0: Classification::Local,
                scope: Scope::FramesReadEarlier,
                ..
            }
        ));
        assert!(s.iter().all(Event::is_process_trust));
    }

    #[test]
    fn malformed_process_and_expect_steps_are_refused() {
        for bad in [
            json!([{"op": "process", "pane": 1, "what": "spawn", "program": "zsh", "listed": true}]),
            json!([{"op": "process", "pane": 1, "what": "spawn", "program": "/bin/zsh"}]),
            json!([{"op": "process", "pane": 1, "what": "exec"}]),
            json!([{"op": "process", "pane": 1, "what": "group", "members": 0}]),
            json!([{"op": "process", "pane": 1, "what": "job_control"}]),
            json!([{"op": "process", "pane": 1, "what": "teleport"}]),
            json!([{"op": "process", "what": "pid_reuse"}]),
            json!([{"op": "expect", "pane": 1, "track0": "mostly_local"}]),
            json!([{"op": "expect", "pane": 1}]),
            json!([{"op": "expect", "pane": 1, "track0": "local", "scope": "yesterday"}]),
        ] {
            let Err(e) = parsed(&json!([case(&bad)])) else {
                unreachable!("{bad} must be refused")
            };
            assert!(e.starts_with("c: "), "{e}");
        }
    }

    #[test]
    fn forbid_kinds_are_checked_so_a_typo_cannot_pass() {
        let mut c = case(&json!([]));
        c["forbid"] = json!(["draw_outside_rect", "key_reuse"]);
        assert!(parsed(&json!([c.clone()])).is_ok());
        c["forbid"] = json!(["draw_outside_rec"]);
        let Err(e) = parsed(&json!([c])) else {
            unreachable!("typo refused")
        };
        assert!(e.contains("unknown forbid kind"));
    }

    #[test]
    fn a_case_needs_a_screen_and_a_script() {
        let mut c = case(&json!([]));
        c["screen"] = Value::Null;
        assert!(parsed(&json!([c])).is_err());
        let mut c = case(&json!([]));
        c["script"] = Value::Null;
        assert!(parsed(&json!([c])).is_err());
    }
}
