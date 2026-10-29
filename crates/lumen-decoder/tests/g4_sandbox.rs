//! G4 sandbox suite: the real worker and the real host runner, plus a hostile
//! worker that lies, floods, hangs and tries to escape.

#![cfg(unix)]
#![allow(missing_docs)]

use std::error::Error;
use std::time::{Duration, Instant};

use lumen_decoder::protocol::{Format, Status};
use lumen_decoder::runner::{Crash, Image, Outcome, Runner};

type R = Result<(), Box<dyn Error>>;

const WORKER: &str = env!("CARGO_BIN_EXE_lumen-decoder-worker");

/// The real worker. Its wall-clock limit is long here: a decode that must
/// succeed should not fail because the host running the suite is busy. The
/// hostile tests below keep the contract's 4 s.
fn real() -> Runner {
    Runner::new(WORKER).with_wall(Duration::from_secs(60))
}

fn pixels(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
    (0..w * h).flat_map(|_| px).collect()
}

fn png_rgba(w: u32, h: u32, px: [u8; 4]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header()?;
    wr.write_image_data(&pixels(w, h, px))?;
    drop(wr);
    Ok(out)
}

fn jpeg_rgba(w: u16, h: u16, px: [u8; 4]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, 100);
    enc.encode(
        &pixels(u32::from(w), u32::from(h), px),
        w,
        h,
        jpeg_encoder::ColorType::Rgba,
    )?;
    Ok(out)
}

fn webp_rgba(w: u32, h: u32, px: [u8; 4]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out).encode(
        &pixels(w, h, px),
        w,
        h,
        image_webp::ColorType::Rgba8,
    )?;
    Ok(out)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(kind: [u8; 4], body: &[u8]) -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    let mut tagged = kind.to_vec();
    tagged.extend_from_slice(body);
    c.extend_from_slice(&tagged);
    c.extend_from_slice(&crc32(&tagged).to_be_bytes());
    c
}

/// A PNG whose header declares `w` x `h` and whose data is a few bytes.
fn png_declaring(w: u32, h: u32) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(*b"IHDR", &ihdr));
    out.extend(chunk(
        *b"IDAT",
        &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    ));
    out.extend(chunk(*b"IEND", &[]));
    out
}

fn expect_image(o: Outcome) -> Result<Image, Box<dyn Error>> {
    match o {
        Outcome::Image(i) => Ok(i),
        other => Err(format!("expected an image, got {other:?}").into()),
    }
}

// ---- the real worker decodes real images -------------------------------

#[test]
fn png_decodes_to_premultiplied_rgba() -> R {
    let img =
        expect_image(real().decode(Format::Png, (4, 3), &png_rgba(4, 3, [200, 100, 50, 128])?))?;
    assert_eq!((img.width, img.height), (4, 3));
    assert_eq!(img.rgba.len(), 4 * 3 * 4);
    // 200 * 128 / 255 = 100.4, 100 * 128 / 255 = 50.2, 50 * 128 / 255 = 25.1
    assert_eq!(&img.rgba[..4], &[100, 50, 25, 128]);
    Ok(())
}

#[test]
fn jpeg_decodes_to_opaque_rgba() -> R {
    let img = expect_image(real().decode(
        Format::Jpeg,
        (8, 8),
        &jpeg_rgba(8, 8, [255, 255, 255, 255])?,
    ))?;
    assert_eq!((img.width, img.height, img.rgba.len()), (8, 8, 256));
    assert!(
        img.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == 255 && p[0] >= 250)
    );
    Ok(())
}

#[test]
fn webp_decodes_to_premultiplied_rgba() -> R {
    let img =
        expect_image(real().decode(Format::Webp, (4, 4), &webp_rgba(4, 4, [10, 20, 30, 255])?))?;
    assert_eq!((img.width, img.height), (4, 4));
    assert_eq!(&img.rgba[..4], &[10, 20, 30, 255]);
    Ok(())
}

#[test]
fn a_hint_smaller_than_the_image_is_a_crash_not_an_allocation() -> R {
    // The image is 100x100 and the producer declared 1x1: the reservation is
    // 4 bytes, so a reply of 40000 bytes must never be accepted.
    let o = real().decode(Format::Png, (1, 1), &png_rgba(100, 100, [1, 2, 3, 255])?);
    assert_eq!(o, Outcome::Crash(Crash::Overflow));
    Ok(())
}

#[test]
fn a_hint_larger_than_the_image_is_fine() -> R {
    let img = expect_image(real().decode(Format::Png, (50, 50), &png_rgba(2, 2, [1, 2, 3, 255])?))?;
    assert_eq!((img.width, img.height), (2, 2));
    Ok(())
}

// ---- input limits, rev 9 64-bit bound ----------------------------------

#[test]
fn hints_over_the_limits_never_start_a_worker() -> R {
    let png = png_rgba(1, 1, [0, 0, 0, 255])?;
    let r = Runner::new("/nonexistent/worker");
    for hint in [
        (0, 1),
        (1, 0),
        (65536, 1),
        (1, 65536),
        (6000, 6000),
        (u32::MAX, u32::MAX),
    ] {
        assert_eq!(
            r.decode(Format::Png, hint, &png),
            Outcome::Refused,
            "{hint:?}"
        );
    }
    // 32 megapixels exactly is allowed through to a worker (which then fails to start).
    assert_eq!(
        r.decode(Format::Png, (8000, 4000), &png),
        Outcome::Crash(Crash::Spawn)
    );
    assert_eq!(r.decode(Format::Png, (8001, 4000), &png), Outcome::Refused);
    Ok(())
}

#[test]
fn a_format_that_disagrees_with_the_signature_is_refused() -> R {
    let jpeg = jpeg_rgba(2, 2, [0, 0, 0, 255])?;
    assert_eq!(real().decode(Format::Png, (2, 2), &jpeg), Outcome::Refused);
    Ok(())
}

#[test]
fn the_worker_bounds_declared_dimensions_before_allocating() {
    // 65535 x 65535 is 4.29 gigapixels; the worker must say TooLarge from the header.
    let o = real().decode(Format::Png, (1, 1), &png_declaring(65535, 65535));
    assert_eq!(o, Outcome::Rejected(Status::TooLarge));
    // Past the side limit as well.
    let o = real().decode(Format::Png, (1, 1), &png_declaring(65536, 1));
    assert_eq!(o, Outcome::Rejected(Status::TooLarge));
    // 32 megapixels + 1 within the side limit: 8001 x 4000.
    let o = real().decode(Format::Png, (1, 1), &png_declaring(8001, 4000));
    assert_eq!(o, Outcome::Rejected(Status::TooLarge));
}

#[test]
fn corrupt_and_truncated_images_are_rejected_not_crashes() -> R {
    let mut png = png_rgba(16, 16, [9, 9, 9, 255])?;
    png.truncate(png.len() / 2);
    assert_eq!(
        real().decode(Format::Png, (16, 16), &png),
        Outcome::Rejected(Status::Corrupt)
    );
    let mut jpeg = jpeg_rgba(16, 16, [9, 9, 9, 255])?;
    jpeg.truncate(30);
    assert!(matches!(
        real().decode(Format::Jpeg, (16, 16), &jpeg),
        Outcome::Rejected(_)
    ));
    let mut webp = webp_rgba(16, 16, [9, 9, 9, 255])?;
    webp.truncate(30);
    assert!(matches!(
        real().decode(Format::Webp, (16, 16), &webp),
        Outcome::Rejected(_)
    ));
    Ok(())
}

// ---- a hostile worker --------------------------------------------------

mod hostile {
    use super::*;

    const HOSTILE: &str = env!("CARGO_BIN_EXE_lumen-decoder-hostile");

    fn run(mode: &str) -> Outcome {
        Runner::new(HOSTILE).with_args(&[mode]).decode(
            Format::Png,
            (1, 1),
            &png_rgba(1, 1, [0, 0, 0, 255]).unwrap_or_default(),
        )
    }

    fn run_with(args: &[&str]) -> Outcome {
        Runner::new(HOSTILE).with_args(args).decode(
            Format::Png,
            (1, 1),
            &png_rgba(1, 1, [0, 0, 0, 255]).unwrap_or_default(),
        )
    }

    #[test]
    fn a_reply_larger_than_the_reservation_is_a_crash() {
        assert_eq!(run("lie-over-reservation"), Outcome::Crash(Crash::Overflow));
    }

    #[test]
    fn a_reply_over_the_pixel_limit_is_a_crash() {
        assert_eq!(run("lie-over-limit"), Outcome::Crash(Crash::BadReply));
    }

    #[test]
    fn a_64_bit_overflowing_reply_is_a_crash() {
        assert_eq!(run("lie-overflow"), Outcome::Crash(Crash::BadReply));
    }

    #[test]
    fn a_wrong_stride_is_a_crash() {
        assert_eq!(run("lie-stride"), Outcome::Crash(Crash::BadReply));
    }

    #[test]
    fn fewer_bytes_than_announced_is_a_crash() {
        let o = Runner::new(HOSTILE).with_args(&["lie-short"]).decode(
            Format::Png,
            (2, 2),
            &png_rgba(1, 1, [0, 0, 0, 255]).unwrap_or_default(),
        );
        assert_eq!(o, Outcome::Crash(Crash::Short));
    }

    #[test]
    fn extra_bytes_after_the_data_are_a_crash() {
        assert_eq!(run("lie-extra"), Outcome::Crash(Crash::BadReply));
    }

    #[test]
    fn an_unknown_status_is_a_crash() {
        assert_eq!(run("lie-status"), Outcome::Crash(Crash::BadReply));
    }

    #[test]
    fn a_rejection_that_carries_data_is_a_crash() {
        assert_eq!(
            run("lie-rejected-with-data"),
            Outcome::Crash(Crash::BadReply)
        );
    }

    #[test]
    fn a_nonzero_exit_without_a_reply_is_a_crash() {
        assert_eq!(run("raw-exit-nonzero"), Outcome::Crash(Crash::Exit));
    }

    #[test]
    fn a_stdout_flood_is_cut_off_at_the_reservation() {
        // A worker that blocked on a full pipe would time out instead.
        assert_eq!(run("flood-stdout"), Outcome::Crash(Crash::Overflow));
    }

    #[test]
    fn a_stderr_flood_neither_blocks_nor_changes_the_result() -> R {
        // A worker blocked on a full stderr pipe would time out instead.
        let img = expect_image(run("flood-stderr"))?;
        assert_eq!(img.rgba, vec![9, 8, 7, 255]);
        Ok(())
    }

    #[test]
    fn a_hung_worker_is_killed_at_the_wall_clock_limit() {
        let t = Instant::now();
        assert_eq!(run("raw-hang"), Outcome::Crash(Crash::Timeout));
        let e = t.elapsed();
        assert!(
            e >= Duration::from_millis(3900) && e < Duration::from_secs(8),
            "{e:?}"
        );
    }

    #[test]
    fn the_cpu_limit_stops_a_spinning_worker_before_the_wall_clock() {
        let t = Instant::now();
        let o = run("spin");
        let e = t.elapsed();
        // Killed by the CPU limit, not by the wall clock (that would be a
        // `Timeout`). On a busy host 2 CPU seconds can take longer than 4 s of
        // wall time, so the elapsed time is not asserted.
        assert_eq!(o, Outcome::Crash(Crash::Exit), "{e:?}");
    }

    #[test]
    fn the_memory_limit_stops_a_worker_that_hogs_memory() {
        // Touches 2 GiB in 32 MiB steps; the 512 MiB limit must stop it.
        assert_eq!(run("hog"), Outcome::Crash(Crash::Exit));
    }

    fn pid_alive(pid: i32) -> bool {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn pid_from(path: &std::path::Path) -> Option<i32> {
        std::fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    fn temp_name(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("lumen-g4-{tag}-{}", std::process::id()))
    }

    fn gone_within(pid: i32, wait: Duration) -> bool {
        let t = Instant::now();
        while t.elapsed() < wait {
            if !pid_alive(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        !pid_alive(pid)
    }

    #[test]
    fn a_grandchild_of_a_hung_worker_is_killed() -> R {
        let f = temp_name("gc-hang");
        let o = run_with(&["raw-grandchild", f.to_str().unwrap_or("")]);
        assert_eq!(o, Outcome::Crash(Crash::Timeout));
        let pid = pid_from(&f);
        let _ = std::fs::remove_file(&f);
        let pid = pid.ok_or("the grandchild never started")?;
        assert!(
            gone_within(pid, Duration::from_secs(2)),
            "grandchild {pid} survived"
        );
        Ok(())
    }

    #[test]
    fn a_grandchild_that_outlives_an_exited_worker_is_killed() -> R {
        let f = temp_name("gc-exit");
        let t = Instant::now();
        // The worker exits at once; the child holds the pipes. The host must
        // neither hang on them nor leave the child running.
        let o = run_with(&["raw-grandchild-exit", f.to_str().unwrap_or("")]);
        // The child sleeps for 60 s; hanging on its pipes would take that long.
        assert!(t.elapsed() < Duration::from_secs(30), "{:?}", t.elapsed());
        assert!(matches!(o, Outcome::Image(_)), "{o:?}");
        let pid = pid_from(&f);
        let _ = std::fs::remove_file(&f);
        let pid = pid.ok_or("the grandchild never started")?;
        assert!(
            gone_within(pid, Duration::from_secs(2)),
            "grandchild {pid} survived"
        );
        Ok(())
    }

    #[test]
    fn a_worker_that_does_nothing_forbidden_still_runs_in_the_sandbox() -> R {
        // Control for the denials below: without it they would also pass if the
        // worker simply died on entering the sandbox.
        expect_image(run("control"))?;
        Ok(())
    }

    #[test]
    fn a_worker_under_the_memory_limit_still_runs() -> R {
        expect_image(run("hog-small"))?;
        Ok(())
    }

    // Escape attempts run inside the sandbox. Success of the attempt would
    // come back as an image. Where the platform refuses the call, the worker
    // reports it and answers `Corrupt`; on Linux the filter kills the worker.
    fn denied_outcome(o: &Outcome, what: &str) {
        #[cfg(target_os = "linux")]
        assert_eq!(*o, Outcome::Crash(Crash::Exit), "{what}");
        #[cfg(not(target_os = "linux"))]
        assert_eq!(*o, Outcome::Rejected(Status::Corrupt), "{what}");
    }

    fn denied(mode: &str) {
        denied_outcome(&run(mode), mode);
    }

    #[test]
    fn the_sandbox_denies_reading_a_file() {
        denied("open-file");
        denied("profile-open-file");
    }

    #[test]
    fn the_profile_alone_still_runs_a_benign_worker() -> R {
        expect_image(run("profile-control"))?;
        Ok(())
    }

    #[test]
    fn the_sandbox_denies_creating_a_file() {
        let marker = temp_name("create");
        let _ = std::fs::remove_file(&marker);
        denied_with(&["create-file", marker.to_str().unwrap_or("")]);
        assert!(!marker.exists(), "the worker created {}", marker.display());
        denied_with(&["profile-create-file", marker.to_str().unwrap_or("")]);
        assert!(
            !marker.exists(),
            "the profile alone let the worker create {}",
            marker.display()
        );
    }

    fn denied_with(args: &[&str]) {
        denied_outcome(&run_with(args), args[0]);
    }

    #[test]
    fn the_sandbox_denies_a_network_connection() -> R {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port().to_string();
        denied_with(&["connect", &port]);
        denied_with(&["profile-connect", &port]);
        drop(listener);
        Ok(())
    }

    #[test]
    fn the_sandbox_denies_a_socket() {
        denied("udp");
        denied("profile-udp");
    }

    #[test]
    fn the_sandbox_denies_starting_a_process() {
        denied("exec");
        denied("profile-exec");
    }
}
