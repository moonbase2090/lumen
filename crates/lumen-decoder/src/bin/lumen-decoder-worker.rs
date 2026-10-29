//! The decoder worker: one request on stdin, one reply on stdout, then exit.
//! The sandbox is entered before any input is read.

use std::io::{Read, Write};
use std::process::ExitCode;

use lumen_decoder::decode::decode;
use lumen_decoder::ffi_alloc::CappedAlloc;
use lumen_decoder::limits::MAX_INPUT_BYTES;
use lumen_decoder::limits::MEMORY_BYTES;
use lumen_decoder::protocol::{REQUEST_LEN, Reply, Request, Status};

#[global_allocator]
#[allow(
    clippy::cast_possible_truncation,
    reason = "512 MiB fits in usize on every target"
)]
static ALLOC: CappedAlloc = CappedAlloc::new(MEMORY_BYTES as usize);

fn reply_status(status: Status) -> ExitCode {
    let _ = std::io::stdout().write_all(&Reply::status_only(status).encode());
    let _ = std::io::stdout().flush();
    ExitCode::SUCCESS
}

fn run() -> ExitCode {
    #[cfg(unix)]
    if lumen_decoder::ffi_sandbox::enter().is_err() {
        // No sandbox, no work: leave before reading anything.
        return ExitCode::from(70);
    }
    #[cfg(not(unix))]
    return ExitCode::from(70);

    let mut stdin = std::io::stdin().lock();
    let mut head = [0u8; REQUEST_LEN];
    if stdin.read_exact(&mut head).is_err() {
        return reply_status(Status::Malformed);
    }
    let req = match Request::parse(&head) {
        Ok(r) => r,
        Err(s) => return reply_status(s),
    };
    if req.payload_len > MAX_INPUT_BYTES {
        return reply_status(Status::TooLarge);
    }
    let Ok(len) = usize::try_from(req.payload_len) else {
        return reply_status(Status::TooLarge);
    };
    let mut payload = Vec::new();
    if payload.try_reserve_exact(len).is_err() {
        return reply_status(Status::TooLarge);
    }
    if (&mut stdin)
        .take(req.payload_len)
        .read_to_end(&mut payload)
        .is_err()
        || payload.len() != len
    {
        return reply_status(Status::Malformed);
    }
    let mut extra = [0u8; 1];
    if matches!(stdin.read(&mut extra), Ok(n) if n > 0) {
        return reply_status(Status::Malformed);
    }
    match decode(req.format, &payload) {
        Ok(img) => {
            let stride = img.width.saturating_mul(4);
            let head = Reply {
                status: Status::Ok as u8,
                width: img.width,
                height: img.height,
                stride,
                data_len: img.rgba.len() as u64,
            };
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(&head.encode());
            let _ = out.write_all(&img.rgba);
            let _ = out.flush();
            ExitCode::SUCCESS
        }
        Err(s) => reply_status(s),
    }
}

fn main() -> ExitCode {
    run()
}
