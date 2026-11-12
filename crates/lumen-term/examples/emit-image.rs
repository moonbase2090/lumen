//! Writes one image to standard output as Track 0 image frames: a start frame
//! and in-order data frames. Run it as the program of a terminal to show an
//! image, or by hand to see the bytes.
//!
//! `emit-image FILE [ALT] [WIDTH HEIGHT]`: the declared size is only a hint that
//! sets how much memory the terminal reserves; it defaults to 64 by 64, and the
//! terminal draws the size the decoder reports.

use std::io::Write;

use lumen_decoder::frames::MAX_CHUNK;
use lumen_detector::TAG;

fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_".to_vec();
    v.extend_from_slice(&TAG);
    v.push(kind);
    v.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
    v.extend_from_slice(body);
    v
}

/// Where the frames go. On Windows the standard library refuses to write bytes
/// that are not UTF-8 to a console, so the console is opened directly: the
/// frames are binary, and the Windows console still changes them on the way
/// (images are not supported on Windows in v0), which is what the refusal
/// proof exercises.
#[cfg(windows)]
fn output() -> std::io::Result<impl Write> {
    std::fs::OpenOptions::new().write(true).open("CONOUT$")
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
fn output() -> std::io::Result<impl Write> {
    Ok(std::io::stdout().lock())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        return Err("usage: emit-image FILE [ALT] [WIDTH HEIGHT]".into());
    };
    let alt = args.get(1).map_or("image", String::as_str);
    let width: u32 = args.get(2).map_or(Ok(64), |s| s.parse())?;
    let height: u32 = args.get(3).map_or(Ok(64), |s| s.parse())?;
    let bytes = std::fs::read(path)?;
    let length = u32::try_from(bytes.len())?;
    let id = 1u32;

    let mut start = id.to_be_bytes().to_vec();
    start.push(0);
    for n in [width, height, length] {
        start.extend_from_slice(&n.to_be_bytes());
    }
    start.extend_from_slice(&u16::try_from(alt.len())?.to_be_bytes());
    start.extend_from_slice(alt.as_bytes());

    let mut out = output()?;
    out.write_all(&frame(lumen_track0::KIND_IMAGE_START, &start))?;
    let mut offset = 0u32;
    for chunk in bytes.chunks(MAX_CHUNK) {
        let mut body = id.to_be_bytes().to_vec();
        body.extend_from_slice(&offset.to_be_bytes());
        body.extend_from_slice(chunk);
        out.write_all(&frame(lumen_track0::KIND_IMAGE_DATA, &body))?;
        offset += u32::try_from(chunk.len())?;
    }
    out.flush()?;
    Ok(())
}
