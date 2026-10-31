//! Gate G9: an in-place shell replacement loses the seal descriptor and the
//! terminal records that the shell process image changed.
//!
//! The harness exercises both Linux execution entry points used by shells,
//! `execve` and `execveat`. The shell stand-in receives a seal socket on fd 2,
//! marks it close-on-exec as the shim must, and then replaces itself. The new
//! program proves fd 2 is closed. In parallel, the production sealing check
//! compares the ready-point process identity and returns a chrome-notice
//! reason for that shell. A passing result therefore requires both halves:
//! the replacement has no channel on which to request a frame, and the
//! terminal no longer treats the layer as sealed.

#![deny(unsafe_code)]

#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "named FFI module for fcntl and execveat; see its safety comments"
)]
mod ffi_linux;

use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod linux {
    use super::ffi_linux;
    use lumen_holder::ffi_linux::{pidfd_open, seqpacket_pair, wait_with_timeout};
    use lumen_seal::{Decision, FsProbe, Reason, Sealer, Shell};
    use std::ffi::CString;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, ExitCode, Stdio};

    const SEAL_FD: i32 = 2;
    const REPLACEMENT: &str =
        "if [ -e /proc/self/fd/2 ]; then echo seal-fd=open; else echo seal-fd=closed; fi; sleep 2";

    #[derive(Clone, Copy, Debug)]
    enum ExecForm {
        Execve,
        Execveat,
    }

    impl ExecForm {
        fn name(self) -> &'static str {
            match self {
                Self::Execve => "execve",
                Self::Execveat => "execveat",
            }
        }

        fn parse(text: &str) -> Option<Self> {
            match text {
                "execve" => Some(Self::Execve),
                "execveat" => Some(Self::Execveat),
                _ => None,
            }
        }
    }

    fn shell(form: ExecForm) -> ExitCode {
        if ffi_linux::set_cloexec(SEAL_FD).is_err() {
            return ExitCode::FAILURE;
        }
        println!("ready");
        let _ = std::io::stdout().flush();
        let mut start = String::new();
        if std::io::stdin().read_line(&mut start).is_err() {
            return ExitCode::FAILURE;
        }
        match form {
            ExecForm::Execve => {
                let error = Command::new("/bin/sh").args(["-c", REPLACEMENT]).exec();
                println!("execve failed: {error}");
            }
            ExecForm::Execveat => {
                let Ok(program) = CString::new("/bin/sh") else {
                    return ExitCode::FAILURE;
                };
                let Ok(arg0) = CString::new("sh") else {
                    return ExitCode::FAILURE;
                };
                let Ok(arg1) = CString::new("-c") else {
                    return ExitCode::FAILURE;
                };
                let Ok(arg2) = CString::new(REPLACEMENT) else {
                    return ExitCode::FAILURE;
                };
                println!(
                    "execveat failed: {}",
                    ffi_linux::execveat(&program, &[arg0, arg1, arg2])
                );
            }
        }
        ExitCode::FAILURE
    }

    fn line(reader: &mut BufReader<impl Read>) -> Option<String> {
        let mut text = String::new();
        (reader.read_line(&mut text).ok()? != 0).then(|| text.trim_end().to_owned())
    }

    fn end(child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    fn scenario(form: ExecForm) -> bool {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let Ok((seal_terminal, seal_shell)) = seqpacket_pair() else {
            return false;
        };
        let Ok(mut child) = Command::new(&exe)
            .args(["shell", form.name()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(seal_shell))
            .spawn()
        else {
            return false;
        };
        let Some(stdout) = child.stdout.take() else {
            end(&mut child);
            return false;
        };
        let mut output = BufReader::new(stdout);
        if line(&mut output).as_deref() != Some("ready") {
            end(&mut child);
            return false;
        }

        let Ok(pidfd) = pidfd_open(child.id()) else {
            end(&mut child);
            return false;
        };
        let Some(path) = exe.to_str() else {
            end(&mut child);
            return false;
        };
        let shell = Shell {
            pid: child.id(),
            pidfd: Some(pidfd.as_raw_fd()),
            path,
            direct: true,
        };
        let sealer = Sealer::new();
        let probe = FsProbe::default();
        let ready = sealer.record_process_identity(&probe, &shell);
        if ready != Decision::Seal {
            println!("G9 FAIL {} ready: {ready:?}", form.name());
            end(&mut child);
            return false;
        }
        let Some(mut input) = child.stdin.take() else {
            end(&mut child);
            return false;
        };
        let _ = input.write_all(b"exec\n");
        drop(input);
        let replacement = line(&mut output).unwrap_or_default();
        let decision = sealer.recheck_process_identity(&probe, &shell);
        let closed = wait_with_timeout(&[seal_terminal.as_raw_fd()], 1_000)
            .ok()
            .and_then(|events| events.first().copied())
            .is_some_and(|event| event.readable || event.hangup)
            && {
                let mut peer = std::fs::File::from(seal_terminal);
                let mut byte = [0u8; 1];
                peer.read(&mut byte).ok() == Some(0)
            };
        let noticed = matches!(
            decision,
            Decision::GridOnly(Reason::CommMismatch | Reason::ShellReplaced)
        );
        println!(
            "G9 {} {}: replacement={replacement:?} seal_closed={closed} notice={decision:?}",
            if closed && noticed && replacement == "seal-fd=closed" {
                "ok"
            } else {
                "FAIL"
            },
            form.name()
        );
        end(&mut child);
        closed && noticed && replacement == "seal-fd=closed"
    }

    pub fn run() -> ExitCode {
        let all = [ExecForm::Execve, ExecForm::Execveat]
            .into_iter()
            .all(scenario);
        println!("G9 {}", if all { "PASS" } else { "FAIL" });
        if all {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    pub fn role(arguments: &[String]) -> Option<ExitCode> {
        match arguments {
            [role, form] if role == "shell" => Some(shell(ExecForm::parse(form)?)),
            _ => None,
        }
    }
}

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        let arguments: Vec<String> = std::env::args().skip(1).collect();
        linux::role(&arguments).unwrap_or_else(linux::run)
    }
    #[cfg(not(target_os = "linux"))]
    {
        println!("G9 SKIP unsupported platform");
        ExitCode::FAILURE
    }
}
