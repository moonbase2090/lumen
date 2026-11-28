#!/usr/bin/env bash
# Tests for notarize-macos.sh, build-bright-dmg.sh and release-notes-preview.sh.
# Apple's notary service is replaced by a fake xcrun that records what it was
# asked and answers as the test says, so no credential is used. The disk image
# build runs only on macOS.
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
NOTARIZE="$DIR/notarize-macos.sh"
BUILD_DMG="$DIR/build-bright-dmg.sh"
NOTES="$DIR/release-notes-preview.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILED=0

# expect <description> <want exit> <command...>
expect() {
  local desc="$1" want="$2" rc=0
  shift 2
  "$@" >"$WORK/out.txt" 2>&1 || rc=$?
  if [[ "$rc" -eq "$want" ]]; then
    echo "ok   $desc"
  else
    echo "FAIL $desc (exit $rc, wanted $want)"
    FAILED=1
  fi
}
log_has() { grep -q -- "$1" "$WORK/calls.log" 2>/dev/null; }
check() { # check <description> <command...>
  local desc="$1"
  shift
  if "$@"; then echo "ok   $desc"; else echo "FAIL $desc"; FAILED=1; fi
}

cat >"$WORK/xcrun" <<'SH'
#!/usr/bin/env bash
echo "xcrun $*" >>"$FAKE_LOG"
case "$1 $2" in
  "notarytool submit")
    [[ "${FAKE_SUBMIT_FAILS:-0}" == 1 ]] && exit 1
    printf '{"id":"x","status":"%s"}\n' "${FAKE_STATUS:-Accepted}"
    ;;
  "stapler staple") [[ "${FAKE_STAPLE_FAILS:-0}" == 1 ]] && exit 1 ;;
  "stapler validate") ;;
esac
exit 0
SH
cat >"$WORK/spctl" <<'SH'
#!/usr/bin/env bash
echo "spctl $*" >>"$FAKE_LOG"
[[ "${FAKE_SPCTL_FAILS:-0}" == 1 ]] && exit 3
exit 0
SH
chmod +x "$WORK/xcrun" "$WORK/spctl"

mkdir -p "$WORK/Bright.app/Contents/MacOS"
printf 'x' >"$WORK/Bright.app/Contents/MacOS/bright"
chmod +x "$WORK/Bright.app/Contents/MacOS/bright"
printf 'dmg' >"$WORK/Bright-0.1.0-preview.1.dmg"

export FAKE_LOG="$WORK/calls.log"
export NOTARY_XCRUN="$WORK/xcrun" NOTARY_SPCTL="$WORK/spctl"
export APPLE_NOTARY_ISSUER=issuer APPLE_NOTARY_KEY_ID=keyid
export APPLE_NOTARY_KEY='-----BEGIN PRIVATE KEY-----
not-a-real-key
-----END PRIVATE KEY-----'

run() { : >"$FAKE_LOG"; "$NOTARIZE" "$@"; }

expect "an accepted app is stapled and checked" 0 run "$WORK/Bright.app"
check "the app was submitted, stapled and validated" bash -c \
  "grep -q 'notarytool submit' '$FAKE_LOG' && grep -q 'stapler staple' '$FAKE_LOG' && grep -q 'stapler validate' '$FAKE_LOG' && grep -q 'spctl --assess --type execute' '$FAKE_LOG'"
expect "an accepted disk image is stapled and checked" 0 run "$WORK/Bright-0.1.0-preview.1.dmg"
check "the disk image was assessed as an open image" grep -q 'spctl --assess --type open' "$FAKE_LOG"

FAKE_STATUS=Invalid expect "a verdict other than Accepted is refused" 1 run "$WORK/Bright.app"
check "nothing is stapled after a refused verdict" bash -c "! grep -q 'stapler' '$FAKE_LOG'"
FAKE_SUBMIT_FAILS=1 expect "a failed submission is refused" 1 run "$WORK/Bright.app"
FAKE_STAPLE_FAILS=1 expect "a failed staple is refused" 1 run "$WORK/Bright.app"
FAKE_SPCTL_FAILS=1 expect "a Gatekeeper refusal is refused" 1 run "$WORK/Bright.app"

for name in APPLE_NOTARY_ISSUER APPLE_NOTARY_KEY_ID APPLE_NOTARY_KEY; do
  expect "without $name it refuses" 1 env -u "$name" "$NOTARIZE" "$WORK/Bright.app"
done
expect "an empty notary key refuses" 1 env APPLE_NOTARY_KEY= "$NOTARIZE" "$WORK/Bright.app"
expect "a notary key that is not a key refuses" 1 env APPLE_NOTARY_KEY=bm90LWEta2V5 "$NOTARIZE" "$WORK/Bright.app"
check "a refusal for a missing secret submits nothing" bash -c \
  ": >'$FAKE_LOG'; env -u APPLE_NOTARY_KEY '$NOTARIZE' '$WORK/Bright.app' >/dev/null 2>&1 || true; ! grep -q notarytool '$FAKE_LOG'"
expect "a base64 key is accepted" 0 env APPLE_NOTARY_KEY="$(printf '%s' "$APPLE_NOTARY_KEY" | base64 | tr -d '\n')" "$NOTARIZE" "$WORK/Bright.app"
expect "a directory that is not Bright.app is refused" 1 "$NOTARIZE" "$WORK"
expect "a missing disk image is refused" 1 "$NOTARIZE" "$WORK/nope.dmg"
expect "no argument is a usage error" 2 "$NOTARIZE"
expect "an option as the path is a usage error" 2 "$NOTARIZE" --bogus
check "the key never appears in the output" bash -c \
  "! \"$NOTARIZE\" '$WORK/Bright.app' 2>&1 | grep -q 'not-a-real-key'"

# Release notes.
mkdir "$WORK/dist"
printf '%s  Bright-0.1.0-preview.1.dmg\n' "$(printf 'a%.0s' {1..64})" >"$WORK/dist/SHA256SUMS"
expect "notes are written for a preview tag" 0 "$NOTES" v0.1.0-preview.1 "$WORK/dist" "$WORK/notes.md"
check "the notes carry the version and the checksum" bash -c \
  "grep -q 'Bright 0.1.0-preview.1' '$WORK/notes.md' && grep -q 'Bright-0.1.0-preview.1.dmg' '$WORK/notes.md'"
expect "a tag that is not a preview is refused" 1 "$NOTES" v0.1.0 "$WORK/dist" "$WORK/n2.md"
expect "a tag with extra text is refused" 1 "$NOTES" 'v0.1.0-preview.1; echo' "$WORK/dist" "$WORK/n3.md"
mkdir "$WORK/empty"
expect "notes without checksums are refused" 1 "$NOTES" v0.1.0-preview.1 "$WORK/empty" "$WORK/n4.md"
printf 'junk\n' >"$WORK/empty/SHA256SUMS"
expect "notes with no disk image in the checksums are refused" 1 "$NOTES" v0.1.0-preview.1 "$WORK/empty" "$WORK/n5.md"

# Disk image build (macOS only).
if [[ "$(uname -s)" == Darwin ]]; then
  expect "no argument to the image builder is a usage error" 2 "$BUILD_DMG"
  expect "a directory that is not Bright.app is refused by the builder" 1 "$BUILD_DMG" "$WORK" "$WORK/a.dmg"
  expect "an output that is not .dmg is refused" 1 "$BUILD_DMG" "$WORK/Bright.app" "$WORK/a.img"
  expect "an existing output is not overwritten" 1 "$BUILD_DMG" "$WORK/Bright.app" "$WORK/Bright-0.1.0-preview.1.dmg"
  expect "the image is built" 0 "$BUILD_DMG" "$WORK/Bright.app" "$WORK/built.dmg"
  mkdir "$WORK/mnt"
  if hdiutil attach -quiet -nobrowse -readonly -mountpoint "$WORK/mnt" "$WORK/built.dmg"; then
    check "the image holds Bright.app and a link to Applications, and nothing else" bash -c \
      "[ -x '$WORK/mnt/Bright.app/Contents/MacOS/bright' ] && [ -L '$WORK/mnt/Applications' ] && [ \"\$(ls -A '$WORK/mnt' | grep -v '^\\.' | wc -l | tr -d ' ')\" = 2 ]"
    hdiutil detach -quiet "$WORK/mnt" || true
  else
    echo "FAIL the image could not be mounted"
    FAILED=1
  fi
else
  echo "skip the disk image build (macOS only)"
fi

exit "$FAILED"
