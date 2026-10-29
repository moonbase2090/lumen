//! The Linux probe, against a directory built to look like `/proc` and
//! `/proc/sys`, so the reads are the same on every host.

use lumen_seal::{Decision, FsProbe, Probe, Reason, Sealer, Shell};
use std::path::{Path, PathBuf};

fn tree(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("seal-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    root
}

fn put(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    assert!(std::fs::create_dir_all(path.parent().unwrap_or(root)).is_ok());
    assert!(std::fs::write(path, text).is_ok());
}

fn probe(root: &Path) -> FsProbe {
    FsProbe::at(root.join("proc"), root.join("proc/sys"))
}

#[test]
fn sysctls_are_read_as_integers() {
    let root = tree("sysctl");
    put(&root, "proc/sys/kernel/yama/ptrace_scope", "1\n");
    put(&root, "proc/sys/dev/tty/legacy_tiocsti", "0\n");
    put(&root, "proc/sys/kernel/perf_event_paranoid", "-1\n");
    let p = probe(&root);
    assert_eq!(p.yama_scope(), Some(1));
    assert_eq!(p.legacy_tiocsti(), Some(0));
    assert_eq!(p.perf_paranoid(), Some(-1));
}

#[test]
fn a_missing_or_malformed_sysctl_is_none() {
    let root = tree("missing");
    let p = probe(&root);
    assert_eq!(
        (p.yama_scope(), p.legacy_tiocsti(), p.perf_paranoid()),
        (None, None, None)
    );
    for bad in ["", "\n", "x\n", "1 2\n", "1\n\n", " 1\n", "0x1\n", "1.0\n"] {
        put(&root, "proc/sys/kernel/yama/ptrace_scope", bad);
        assert_eq!(p.yama_scope(), None, "{bad:?}");
    }
    put(&root, "proc/sys/kernel/yama/ptrace_scope", "-1\n");
    assert_eq!(p.yama_scope(), None, "a negative scope is not a scope");
}

#[test]
fn per_process_files_are_read_by_pid_and_name_only() {
    let root = tree("procfiles");
    put(&root, "proc/77/comm", "zsh\n");
    put(&root, "proc/secret", "no");
    let p = probe(&root);
    assert_eq!(p.proc_text(77, "comm"), Some("zsh\n".into()));
    assert_eq!(p.proc_text(78, "comm"), None);
    for name in ["../secret", "a/b", "..", "../../secret"] {
        assert_eq!(p.proc_text(77, name), None, "{name}");
    }
}

#[test]
fn a_made_up_system_is_stopped_at_the_first_check_it_fails() {
    let shell = Shell {
        pid: 500,
        pidfd: None,
        path: "/bin/zsh",
        direct: true,
    };
    let files = [
        ("proc/sys/kernel/yama/ptrace_scope", "1\n"),
        ("proc/sys/dev/tty/legacy_tiocsti", "0\n"),
        ("proc/sys/kernel/perf_event_paranoid", "2\n"),
        ("proc/500/comm", "zsh\n"),
        ("proc/500/uid_map", "         0          0 4294967295\n"),
        ("proc/500/gid_map", "         0          0 4294967295\n"),
        (
            "proc/500/status",
            "CapInh:\t0000000000000000\nCapPrm:\t0000000000000000\nCapEff:\t0000000000000000\nCapAmb:\t0000000000000000\n",
        ),
    ];
    // The made-up tree has no namespace link and the shell has no pidfd, which
    // need a live kernel, so a complete tree is stopped at the namespace and
    // never seals; the file checks before it are what differ by missing file.
    let on_linux = FsProbe::default().is_linux();
    for skip in 0..=files.len() {
        let root = tree(&format!("whole-{skip}"));
        for (i, (rel, text)) in files.iter().enumerate() {
            if i != skip {
                put(&root, rel, text);
            }
        }
        let p = probe(&root);
        let expected = if on_linux {
            match skip {
                0 => Reason::YamaUnreadable,
                1 => Reason::TiocstiUnreadable,
                2 => Reason::PerfParanoidUnreadable,
                3 => Reason::CommMismatch,
                _ => Reason::UserNamespace,
            }
        } else {
            Reason::NotLinux
        };
        assert_eq!(
            Sealer::new().ready(&p, &shell, &["/bin/zsh"]),
            Decision::GridOnly(expected),
            "skip {skip}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn the_real_namespace_check_answers_for_this_process_on_linux() {
    let p = FsProbe::default();
    let pid = std::process::id();
    assert!(
        p.initial_user_namespace(pid).is_some(),
        "the ioctl and links answer"
    );
    assert_eq!(
        p.initial_user_namespace(u32::MAX),
        None,
        "a pid with no /proc entry is unknown"
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn off_linux_the_probe_is_not_linux_and_nothing_seals() {
    let p = FsProbe::default();
    assert!(!p.is_linux());
    let shell = Shell {
        pid: std::process::id(),
        pidfd: None,
        path: "/bin/zsh",
        direct: true,
    };
    assert_eq!(
        Sealer::new().ready(&p, &shell, &["/bin/zsh"]),
        Decision::GridOnly(Reason::NotLinux)
    );
}
