//! Host side: starts one worker per image and bounds everything it reads. The
//! reader holds at most the reserved pixel bytes plus 4 KiB, which the caller
//! has already reserved from the budget; the reply is validated before the
//! image is handed on or uploaded.
//!
//! The worker is hostile by assumption. A status other than `Ok` is a
//! rejection; anything else that is not a valid reply is a crash.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::limits::{self, MAX_INPUT_BYTES, READ_SLACK, STDERR_KEEP, WALL_MILLIS};
use crate::protocol::{Format, REPLY_LEN, Reply, Request, Status};

/// Starting and stopping a worker. On Unix it is a process group; on Windows
/// it is an `AppContainer` and a job object (see `ffi_win_spawn`).
#[cfg(unix)]
mod os {
    use std::io::{self, Read, Write};
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    use crate::ffi_process::kill_group;

    pub struct Worker {
        child: Child,
        pid: u32,
    }

    pub type Pipes = (
        Box<dyn Write + Send>,
        Box<dyn Read + Send>,
        Box<dyn Read + Send>,
    );

    pub fn spawn(worker: &Path, args: &[String]) -> io::Result<(Worker, Pipes)> {
        let mut cmd = Command::new(worker);
        cmd.args(args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = cmd.spawn()?;
        let pid = child.id();
        let (Some(i), Some(o), Some(e)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = kill_group(pid);
            let _ = child.wait();
            return Err(io::Error::other("worker pipes missing"));
        };
        Ok((
            Worker { child, pid },
            (Box::new(i), Box::new(o), Box::new(e)),
        ))
    }

    impl Worker {
        pub fn try_wait(&mut self) -> io::Result<Option<bool>> {
            Ok(self.child.try_wait()?.map(|s| s.success()))
        }

        /// Kills the whole group, which also takes down anything the worker
        /// left holding a pipe open.
        pub fn kill(&mut self) {
            let _ = kill_group(self.pid);
        }

        pub fn reap(&mut self) {
            let _ = self.child.wait();
        }
    }
}

#[cfg(windows)]
mod os {
    use std::io::{self, Read, Write};
    use std::path::Path;

    use crate::ffi_win_spawn::{self, Worker as Job};

    pub struct Worker(Job);

    pub type Pipes = (
        Box<dyn Write + Send>,
        Box<dyn Read + Send>,
        Box<dyn Read + Send>,
    );

    pub fn spawn(worker: &Path, args: &[String]) -> io::Result<(Worker, Pipes)> {
        let s = ffi_win_spawn::spawn(worker, args)?;
        Ok((
            Worker(s.worker),
            (Box::new(s.stdin), Box::new(s.stdout), Box::new(s.stderr)),
        ))
    }

    impl Worker {
        pub fn try_wait(&mut self) -> io::Result<Option<bool>> {
            self.0.try_wait()
        }

        /// Kills every process in the job.
        pub fn kill(&mut self) {
            self.0.kill();
        }

        #[allow(
            clippy::unused_self,
            reason = "the same shape as the Unix worker, which waits here"
        )]
        pub fn reap(&mut self) {}
    }
}

/// A validated image, ready to upload.
#[derive(Debug, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of premultiplied, top-down RGBA.
    pub rgba: Vec<u8>,
}

/// Why a worker counts as a crash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crash {
    /// The worker could not be started.
    Spawn,
    /// It did not finish within the wall-clock limit.
    Timeout,
    /// It died by signal or exited non-zero without a valid reply.
    Exit,
    /// It wrote more than the reservation allows.
    Overflow,
    /// It wrote fewer bytes than a header, or fewer than it announced.
    Short,
    /// The reply failed validation.
    BadReply,
}

/// What a decode came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A validated image.
    Image(Image),
    /// The worker declined with a status; show the alt text, not a crash.
    Rejected(Status),
    /// The input was refused before a worker was started.
    Refused,
    /// The worker crashed; the producer's images stay on alt text.
    Crash(Crash),
}

/// Starts workers.
#[derive(Clone, Debug)]
pub struct Runner {
    worker: PathBuf,
    args: Vec<String>,
    wall: Duration,
}

impl Runner {
    /// A runner for the worker executable at `worker`.
    #[must_use]
    pub fn new(worker: impl Into<PathBuf>) -> Self {
        Self {
            worker: worker.into(),
            args: Vec::new(),
            wall: Duration::from_millis(WALL_MILLIS),
        }
    }

    /// Passes arguments to the worker. The real worker takes none; the test
    /// suite uses this to select a hostile mode.
    #[must_use]
    pub fn with_args(mut self, args: &[&str]) -> Self {
        self.args = args.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// Replaces the wall-clock limit. The default is the contract's 4 s; the
    /// test suite lengthens it for decodes that must succeed on a loaded host.
    #[must_use]
    pub fn with_wall(mut self, wall: Duration) -> Self {
        self.wall = wall;
        self
    }

    /// The worker path.
    #[must_use]
    pub fn worker(&self) -> &Path {
        &self.worker
    }

    /// Decodes `payload` in a fresh worker. The hint is the size the producer
    /// declared; it fixes the reservation and the read cap.
    #[must_use]
    pub fn decode(&self, format: Format, hint: (u32, u32), payload: &[u8]) -> Outcome {
        if Format::sniff(payload) != Some(format) || payload.len() as u64 > MAX_INPUT_BYTES {
            return Outcome::Refused;
        }
        let Some(reserved) = limits::rgba_bytes(u64::from(hint.0), u64::from(hint.1)) else {
            return Outcome::Refused;
        };
        let cap = reserved + READ_SLACK;
        let req = Request {
            format,
            hint_width: hint.0,
            hint_height: hint.1,
            payload_len: payload.len() as u64,
        };
        self.run(&req, payload, reserved, cap)
    }

    fn run(&self, req: &Request, payload: &[u8], reserved: u64, cap: u64) -> Outcome {
        let Ok((mut child, (mut stdin, mut stdout, mut stderr))) =
            os::spawn(&self.worker, &self.args)
        else {
            return Outcome::Crash(Crash::Spawn);
        };

        let head = req.encode();
        let body = payload.to_vec();
        let writer = thread::spawn(move || {
            // A worker that stops reading closes the pipe; that is not an error here.
            let _ = stdin.write_all(&head).and_then(|()| stdin.write_all(&body));
        });
        // Reads one byte past the cap so an overrun is seen, and never more.
        let reader = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = (&mut stdout).take(cap + 1).read_to_end(&mut buf);
            buf
        });
        // Keeps the first bytes and drops the rest, so a flood neither blocks
        // the worker nor grows memory.
        let drain = thread::spawn(move || {
            let mut kept = Vec::new();
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                if kept.len() < STDERR_KEEP {
                    let room = STDERR_KEEP - kept.len();
                    kept.extend_from_slice(&chunk[..n.min(room)]);
                }
            }
            kept
        });

        let deadline = Instant::now() + self.wall;
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(ok)) => break Some(ok),
                Ok(None) if Instant::now() >= deadline => {
                    timed_out = true;
                    break None;
                }
                Ok(None) => thread::sleep(Duration::from_millis(2)),
                Err(_) => break None,
            }
        };
        // Always kill the group: it also takes down anything the worker left
        // holding a pipe open, so the joins below cannot hang on a grandchild.
        child.kill();
        child.reap();
        let _ = writer.join();
        let out = reader.join().unwrap_or_default();
        let _ = drain.join();

        if timed_out {
            return Outcome::Crash(Crash::Timeout);
        }
        validate(&out, status == Some(true), reserved, cap)
    }
}

/// Judges everything a worker wrote. `out` is stdout as read (at most `cap + 1`
/// bytes), `exited_ok` is whether it exited with status 0, `reserved` the pixel
/// bytes reserved for the image and `cap` the read cap. Public so the fuzz
/// target can drive it with arbitrary bytes.
#[must_use]
pub fn validate(out: &[u8], exited_ok: bool, reserved: u64, cap: u64) -> Outcome {
    if out.len() as u64 > cap {
        return Outcome::Crash(Crash::Overflow);
    }
    let Some(head) = out.get(..REPLY_LEN) else {
        return Outcome::Crash(if exited_ok { Crash::Short } else { Crash::Exit });
    };
    let mut h = [0u8; REPLY_LEN];
    h.copy_from_slice(head);
    let Some(reply) = Reply::parse(&h) else {
        return Outcome::Crash(Crash::BadReply);
    };
    let body = &out[REPLY_LEN..];
    if !exited_ok {
        return Outcome::Crash(Crash::Exit);
    }
    match Status::from_byte(reply.status) {
        None => Outcome::Crash(Crash::BadReply),
        Some(Status::Ok) => validate_image(&reply, body, reserved),
        Some(s) => {
            let zero =
                reply.width == 0 && reply.height == 0 && reply.stride == 0 && reply.data_len == 0;
            if zero && body.is_empty() {
                Outcome::Rejected(s)
            } else {
                Outcome::Crash(Crash::BadReply)
            }
        }
    }
}

fn validate_image(reply: &Reply, body: &[u8], reserved: u64) -> Outcome {
    let (w, h) = (u64::from(reply.width), u64::from(reply.height));
    let Some(rgba_len) = limits::rgba_bytes(w, h) else {
        return Outcome::Crash(Crash::BadReply);
    };
    let Some(stride) = w.checked_mul(4) else {
        return Outcome::Crash(Crash::BadReply);
    };
    if u64::from(reply.stride) != stride || reply.data_len != rgba_len || rgba_len > reserved {
        return Outcome::Crash(Crash::BadReply);
    }
    if (body.len() as u64) < reply.data_len {
        return Outcome::Crash(Crash::Short);
    }
    if body.len() as u64 != reply.data_len {
        return Outcome::Crash(Crash::BadReply);
    }
    Outcome::Image(Image {
        width: reply.width,
        height: reply.height,
        rgba: body.to_vec(),
    })
}
