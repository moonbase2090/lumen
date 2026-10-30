//! The `pp-1` corpus: how a pane's process affects the trust of its Track 0.
//!
//! These are the process-state cases of the pane gate: a pane whose first
//! command is not a listed shell, an exec in the pane's shell, a recorded pid
//! that now belongs to another process, job control off, a second process in
//! the foreground group, and a frame read while `ssh` was in the foreground.
//!
//! In v0 all of a pane's Track 0 is possibly remote (`spec/v0/multiplexers.md`,
//! pane containers), and the local signal for panes is not part of v0, so there
//! is no case in which a pane is expected to be local: every expectation here
//! is `possibly_remote`, including the one where every old condition holds. The
//! cases can only fail an implementation that lets some process state make a
//! pane's Track 0 local.
//!
//! Placement A replays the process observations through the terminal's pane
//! process-state model and checks each read-time or current expectation.
//! Placement B has no per-pane process state and reports these cases as not
//! run. The committed JSON file is the output of [`pp1_json`]; a test fails if
//! the two differ.

use lumen_detector::TAG;
use serde_json::{Value, json};
use std::fmt::Write as _;

/// The corpus revision.
pub const REVISION: &str = "pp-1";

const ZSH: &str = "/bin/zsh";
const VIM: &str = "/usr/bin/vim";
const SSH: &str = "/usr/bin/ssh";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn paragraph(text: &str) -> Vec<u8> {
    let mut body = u16::try_from(text.len())
        .unwrap_or(0)
        .to_be_bytes()
        .to_vec();
    body.extend_from_slice(text.as_bytes());
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(0x80);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend(body);
    v
}

fn layout() -> Value {
    json!({"op": "layout", "generation": 1, "panes": [
        {"id": 1, "x": 0, "y": 0, "w": 80, "h": 24}
    ]})
}

fn spawn(program: &str, listed: bool) -> Value {
    json!({"op": "process", "pane": 1, "what": "spawn", "program": program, "listed": listed})
}

fn process(what: &str, extra: &Value) -> Value {
    let mut v = json!({"op": "process", "pane": 1, "what": what});
    if let (Some(v), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        v.extend(e.clone());
    }
    v
}

fn frame(text: &str) -> Value {
    json!({"op": "output", "pane": 1, "hex": hex(&paragraph(text))})
}

fn remote() -> Value {
    json!({"op": "expect", "pane": 1, "track0": "possibly_remote"})
}

fn remote_earlier() -> Value {
    json!({"op": "expect", "pane": 1, "track0": "possibly_remote", "scope": "frames_read_earlier"})
}

fn case(id: &str, note: &str, script: &[Value]) -> Value {
    json!({
        "id": id, "category": "process-trust", "note": note,
        "screen": {"cols": 80, "rows": 24}, "script": script
    })
}

/// The corpus as pretty-printed JSON with a trailing newline.
#[must_use]
#[allow(clippy::too_many_lines, reason = "one flat list of corpus cases")]
pub fn pp1_json() -> String {
    let cases = vec![
        case(
            "first-command-not-a-listed-shell",
            "A pane whose first command is not a listed shell is never local.",
            &[
                layout(),
                spawn(VIM, false),
                frame("from an editor"),
                remote(),
            ],
        ),
        case(
            "listed-shell-with-every-condition-holding",
            "No process state makes a pane local in v0: even the clean case is possibly remote.",
            &[
                layout(),
                spawn(ZSH, true),
                process("job_control", &json!({"on": true})),
                process("group", &json!({"members": 1})),
                frame("from a clean shell"),
                remote(),
            ],
        ),
        case(
            "exec-in-the-pane-shell",
            "An exec by the foreground leader does not make the pane local, and exec-ing back does not undo it.",
            &[
                layout(),
                spawn(ZSH, true),
                process("exec", &json!({"program": SSH})),
                frame("after the exec"),
                remote(),
                process("exec", &json!({"program": ZSH})),
                frame("after exec-ing back"),
                remote(),
            ],
        ),
        case(
            "recorded-pid-reused-by-another-process",
            "A pid or start-time mismatch with what was recorded at spawn leaves the pane possibly remote.",
            &[
                layout(),
                spawn(ZSH, true),
                process("pid_reuse", &json!({})),
                frame("from the new owner of the pid"),
                remote(),
            ],
        ),
        case(
            "job-control-off",
            "Turning job control off leaves the pane possibly remote, and turning it on again does not change that.",
            &[
                layout(),
                spawn(ZSH, true),
                process("job_control", &json!({"on": false})),
                frame("with job control off"),
                remote(),
                process("job_control", &json!({"on": true})),
                frame("with job control back on"),
                remote(),
            ],
        ),
        case(
            "second-process-in-the-foreground-group",
            "A second process in the foreground group leaves the pane possibly remote, also after it leaves.",
            &[
                layout(),
                spawn(ZSH, true),
                process("group", &json!({"members": 2})),
                frame("shared with a second process"),
                remote(),
                process("group", &json!({"members": 1})),
                frame("after it left"),
                remote(),
            ],
        ),
        case(
            "frame-read-while-ssh-was-in-the-foreground",
            "A frame read while ssh was in the foreground stays possibly remote after the shell returns.",
            &[
                layout(),
                spawn(ZSH, true),
                process("foreground", &json!({"program": SSH})),
                frame("typed by the remote side"),
                process("foreground", &json!({"program": ZSH})),
                remote_earlier(),
                frame("from the shell again"),
                remote(),
            ],
        ),
        case(
            "track-0-printed-by-the-shell-itself",
            "Anything the shell itself prints is possibly remote.",
            &[
                layout(),
                spawn(ZSH, true),
                process("group", &json!({"members": 1})),
                frame("printed by the shell"),
                remote(),
            ],
        ),
    ];
    let mut out = serde_json::to_string_pretty(&json!({"revision": REVISION, "cases": cases}))
        .unwrap_or_default();
    out.push('\n');
    out
}
