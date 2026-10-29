//! A deliberately misbehaving worker for the sandbox suite. Test-only and
//! never shipped. The first argument selects
//! the behaviour; modes starting with `raw-` skip the sandbox so the host's own
//! limits can be tested without the worker's.

use std::io::Write;
use std::process::{Command, ExitCode, Stdio};

use lumen_decoder::ffi_alloc::CappedAlloc;
use lumen_decoder::limits::MEMORY_BYTES;
use lumen_decoder::protocol::{Reply, Status};

#[global_allocator]
#[allow(
    clippy::cast_possible_truncation,
    reason = "512 MiB fits in usize on every target"
)]
static ALLOC: CappedAlloc = CappedAlloc::new(MEMORY_BYTES as usize);

fn ok_header(w: u32, h: u32, stride: u32, data_len: u64) -> [u8; 28] {
    Reply {
        status: Status::Ok as u8,
        width: w,
        height: h,
        stride,
        data_len,
    }
    .encode()
}

fn out(bytes: &[u8]) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(bytes);
    let _ = o.flush();
}

/// Reports whether an escape attempt got through: an image means it did.
fn report(escaped: bool) -> ExitCode {
    if escaped {
        out(&ok_header(1, 1, 4, 4));
        out(&[0, 0, 0, 255]);
    } else {
        out(&Reply::status_only(Status::Corrupt).encode());
    }
    ExitCode::SUCCESS
}

#[allow(clippy::too_many_lines, reason = "one arm per hostile mode")]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map_or("", String::as_str);
    // `profile-` modes enter the platform profile alone, so a denial there is
    // the profile's and not the descriptor or process limits'.
    let entered = if mode.starts_with("raw-") {
        Ok(())
    } else if mode.starts_with("profile-") {
        lumen_decoder::ffi_sandbox::enter_profile_only()
    } else {
        lumen_decoder::ffi_sandbox::enter()
    };
    if entered.is_err() {
        return ExitCode::from(70);
    }
    let mode = mode.strip_prefix("profile-").unwrap_or(mode);
    match mode {
        // Replies that lie about sizes. The hint in the suite is 1x1.
        "lie-over-reservation" => {
            out(&ok_header(100, 100, 400, 40_000));
            out(&vec![0u8; 40_000]);
        }
        "lie-over-limit" => {
            out(&ok_header(65535, 65535, 262_140, 65535 * 65535 * 4));
        }
        "lie-overflow" => {
            out(&ok_header(u32::MAX, u32::MAX, u32::MAX, u64::MAX));
        }
        "lie-stride" => {
            out(&ok_header(1, 1, 8, 4));
            out(&[0, 0, 0, 255]);
        }
        "lie-short" => {
            out(&ok_header(2, 2, 8, 16));
            out(&[0u8; 8]);
        }
        "lie-extra" => {
            out(&ok_header(1, 1, 4, 4));
            out(&[0u8; 100]);
        }
        "lie-status" => {
            let mut h = ok_header(0, 0, 0, 0);
            h[4] = 99;
            out(&h);
        }
        "lie-rejected-with-data" => {
            out(&Reply::status_only(Status::Corrupt).encode());
            out(&[1, 2, 3]);
        }
        "flood-stdout" => {
            let chunk = vec![0u8; 65536];
            let mut o = std::io::stdout().lock();
            while o.write_all(&chunk).is_ok() {}
        }
        "flood-stderr" => {
            let chunk = vec![b'x'; 65536];
            let mut e = std::io::stderr().lock();
            for _ in 0..800 {
                let _ = e.write_all(&chunk);
            }
            out(&ok_header(1, 1, 4, 4));
            out(&[9, 8, 7, 255]);
        }
        // Resource abuse, inside the sandbox.
        "spin" => loop {
            std::hint::black_box(0u64);
        },
        // Controls: the same sandbox, doing nothing forbidden.
        "control" => return report(true),
        "hog-small" | "hog" => {
            let steps = if mode == "hog" { 64 } else { 4 };
            let mut keep: Vec<Vec<u8>> = Vec::new();
            for _ in 0..steps {
                let mut v = vec![0u8; 32 * 1024 * 1024];
                for i in (0..v.len()).step_by(4096) {
                    v[i] = 1;
                }
                keep.push(v);
            }
            std::hint::black_box(&keep);
            return report(true);
        }
        // Escape attempts: `report(true)` means the attempt succeeded.
        "open-file" => return report(std::fs::File::open("/etc/hosts").is_ok()),
        "create-file" => {
            let p = args.get(2).map_or("escape-marker", String::as_str);
            return report(std::fs::File::create(p).is_ok());
        }
        "connect" => {
            // The suite passes the port of a listener it holds open, so a
            // success here means the sandbox let the connection through.
            let port: u16 = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(9);
            let ok = std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                std::time::Duration::from_millis(200),
            )
            .is_ok();
            return report(ok);
        }
        "udp" => return report(std::net::UdpSocket::bind("127.0.0.1:0").is_ok()),
        "exec" => {
            let ok = Command::new("/bin/echo")
                .stdout(Stdio::null())
                .status()
                .is_ok();
            return report(ok);
        }
        // Unsandboxed, to test the host.
        "raw-hang" => std::thread::sleep(std::time::Duration::from_secs(60)),
        "raw-grandchild" => {
            // Leaves a child that outlives the worker, holding stdout open.
            if let (Ok(child), Some(p)) =
                (Command::new("/bin/sleep").arg("60").spawn(), args.get(2))
            {
                let _ = std::fs::write(p, child.id().to_string());
            }
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        "raw-grandchild-exit" => {
            // The worker exits at once; the child keeps the pipes open.
            if let (Ok(child), Some(p)) =
                (Command::new("/bin/sleep").arg("60").spawn(), args.get(2))
            {
                let _ = std::fs::write(p, child.id().to_string());
            }
            out(&ok_header(1, 1, 4, 4));
            out(&[0, 0, 0, 255]);
        }
        "raw-exit-nonzero" => return ExitCode::from(3),
        _ => return ExitCode::from(64),
    }
    ExitCode::SUCCESS
}
