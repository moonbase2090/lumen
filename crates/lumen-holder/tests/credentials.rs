//! The seal channel's rules without a process: message parsing, who is the
//! shell, and what the holder does with each message.

use lumen_holder::layer::{KeyOutcome, Layer, Outcome};
use lumen_holder::principal::{
    Creds, Principal, Terminal, Verdict, judge, judge_terminal, parse_start_time, start_time_in,
};
use lumen_holder::protocol::{self, MAX_MESSAGE, Malformed, Message};

const TERMINAL: Terminal = Terminal {
    pid: 100,
    uid: 1000,
};

const FROM_TERMINAL: Option<Creds> = Some(Creds {
    pid: 100,
    uid: 1000,
    gid: 1000,
});

const SHELL: Principal = Principal {
    pid: 4242,
    start_time: 99_000,
    uid: 1000,
};

const FROM_SHELL: Option<Creds> = Some(Creds {
    pid: 4242,
    uid: 1000,
    gid: 1000,
});

fn clock(t: u64) -> impl Fn(u32) -> Option<u64> {
    move |_| Some(t)
}

#[test]
fn a_ready_message_is_version_type_and_a_big_endian_number() {
    assert_eq!(
        protocol::parse(&[1, 1, 0, 0, 1, 2], false),
        Ok(Message::Ready { number: 258 })
    );
    assert_eq!(protocol::ready(258), [1, 1, 0, 0, 1, 2]);
    assert_eq!(
        protocol::parse(&protocol::ready(u32::MAX), false),
        Ok(Message::Ready { number: u32::MAX })
    );
}

#[test]
fn every_other_shape_is_dropped_for_its_own_reason() {
    use Malformed::{Length, TooShort, Truncated, Type, Version};
    let cases: [(&[u8], bool, Malformed); 9] = [
        (&[], false, TooShort),
        (&[1], false, TooShort),
        (&[2, 1, 0, 0, 0, 1], false, Version),
        (&[0, 1, 0, 0, 0, 1], false, Version),
        (&[1, 0, 0, 0, 0, 1], false, Type),
        (&[1, 2, 0, 0, 0, 1], false, Type),
        (&[1, 1, 0, 0, 0], false, Length),
        (&[1, 1, 0, 0, 0, 1, 0], false, Length),
        (&[1, 1, 0, 0, 0, 1], true, Truncated),
    ];
    for (bytes, truncated, why) in cases {
        assert_eq!(protocol::parse(bytes, truncated), Err(why), "{bytes:?}");
    }
    assert_eq!(
        protocol::parse(&[1; MAX_MESSAGE + 1], false),
        Err(Truncated)
    );
}

#[test]
fn only_the_shell_by_pid_user_and_start_time_is_accepted() {
    assert_eq!(judge(&SHELL, FROM_SHELL, &clock(99_000)), Verdict::Accept);
    assert_eq!(judge(&SHELL, None, &clock(99_000)), Verdict::NoCredentials);
    let other = |pid, uid| {
        Some(Creds {
            pid,
            uid,
            gid: 1000,
        })
    };
    assert_eq!(
        judge(&SHELL, other(4243, 1000), &clock(99_000)),
        Verdict::WrongProcess
    );
    assert_eq!(
        judge(&SHELL, other(0, 1000), &clock(99_000)),
        Verdict::WrongProcess
    );
    assert_eq!(
        judge(&SHELL, other(-1, 1000), &clock(99_000)),
        Verdict::WrongProcess
    );
    assert_eq!(
        judge(&SHELL, other(4242, 0), &clock(99_000)),
        Verdict::WrongUser
    );
    assert_eq!(
        judge(&SHELL, FROM_SHELL, &clock(99_001)),
        Verdict::StartTimeChanged
    );
    assert_eq!(
        judge(&SHELL, FROM_SHELL, &|_| None),
        Verdict::StartTimeUnreadable
    );
}

#[test]
fn start_time_is_field_22_counted_from_the_last_parenthesis() {
    let stat = "4242 (zsh) S 1 4242 4242 34816 4242 4194304 1 0 0 0 1 2 0 0 20 0 1 0 99000 12345678 100 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0";
    assert_eq!(parse_start_time(stat), Some(99_000));
    let tricky = stat.replace("(zsh)", "(a) b (c d)");
    assert_eq!(
        parse_start_time(&tricky),
        Some(99_000),
        "a name with spaces and parentheses"
    );
    assert_eq!(parse_start_time("4242 (zsh) S 1"), None);
    assert_eq!(parse_start_time("no parenthesis"), None);
    assert_eq!(
        parse_start_time("1 (x) S 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 notanumber 1"),
        None
    );
}

#[test]
fn start_time_is_read_from_a_proc_tree() {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("holder-proc");
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(root.join("77")).is_ok());
    let stat = "77 (sh) S 1 77 77 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 31337 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
    assert!(std::fs::write(root.join("77/stat"), stat).is_ok());
    assert_eq!(start_time_in(&root, 77), Some(31_337));
    assert_eq!(start_time_in(&root, 78), None);
}

#[test]
fn the_first_ready_from_the_shell_is_accepted_and_nothing_is_ready_before_it() {
    let mut layer = Layer::new(SHELL, TERMINAL);
    assert!(!layer.is_ready());
    let ready = protocol::ready(1);
    assert_eq!(
        layer.handle(FROM_SHELL, &ready, false, &clock(99_000)),
        Outcome::Ready
    );
    assert!(layer.is_ready());
    assert_eq!(
        layer.handle(FROM_SHELL, &ready, false, &clock(99_000)),
        Outcome::SecondReady
    );
    let c = layer.counters();
    assert_eq!(
        (c.ready, c.second_ready, c.refused, c.malformed),
        (1, 1, 0, 0)
    );
}

#[test]
fn a_message_from_anyone_else_is_refused_before_it_is_read() {
    let mut layer = Layer::new(SHELL, TERMINAL);
    let not_the_shell = Some(Creds {
        pid: 5000,
        uid: 1000,
        gid: 1000,
    });
    // Even a good ready, and even garbage, is refused as a sender problem first.
    for bytes in [&protocol::ready(1)[..], &[0xff; 3][..], &[][..]] {
        assert_eq!(
            layer.handle(not_the_shell, bytes, false, &clock(99_000)),
            Outcome::Refused(Verdict::WrongProcess)
        );
    }
    assert!(
        !layer.is_ready(),
        "a refused sender cannot make the layer ready"
    );
    assert_eq!(layer.counters().refused, 3);
    assert_eq!(layer.counters().malformed, 0, "none of it was read");
}

#[test]
fn malformed_from_the_shell_is_counted_and_leaves_the_layer_not_ready() {
    let mut layer = Layer::new(SHELL, TERMINAL);
    assert_eq!(
        layer.handle(FROM_SHELL, &[9, 9, 9], false, &clock(99_000)),
        Outcome::Malformed(Malformed::Version)
    );
    assert!(!layer.is_ready());
    assert_eq!(layer.counters().malformed, 1);
}

const ROOT: [u8; 32] = [7; 32];

fn locks(ok: bool) -> impl Fn(&[u8]) -> bool {
    move |bytes| {
        assert_eq!(bytes, ROOT, "the memory asked to be locked is the key");
        ok
    }
}

#[test]
fn a_key_message_carries_a_number_a_layer_and_32_bytes() {
    let m = protocol::key_message(5, 9, &ROOT);
    assert_eq!(m.len(), protocol::KEY_LEN);
    assert_eq!(&m[..6], [1, 2, 0, 0, 0, 5]);
    let parsed = protocol::parse_key(&m[..], false);
    let Ok(parsed) = parsed else {
        unreachable!("a key message parses")
    };
    assert_eq!((parsed.number, parsed.layer, *parsed.root), (5, 9, ROOT));
    assert!(
        !format!("{parsed:?}").contains("7, 7"),
        "the key is not printed"
    );
}

#[test]
fn every_other_shape_of_key_message_is_dropped() {
    use Malformed::{Length, TooShort, Truncated, Type, Version};
    let good = protocol::key_message(1, 1, &ROOT);
    let mut long = good.to_vec();
    long.push(0);
    let mut wrong_version = good.to_vec();
    wrong_version[0] = 2;
    let mut wrong_type = good.to_vec();
    wrong_type[1] = 1;
    let cases: [(&[u8], bool, Malformed); 7] = [
        (&[], false, TooShort),
        (&good[..41], false, Length),
        (&long, false, Length),
        (&wrong_version, false, Version),
        (&wrong_type, false, Type),
        (&good[..], true, Truncated),
        (&[1; 200], false, Truncated),
    ];
    for (bytes, truncated, why) in cases {
        assert_eq!(
            protocol::parse_key(bytes, truncated).err(),
            Some(why),
            "{bytes:?}"
        );
    }
    // A key is not a seal-channel message and a ready is not a key.
    assert_eq!(protocol::parse(&good[..], false), Err(Type));
    assert_eq!(
        protocol::parse_key(&protocol::ready(1), false).err(),
        Some(Type)
    );
}

#[test]
fn only_the_terminal_by_pid_and_user_may_deliver_the_key() {
    assert_eq!(judge_terminal(&TERMINAL, FROM_TERMINAL), Verdict::Accept);
    assert_eq!(judge_terminal(&TERMINAL, None), Verdict::NoCredentials);
    let other = |pid, uid| {
        Some(Creds {
            pid,
            uid,
            gid: 1000,
        })
    };
    assert_eq!(
        judge_terminal(&TERMINAL, other(4242, 1000)),
        Verdict::WrongProcess
    );
    assert_eq!(
        judge_terminal(&TERMINAL, other(-100, 1000)),
        Verdict::WrongProcess
    );
    assert_eq!(judge_terminal(&TERMINAL, other(100, 0)), Verdict::WrongUser);
}

#[test]
fn the_key_is_accepted_from_the_terminal_once_and_locked() {
    let mut layer = Layer::new(SHELL, TERMINAL);
    assert!(!layer.has_key());
    let m = protocol::key_message(1, 12, &ROOT);
    assert_eq!(
        layer.handle_spawn(FROM_TERMINAL, &m[..], false, &locks(true)),
        KeyOutcome::Accepted { layer: 12 }
    );
    assert!(layer.has_key());
    assert_eq!(layer.key_layer(), Some(12));
    assert_eq!(layer.counters().locked, 1);
    let second = protocol::key_message(2, 13, &[9; 32]);
    assert_eq!(
        layer.handle_spawn(FROM_TERMINAL, &second[..], false, &|_| unreachable!(
            "a second key is never locked"
        )),
        KeyOutcome::Second
    );
    assert_eq!(layer.key_layer(), Some(12), "the first key stays");
    let c = layer.counters();
    assert_eq!(
        (c.key, c.second_key, c.refused_spawn, c.malformed_spawn),
        (1, 1, 0, 0)
    );
}

#[test]
fn a_key_that_could_not_be_locked_is_still_held_and_the_miss_is_recorded() {
    let mut layer = Layer::new(SHELL, TERMINAL);
    let m = protocol::key_message(1, 1, &ROOT);
    layer.handle_spawn(FROM_TERMINAL, &m[..], false, &locks(false));
    assert!(layer.has_key());
    assert_eq!(layer.counters().locked, 0);
}

#[test]
fn a_key_from_anyone_but_the_terminal_is_refused_unread_and_leaves_no_key() {
    let mut layer = Layer::new(SHELL, TERMINAL);
    let m = protocol::key_message(1, 1, &ROOT);
    // The shell itself has the same user and is still not the terminal.
    for creds in [
        FROM_SHELL,
        None,
        Some(Creds {
            pid: 100,
            uid: 0,
            gid: 0,
        }),
    ] {
        let out = layer.handle_spawn(creds, &m[..], false, &|_| {
            unreachable!("a refused key is never locked")
        });
        assert!(matches!(out, KeyOutcome::Refused(_)), "{out:?}");
    }
    assert_eq!(
        layer.handle_spawn(FROM_SHELL, &[0xff; 3], false, &|_| false),
        KeyOutcome::Refused(Verdict::WrongProcess),
        "garbage from a non-terminal is not read either"
    );
    assert!(!layer.has_key());
    assert_eq!(layer.counters().refused_spawn, 4);
    assert_eq!(layer.counters().malformed_spawn, 0);
}

#[test]
fn a_malformed_key_from_the_terminal_is_counted_and_does_not_use_up_the_key() {
    let mut layer = Layer::new(SHELL, TERMINAL);
    assert_eq!(
        layer.handle_spawn(FROM_TERMINAL, &[1, 2, 3], false, &|_| false),
        KeyOutcome::Malformed(Malformed::Length)
    );
    assert!(!layer.has_key());
    let m = protocol::key_message(1, 1, &ROOT);
    assert_eq!(
        layer.handle_spawn(FROM_TERMINAL, &m[..], false, &locks(true)),
        KeyOutcome::Accepted { layer: 1 }
    );
    assert_eq!(layer.counters().malformed_spawn, 1);
}
