"""Writes the program output each comp shows, as files of bytes.

Each scene is what a program writes to the terminal: ordinary text and
colours for the grid, and Track 0 frames for the rich blocks. `make-comps.sh`
runs the terminal headless with `cat SCENE` as its program, so every comp is
the terminal's own drawing of real output. Usage: python3 scenes.py OUT_DIR
"""

import struct
import sys
import zlib
from pathlib import Path

TAG = b"\x8aLUMEN\x01\xf3"
ESC = b"\x1b"


def frame(kind: int, body: bytes) -> bytes:
    return ESC + b"_" + TAG + bytes([kind]) + struct.pack(">I", len(body)) + body


def string(text: str) -> bytes:
    data = text.encode()
    return struct.pack(">H", len(data)) + data


def paragraph(text: str) -> bytes:
    return frame(0x80, string(text))


def heading(level: int, text: str) -> bytes:
    return frame(0x81, bytes([level]) + string(text))


def code(lang: str, text: str) -> bytes:
    return frame(0x82, string(lang) + string(text))


def tiny_png(w: int = 8, h: int = 8) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        c = kind + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))

    rows = b"".join(b"\x00" + bytes([40, 90, 200] * w) for _ in range(h))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows))
        + chunk(b"IEND", b"")
    )


def image(image_id: int, alt: str, data: bytes) -> bytes:
    start = struct.pack(">IBIII", image_id, 1, 8, 8, len(data)) + string(alt)
    body = struct.pack(">II", image_id, 0) + data
    return frame(0x83, start) + frame(0x84, body)


def sgr(codes: str, text: str) -> bytes:
    return ESC + b"[" + codes.encode() + b"m" + text.encode() + ESC + b"[0m"


def grid() -> bytes:
    out = b"$ ls\r\n"
    out += sgr("1;34", "docs") + b"  " + sgr("1;34", "src") + b"  " + sgr("32", "build.sh")
    out += b"  Cargo.toml  README.md\r\n"
    out += b"$ cargo test\r\n"
    out += b"   Compiling bright v0.1.0\r\n"
    out += b"test layout::wraps_long_lines ... " + sgr("32", "ok") + b"\r\n"
    out += b"test theme::reads_a_file ... " + sgr("32", "ok") + b"\r\n"
    out += b"test fonts::clamps_sizes ... " + sgr("31", "FAILED") + b"\r\n"
    out += sgr("33", "warning") + b": unused variable `scale`\r\n"
    out += b"$ colours\r\n"
    for base in (40, 100):
        for n in range(8):
            out += ESC + b"[" + str(base + n).encode() + b"m    " + ESC + b"[0m"
        out += b"\r\n"
    for base in (30, 90):
        for n in range(8):
            out += sgr(str(base + n), f" {base + n} ")
        out += b"\r\n"
    out += sgr("1", "bold") + b" " + sgr("4", "underline") + b" " + sgr("7", "inverse") + b"\r\n"
    return out + b"$ "


def rich() -> bytes:
    # The window draws the document top down, and a grid block is as tall as
    # the screen, so the frames come first: a rich block after a full grid
    # block would be below the window.
    out = heading(1, "Release notes")
    out += paragraph(
        "Bright draws a program's paragraphs and headings in a proportional face, "
        "inside a box labelled as unverified output, because the terminal cannot "
        "vouch for the program that wrote them."
    )
    out += heading(2, "Build")
    out += code("sh", "cargo build --release\ncargo test --workspace")
    out += image(1, "Chart: build time per commit over the last week", tiny_png())
    out += b"$ "
    return out


def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for name, scene in (("grid", grid), ("rich", rich)):
        (out / f"{name}.bin").write_bytes(scene())


if __name__ == "__main__":
    main()
