//! Gate G1: the seal channel learns who sent each message from the kernel.
//!
//! On Linux every message received on a socket with `SO_PASSCRED` carries the
//! sender's process id, user id and group id as the kernel saw them at send
//! time, and an unprivileged process cannot send a different identity. This
//! program checks that on a `SOCK_SEQPACKET` pair, including a child that
//! inherited the socket and writes to it, and that forged claims are refused.
//!
//! macOS has no per-message credentials. The peer pid it reports follows the
//! last process to use the socket, so with two writers it can name the wrong
//! one. This program shows that too, so the difference is on record rather
//! than assumed.
//!
//! Run with no arguments. `child` is an internal role.

use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod linux {
    use lumen_gates::ffi_cred::{
        Creds, enable_passcred, peer_pid, recv, send, socketpair_seqpacket,
    };
    use std::io;
    use std::os::fd::OwnedFd;
    use std::process::{Command, ExitCode, Stdio};

    fn id() -> Creds {
        Creds {
            pid: i32::try_from(std::process::id()).unwrap_or(-1),
            uid: lumen_gates::ffi_mem::effective_uid(),
            gid: lumen_gates::ffi_cred::effective_gid(),
        }
    }

    /// The child: stdin is the sending end. Send one message and exit.
    pub fn child() -> ExitCode {
        let fd = lumen_gates::ffi_cred::stdin_owned();
        match send(&fd, b"from child", None) {
            Ok(_) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        }
    }

    fn check(name: &str, ok: bool, detail: &str) -> bool {
        println!("G1 {} {name}: {detail}", if ok { "ok  " } else { "FAIL" });
        ok
    }

    pub fn run() -> ExitCode {
        let me = id();
        println!(
            "G1 sender identity pid={} uid={} gid={}",
            me.pid, me.uid, me.gid
        );
        if me.uid == 0 {
            println!("G1 FAIL run as an unprivileged user: root may send any identity");
            return ExitCode::FAILURE;
        }
        let Ok((tx, rx)) = socketpair_seqpacket() else {
            println!("G1 FAIL socketpair");
            return ExitCode::FAILURE;
        };
        if enable_passcred(&rx).is_err() {
            println!("G1 FAIL SO_PASSCRED");
            return ExitCode::FAILURE;
        }
        let mut all = true;
        let mut buf = [0u8; 64];

        for i in 0..3 {
            let sent = send(&tx, b"hello", None);
            let got = recv(&rx, &mut buf);
            let ok = matches!((&sent, &got), (Ok(5), Ok((5, Some(c)))) if *c == me);
            all &= check(
                &format!("message {i} carries the sender"),
                ok,
                &format!("{got:?}"),
            );
        }

        all &= forged(&tx, &rx, me, &mut buf);
        all &= inherited(tx, &rx, me, &mut buf);

        println!("G1 {}", if all { "PASS" } else { "FAIL" });
        if all {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    fn forged(tx: &OwnedFd, rx: &OwnedFd, me: Creds, buf: &mut [u8]) -> bool {
        let mut all = true;
        let bad = [
            ("pid 1", Creds { pid: 1, ..me }),
            ("uid 0", Creds { uid: 0, ..me }),
            ("gid 0", Creds { gid: 0, ..me }),
        ];
        for (name, claim) in bad {
            let r = send(tx, b"forged", Some(claim));
            let eperm = matches!(&r, Err(e) if e.kind() == io::ErrorKind::PermissionDenied);
            all &= check(
                &format!("forged {name} is refused"),
                eperm,
                &format!("{r:?}"),
            );
        }
        let honest = send(tx, b"honest", Some(me));
        let got = recv(rx, buf);
        let ok = honest.is_ok() && matches!(&got, Ok((6, Some(c))) if *c == me);
        all &= check(
            "an honest explicit claim goes through",
            ok,
            &format!("{got:?}"),
        );
        all
    }

    fn inherited(tx: OwnedFd, rx: &OwnedFd, me: Creds, buf: &mut [u8]) -> bool {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let child = Command::new(exe)
            .arg("child")
            .stdin(Stdio::from(tx))
            .spawn();
        let Ok(mut child) = child else {
            return check("child spawn", false, "spawn failed");
        };
        let child_pid = i32::try_from(child.id()).unwrap_or(-1);
        let got = recv(rx, buf);
        let _ = child.wait();
        let per_message = matches!(&got, Ok((10, Some(c)))
            if c.pid == child_pid && c.uid == me.uid && c.pid != me.pid);
        let mut all = check(
            "a child writing on an inherited socket is seen as the child",
            per_message,
            &format!("child pid {child_pid}, got {got:?}"),
        );
        let creator = peer_pid(rx).ok();
        all &= check(
            "the connection-time identity names the creator, not the writer",
            creator == Some(me.pid),
            &format!("SO_PEERCRED pid {creator:?}, writer pid {child_pid}"),
        );
        all
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use lumen_gates::ffi_cred::peer_pid;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::process::{Command, ExitCode, Stdio};

    pub fn child() -> ExitCode {
        let mut s = UnixStream::from(lumen_gates::ffi_cred::stdin_owned());
        let _ = s.write_all(b"x");
        ExitCode::SUCCESS
    }

    pub fn run() -> ExitCode {
        let me = std::process::id();
        let Ok((a, b)) = UnixStream::pair() else {
            println!("G1 FAIL socketpair");
            return ExitCode::FAILURE;
        };
        let Ok(exe) = std::env::current_exe() else {
            return ExitCode::FAILURE;
        };
        let Ok(b_for_child) = b.try_clone() else {
            return ExitCode::FAILURE;
        };
        // The parent writes first, then the child writes on the same socket.
        let mut writer = &b;
        if writer.write_all(b"p").is_err() {
            println!("G1 FAIL parent write");
            return ExitCode::FAILURE;
        }
        let child = Command::new(exe)
            .arg("child")
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(b_for_child)))
            .spawn();
        let Ok(mut child) = child else {
            println!("G1 FAIL spawn");
            return ExitCode::FAILURE;
        };
        let child_pid = child.id();
        let _ = child.wait();
        // The first byte in the queue came from the parent. Ask who the peer
        // is now.
        let mut byte = [0u8; 1];
        let _ = (&a).read(&mut byte);
        let seen = peer_pid(&a).ok();
        println!(
            "G1 info first message sent by pid {me} (parent), later message by pid {child_pid} (child); LOCAL_PEERPID asked after reading the first says {seen:?}"
        );
        let misattributed = seen == Some(i32::try_from(child_pid).unwrap_or(-1));
        println!(
            "G1 {} macOS has no per-message sender: the peer pid follows the last process to use the socket, so it named the wrong sender for the first message",
            if misattributed { "PASS" } else { "FAIL" }
        );
        if misattributed {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let child = std::env::args().nth(1).as_deref() == Some("child");
    #[cfg(target_os = "linux")]
    {
        if child { linux::child() } else { linux::run() }
    }
    #[cfg(target_os = "macos")]
    {
        if child { macos::child() } else { macos::run() }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = child;
        println!("G1 SKIP unsupported platform");
        ExitCode::FAILURE
    }
}
