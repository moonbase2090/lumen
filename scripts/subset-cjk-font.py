#!/usr/bin/env python3
"""Rebuild crates/lumen-term/assets/fonts/NotoSansCJK-Subset-Regular.otf.

Usage: subset-cjk-font.py NotoSansCJKsc-Regular.otf OUTPUT.otf
Needs fontTools (pip install fonttools). The characters are kana, CJK and
full-width punctuation, Hangul compatibility jamo, and every character in the
GB 2312, JIS X 0208 and KS X 1001 sets.
"""
import subprocess
import sys
import tempfile

source, output = sys.argv[1], sys.argv[2]
chars = set()


def add(encoding, leads):
    for lead in range(leads[0], leads[1] + 1):
        for trail in range(0xA1, 0xFF):
            try:
                text = bytes([lead, trail]).decode(encoding)
            except UnicodeDecodeError:
                continue
            if len(text) == 1 and ord(text) >= 0x2E80:
                chars.add(ord(text))


add("gb2312", (0xA1, 0xF7))
add("euc_jp", (0xA1, 0xF4))
add("euc_kr", (0xA1, 0xFD))
for first, last in ((0x3000, 0x303F), (0x3041, 0x30FE), (0x3131, 0x318E), (0xFF01, 0xFF5E), (0xFFE0, 0xFFE6), (0x2010, 0x2026)):
    chars.update(range(first, last + 1))
with tempfile.NamedTemporaryFile("w", suffix=".txt") as listing:
    listing.write(" ".join("U+%04X" % c for c in sorted(chars)))
    listing.flush()
    subprocess.run(
        ["pyftsubset", source, "--unicodes-file=" + listing.name, "--layout-features=",
         "--no-hinting", "--desubroutinize", "--drop-tables+=GSUB,GPOS,GDEF,vhea,vmtx,VORG",
         "--name-IDs=*", "--output-file=" + output],
        check=True,
    )
