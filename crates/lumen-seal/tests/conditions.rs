//! Each sealing condition on its own, against a probe that reports what a
//! test says. Every row fails exactly one check and says which.

use lumen_seal::conditions::{capability_failure, is_identity_map};
use lumen_seal::{Decision, Probe, Reason, Sealer, Shell};
use std::cell::Cell;
use std::fmt::Write as _;
use std::os::fd::RawFd;

const SHELL_PATH: &str = "/bin/zsh";
const LISTED: [&str; 2] = ["/bin/zsh", "/bin/bash"];
const IDENTITY: &str = "         0          0 4294967295\n";
const NO_CAPS: &str = "Name:\tzsh\nCapInh:\t0000000000000000\nCapPrm:\t0000000000000000\nCapEff:\t0000000000000000\nCapBnd:\t000001ffffffffff\nCapAmb:\t0000000000000000\n";
const STAT: &str = "4242 (zsh) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 90000 20\n";

#[derive(Clone)]
struct Fake {
    linux: bool,
    yama: Option<u32>,
    tiocsti: Option<u32>,
    paranoid: Option<i64>,
    comm: Option<String>,
    cmdline: Option<String>,
    stat: Option<String>,
    uid_map: Option<String>,
    gid_map: Option<String>,
    status: Option<String>,
    namespace: Option<bool>,
    alive: Option<bool>,
    /// How many times the probe was asked anything.
    asked: Cell<u32>,
}

impl Fake {
    fn good() -> Self {
        Self {
            linux: true,
            yama: Some(1),
            tiocsti: Some(0),
            paranoid: Some(2),
            comm: Some("zsh\n".into()),
            cmdline: Some("/bin/zsh\0".into()),
            stat: Some(STAT.into()),
            uid_map: Some(IDENTITY.into()),
            gid_map: Some(IDENTITY.into()),
            status: Some(NO_CAPS.into()),
            namespace: Some(true),
            alive: Some(true),
            asked: Cell::new(0),
        }
    }
}

impl Probe for Fake {
    fn is_linux(&self) -> bool {
        self.asked.set(self.asked.get() + 1);
        self.linux
    }
    fn yama_scope(&self) -> Option<u32> {
        self.yama
    }
    fn legacy_tiocsti(&self) -> Option<u32> {
        self.tiocsti
    }
    fn perf_paranoid(&self) -> Option<i64> {
        self.paranoid
    }
    fn proc_text(&self, _pid: u32, name: &str) -> Option<String> {
        match name {
            "comm" => self.comm.clone(),
            "cmdline" => self.cmdline.clone(),
            "stat" => self.stat.clone(),
            "uid_map" => self.uid_map.clone(),
            "gid_map" => self.gid_map.clone(),
            "status" => self.status.clone(),
            _ => None,
        }
    }
    fn initial_user_namespace(&self, _pid: u32) -> Option<bool> {
        self.namespace
    }
    fn shell_alive(&self, _pidfd: Option<RawFd>) -> Option<bool> {
        self.alive
    }
}

fn shell() -> Shell<'static> {
    Shell {
        pid: 4242,
        pidfd: Some(9),
        path: SHELL_PATH,
        direct: true,
    }
}

fn ready(sealer: &Sealer, p: &Fake) -> Decision {
    sealer.ready(p, &shell(), &LISTED)
}

#[test]
fn every_condition_holding_seals() {
    assert_eq!(ready(&Sealer::new(), &Fake::good()), Decision::Seal);
    assert!(ready(&Sealer::new(), &Fake::good()).seals());
}

#[test]
#[allow(clippy::too_many_lines, reason = "one row per condition")]
fn each_failing_condition_is_grid_only_and_names_itself() {
    let cases: Vec<(&str, Fake, Reason)> = vec![
        (
            "not linux",
            Fake {
                linux: false,
                ..Fake::good()
            },
            Reason::NotLinux,
        ),
        (
            "yama 0",
            Fake {
                yama: Some(0),
                ..Fake::good()
            },
            Reason::YamaTooLow(0),
        ),
        (
            "yama unreadable",
            Fake {
                yama: None,
                ..Fake::good()
            },
            Reason::YamaUnreadable,
        ),
        (
            "paranoid 0",
            Fake {
                paranoid: Some(0),
                ..Fake::good()
            },
            Reason::PerfParanoidTooLow(0),
        ),
        (
            "paranoid -1",
            Fake {
                paranoid: Some(-1),
                ..Fake::good()
            },
            Reason::PerfParanoidTooLow(-1),
        ),
        (
            "paranoid unreadable",
            Fake {
                paranoid: None,
                ..Fake::good()
            },
            Reason::PerfParanoidUnreadable,
        ),
        (
            "tiocsti 1",
            Fake {
                tiocsti: Some(1),
                ..Fake::good()
            },
            Reason::TiocstiEnabled(1),
        ),
        (
            "tiocsti missing",
            Fake {
                tiocsti: None,
                ..Fake::good()
            },
            Reason::TiocstiUnreadable,
        ),
        (
            "comm differs",
            Fake {
                comm: Some("python\n".into()),
                ..Fake::good()
            },
            Reason::CommMismatch,
        ),
        (
            "comm unreadable",
            Fake {
                comm: None,
                ..Fake::good()
            },
            Reason::CommMismatch,
        ),
        (
            "other user namespace",
            Fake {
                namespace: Some(false),
                ..Fake::good()
            },
            Reason::UserNamespace,
        ),
        (
            "namespace unknown",
            Fake {
                namespace: None,
                ..Fake::good()
            },
            Reason::UserNamespace,
        ),
        (
            "uid_map shifted",
            Fake {
                uid_map: Some("0 1000 1\n".into()),
                ..Fake::good()
            },
            Reason::IdMap("uid_map"),
        ),
        (
            "uid_map unreadable",
            Fake {
                uid_map: None,
                ..Fake::good()
            },
            Reason::IdMap("uid_map"),
        ),
        (
            "gid_map shifted",
            Fake {
                gid_map: Some("0 1000 1\n".into()),
                ..Fake::good()
            },
            Reason::IdMap("gid_map"),
        ),
        (
            "status unreadable",
            Fake {
                status: None,
                ..Fake::good()
            },
            Reason::Capabilities("status"),
        ),
        (
            "shell gone",
            Fake {
                alive: Some(false),
                ..Fake::good()
            },
            Reason::ShellGone,
        ),
        (
            "no pidfd answer",
            Fake {
                alive: None,
                ..Fake::good()
            },
            Reason::ShellGone,
        ),
    ];
    for (name, probe, reason) in cases {
        assert_eq!(
            ready(&Sealer::new(), &probe),
            Decision::GridOnly(reason),
            "{name}"
        );
    }
}

#[test]
fn the_shell_must_be_run_directly_from_a_listed_absolute_path() {
    let sealer = Sealer::new();
    let p = Fake::good();
    let wrapper = Shell {
        direct: false,
        ..shell()
    };
    assert_eq!(
        sealer.ready(&p, &wrapper, &LISTED),
        Decision::GridOnly(Reason::NotDirect)
    );
    let other = Shell {
        path: "/usr/local/bin/fish",
        ..shell()
    };
    assert_eq!(
        sealer.ready(&p, &other, &LISTED),
        Decision::GridOnly(Reason::NotListed)
    );
    let relative = Shell {
        path: "zsh",
        ..shell()
    };
    assert_eq!(
        sealer.ready(&p, &relative, &["zsh"]),
        Decision::GridOnly(Reason::NotListed)
    );
    assert_eq!(
        sealer.ready(&p, &shell(), &[]),
        Decision::GridOnly(Reason::NotListed),
        "an empty list seals nothing"
    );
}

#[test]
fn comm_is_compared_as_the_kernel_keeps_it_fifteen_bytes() {
    let long = "/usr/bin/a-very-long-shell-name";
    let p = Fake {
        comm: Some("a-very-long-she\n".into()),
        ..Fake::good()
    };
    let s = Shell {
        path: long,
        ..shell()
    };
    assert_eq!(Sealer::new().ready(&p, &s, &[long]), Decision::Seal);
    let p = Fake {
        comm: Some("a-very-long-shx\n".into()),
        ..Fake::good()
    };
    assert_eq!(
        Sealer::new().ready(&p, &s, &[long]),
        Decision::GridOnly(Reason::CommMismatch)
    );
}

#[test]
fn a_ptrace_capability_in_any_of_the_four_sets_is_grid_only_and_the_bounding_set_is_not_used() {
    let ptrace = 1u64 << 19;
    for name in ["CapInh", "CapPrm", "CapEff", "CapAmb"] {
        let mut status = String::new();
        for field in ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"] {
            let value = if field == name { ptrace } else { 0 };
            let _ = writeln!(status, "{field}:\t{value:016x}");
        }
        let p = Fake {
            status: Some(status),
            ..Fake::good()
        };
        assert_eq!(
            ready(&Sealer::new(), &p),
            Decision::GridOnly(Reason::Capabilities(match name {
                "CapInh" => "CapInh",
                "CapPrm" => "CapPrm",
                "CapEff" => "CapEff",
                _ => "CapAmb",
            })),
            "{name}"
        );
    }
    // A full bounding set, as ordinary shells carry, is not a failure.
    let p = Fake {
        status: Some(NO_CAPS.replace("000001ffffffffff", "ffffffffffffffff")),
        ..Fake::good()
    };
    assert_eq!(ready(&Sealer::new(), &p), Decision::Seal);
}

#[test]
fn the_identity_map_is_parsed_as_numbers() {
    for good in [
        "0 0 4294967295",
        "         0          0 4294967295\n",
        "0\t0\t4294967295\n",
        " \t0 \t 0  \t4294967295 \n",
        "00 0 4294967295\n",
    ] {
        assert!(is_identity_map(good), "{good:?}");
    }
    for bad in [
        "",
        "\n",
        "0 0 4294967294\n",
        "0 1000 1\n",
        "1000 1000 1\n",
        "0 0 4294967295\n0 0 4294967295\n",
        "0 0 4294967295\n\n",
        "0 0\n",
        "0 0 4294967295 0\n",
        "0 0 0x100000000\n",
        "0 0 -1\n",
        "0 0 4294967295x\n",
        "0 0 4294967296\n",
        "0,0,4294967295\n",
    ] {
        assert!(!is_identity_map(bad), "{bad:?}");
    }
}

#[test]
fn capability_lines_must_be_present_and_hex() {
    assert_eq!(capability_failure(NO_CAPS), None);
    assert_eq!(capability_failure(""), Some("CapInh"));
    assert_eq!(capability_failure("CapInh:\tzz\n"), Some("CapInh"));
    let missing_amb = NO_CAPS.replace("CapAmb:\t0000000000000000\n", "");
    assert_eq!(capability_failure(&missing_amb), Some("CapAmb"));
    assert_eq!(
        capability_failure("CapInhX:\t0\nCapPrm:\t0\nCapEff:\t0\nCapAmb:\t0\n"),
        Some("CapInh")
    );
}

#[test]
fn a_value_below_the_floor_latches_every_layer_until_the_terminal_exits() {
    let sealer = Sealer::new();
    assert!(!sealer.latched());
    let low = Fake {
        yama: Some(0),
        ..Fake::good()
    };
    assert_eq!(
        ready(&sealer, &low),
        Decision::GridOnly(Reason::YamaTooLow(0))
    );
    assert!(sealer.latched());
    // The value rises, and a layer started later is still grid-only.
    assert_eq!(
        ready(&sealer, &Fake::good()),
        Decision::GridOnly(Reason::Latched)
    );
    assert_eq!(
        sealer.recheck(&Fake::good(), &shell()),
        Decision::GridOnly(Reason::Latched)
    );
}

#[test]
fn a_low_perf_paranoid_latches_too() {
    let sealer = Sealer::new();
    assert_eq!(
        ready(
            &sealer,
            &Fake {
                paranoid: Some(0),
                ..Fake::good()
            }
        ),
        Decision::GridOnly(Reason::PerfParanoidTooLow(0))
    );
    assert_eq!(
        ready(&sealer, &Fake::good()),
        Decision::GridOnly(Reason::Latched)
    );
}

#[test]
fn a_read_error_and_a_tiocsti_failure_do_not_latch() {
    for p in [
        Fake {
            yama: None,
            ..Fake::good()
        },
        Fake {
            paranoid: None,
            ..Fake::good()
        },
        Fake {
            tiocsti: Some(1),
            ..Fake::good()
        },
        Fake {
            tiocsti: None,
            ..Fake::good()
        },
        Fake {
            namespace: Some(false),
            ..Fake::good()
        },
    ] {
        let sealer = Sealer::new();
        assert!(!ready(&sealer, &p).seals());
        assert!(!sealer.latched());
        assert_eq!(
            ready(&sealer, &Fake::good()),
            Decision::Seal,
            "the next layer can seal"
        );
    }
}

#[test]
fn the_command_start_recheck_covers_the_settings_and_the_shell_and_not_the_rest() {
    let sealer = Sealer::new();
    assert_eq!(ready(&sealer, &Fake::good()), Decision::Seal);
    assert_eq!(sealer.recheck(&Fake::good(), &shell()), Decision::Seal);
    assert_eq!(
        sealer.recheck(
            &Fake {
                tiocsti: Some(1),
                ..Fake::good()
            },
            &shell()
        ),
        Decision::GridOnly(Reason::TiocstiEnabled(1))
    );
    assert_eq!(
        sealer.recheck(
            &Fake {
                alive: Some(false),
                ..Fake::good()
            },
            &shell()
        ),
        Decision::GridOnly(Reason::ShellGone)
    );
    assert_eq!(
        sealer.recheck(
            &Fake {
                paranoid: Some(-1),
                ..Fake::good()
            },
            &shell()
        ),
        Decision::GridOnly(Reason::PerfParanoidTooLow(-1))
    );
    assert!(
        sealer.latched(),
        "a low value seen at a command start latches"
    );
    // The start-only checks are not repeated.
    let sealer = Sealer::new();
    assert_eq!(ready(&sealer, &Fake::good()), Decision::Seal);
    let p = Fake {
        namespace: None,
        uid_map: None,
        status: None,
        ..Fake::good()
    };
    assert_eq!(sealer.recheck(&p, &shell()), Decision::Seal);
}

#[test]
fn the_command_start_recheck_refuses_a_shell_that_execed_another_program() {
    let sealer = Sealer::new();
    assert_eq!(ready(&sealer, &Fake::good()), Decision::Seal);

    let execed = Fake {
        comm: Some("python\n".into()),
        ..Fake::good()
    };
    assert_eq!(
        sealer.recheck(&execed, &shell()),
        Decision::GridOnly(Reason::CommMismatch)
    );
    assert_eq!(
        sealer.recheck(&Fake::good(), &shell()),
        Decision::GridOnly(Reason::ShellReplaced),
        "exec-ing back does not restore sealing"
    );
}

#[test]
fn the_command_start_recheck_compares_argv0_and_the_process_start_time() {
    for changed in [
        Fake {
            cmdline: Some("/usr/bin/python\0script.py\0".into()),
            ..Fake::good()
        },
        Fake {
            stat: Some(STAT.replace(" 90000 ", " 90001 ")),
            ..Fake::good()
        },
    ] {
        let sealer = Sealer::new();
        assert_eq!(ready(&sealer, &Fake::good()), Decision::Seal);
        assert_eq!(
            sealer.recheck(&changed, &shell()),
            Decision::GridOnly(Reason::ShellReplaced)
        );
    }
}

#[test]
fn an_unreadable_ready_point_identity_never_seals() {
    for unreadable in [
        Fake {
            cmdline: None,
            ..Fake::good()
        },
        Fake {
            stat: None,
            ..Fake::good()
        },
        Fake {
            cmdline: Some("\0".into()),
            ..Fake::good()
        },
    ] {
        assert_eq!(
            ready(&Sealer::new(), &unreadable),
            Decision::GridOnly(Reason::ShellIdentityUnreadable)
        );
    }
}

#[test]
fn an_exec_notice_is_latched_for_one_shell_without_disabling_another() {
    let sealer = Sealer::new();
    let other = Shell {
        pid: 4343,
        ..shell()
    };
    assert_eq!(ready(&sealer, &Fake::good()), Decision::Seal);
    assert_eq!(sealer.ready(&Fake::good(), &other, &LISTED), Decision::Seal);
    assert_eq!(
        sealer.recheck(
            &Fake {
                comm: Some("python\n".into()),
                ..Fake::good()
            },
            &shell()
        ),
        Decision::GridOnly(Reason::CommMismatch)
    );
    assert_eq!(sealer.recheck(&Fake::good(), &other), Decision::Seal);
}

#[test]
fn every_reason_has_a_notice_that_names_the_check() {
    for (reason, word) in [
        (Reason::NotLinux, "Linux"),
        (Reason::ShellIdentityUnreadable, "identity"),
        (Reason::ShellReplaced, "executed"),
        (Reason::YamaTooLow(0), "ptrace_scope"),
        (Reason::PerfParanoidTooLow(0), "perf_event_paranoid"),
        (Reason::TiocstiEnabled(1), "legacy_tiocsti"),
        (Reason::UserNamespace, "user namespace"),
        (Reason::IdMap("uid_map"), "uid_map"),
        (Reason::Capabilities("CapEff"), "CapEff"),
        (Reason::ShellGone, "gone"),
    ] {
        assert!(reason.to_string().contains(word), "{reason:?}");
    }
}
