#!/usr/bin/env bash
# Fetches the regression inputs used as fuzz seeds from the upstream decoder
# repositories, at pinned commits, and writes them with a manifest (source,
# commit, path, size, sha256). These are inputs the upstream projects keep for
# bugs they fixed; they are NOT CVE reproducers (the three chosen libraries have
# no registered advisories). Re-run to refresh; review the diff before committing.
#
#   scripts/fetch-regression-seeds.sh DIR
set -euo pipefail
OUT="${1:?usage: fetch-regression-seeds.sh DIR}"

PNG_REPO=image-rs/image-png;      PNG_SHA=b2f10c6a7e3127d4aaf231f3eae731a33e6eb0bc
WEBP_REPO=image-rs/image-webp;    WEBP_SHA=f4d80bd965df2c81e65b6f43c1f70e0750bd4b0f
JPEG_REPO=etemesi254/zune-image;  JPEG_SHA=2f9086b07cdf5a36621ce18f79d310f99b751de9

FILES=(
  "png $PNG_REPO $PNG_SHA tests/bugfixes/acid2.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/cicp_pq.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/invalid_palette_index.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/issue#403.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/issue#430.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/F-exif-chunk-early.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/issue#1825.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/issue#202.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/x_consecutive_idat.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/x_ihdr_missing.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/x_image-issue#1075.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/x_interlaced_chck.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/x_issue#214.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/x_issue#230.0.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/x_issue#230.1.png Apache-2.0 OR MIT"
  "png $PNG_REPO $PNG_SHA tests/bugfixes/x_unexpected_eof.png Apache-2.0 OR MIT"
  "webp $WEBP_REPO $WEBP_SHA tests/images/regression/color_index.webp Apache-2.0 OR MIT"
  "webp $WEBP_REPO $WEBP_SHA tests/images/regression/dark.webp Apache-2.0 OR MIT"
  "webp $WEBP_REPO $WEBP_SHA tests/images/regression/lossless_indexed_1bit_palette.webp Apache-2.0 OR MIT"
  "webp $WEBP_REPO $WEBP_SHA tests/images/regression/lossless_indexed_2bit_palette.webp Apache-2.0 OR MIT"
  "webp $WEBP_REPO $WEBP_SHA tests/images/regression/lossless_indexed_4bit_palette.webp Apache-2.0 OR MIT"
  "webp $WEBP_REPO $WEBP_SHA tests/images/regression/tiny.webp Apache-2.0 OR MIT"
  "jpeg $JPEG_REPO $JPEG_SHA test-images/jpeg/baseline_non_interleaved.jpg MIT OR Apache-2.0 OR Zlib"
  "jpeg $JPEG_REPO $JPEG_SHA test-images/jpeg/fill_bytes_at_incremental_cut.jpg MIT OR Apache-2.0 OR Zlib"
  "jpeg $JPEG_REPO $JPEG_SHA test-images/jpeg/issue_482_mixed_sampling.jpg MIT OR Apache-2.0 OR Zlib"
  "jpeg $JPEG_REPO $JPEG_SHA test-images/jpeg/lumaa_h_sampled_unused_component.jpg MIT OR Apache-2.0 OR Zlib"
  "jpeg $JPEG_REPO $JPEG_SHA test-images/jpeg/non_interleaved_420_64x64.jpg MIT OR Apache-2.0 OR Zlib"
  "jpeg $JPEG_REPO $JPEG_SHA test-images/jpeg/non_interleaved_422_64x64.jpg MIT OR Apache-2.0 OR Zlib"
  "jpeg $JPEG_REPO $JPEG_SHA test-images/jpeg/non_interleaved_422_65x65.jpg MIT OR Apache-2.0 OR Zlib"
  "jpeg $JPEG_REPO $JPEG_SHA test-images/jpeg/non_interleaved_440_64x64.jpg MIT OR Apache-2.0 OR Zlib"
  "jpeg $JPEG_REPO $JPEG_SHA test-images/jpeg/non_interleaved_444_64x64.jpg MIT OR Apache-2.0 OR Zlib"
)

mkdir -p "$OUT"
MANIFEST="$OUT/MANIFEST.tsv"
printf 'file\tsource\tcommit\tpath\tbytes\tsha256\tupstream licence\n' > "$MANIFEST"
for row in "${FILES[@]}"; do
  read -r fmt repo sha path lic <<< "$row"
  name="$(basename "$path" | tr '#' '-')"
  mkdir -p "$OUT/$fmt"
  # `#` in a path must be percent-encoded in a URL.
  curl -fsSL "https://raw.githubusercontent.com/$repo/$sha/${path//#/%23}" -o "$OUT/$fmt/$name"
  bytes="$(wc -c < "$OUT/$fmt/$name" | tr -d ' ')"
  hash="$(shasum -a 256 "$OUT/$fmt/$name" | cut -d' ' -f1)"
  printf '%s/%s\thttps://github.com/%s\t%s\t%s\t%s\t%s\t%s\n' "$fmt" "$name" "$repo" "$sha" "$path" "$bytes" "$hash" "$lic" >> "$MANIFEST"
done
echo "wrote $(($(wc -l < "$MANIFEST") - 1)) files to $OUT"
