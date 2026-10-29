//! The seal channel's rules without a process: message parsing, who is the
//! shell, and what the holder does with each message.

use lumen_holder::layer::{Layer, Outcome};
use lumen_holder::principal::{Creds, Principal, Verdict, judge, parse_start_time, start_time_in};
use lumen_holder::protocol::{self, MAX_MESSAGE, Malformed, Message};

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
    let mut layer = Layer::new(SHELL);
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
    let mut layer = Layer::new(SHELL);
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
    let mut layer = Layer::new(SHELL);
    assert_eq!(
        layer.handle(FROM_SHELL, &[9, 9, 9], false, &clock(99_000)),
        Outcome::Malformed(Malformed::Version)
    );
    assert!(!layer.is_ready());
    assert_eq!(layer.counters().malformed, 1);
}
