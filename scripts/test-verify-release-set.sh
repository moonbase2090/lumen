#!/usr/bin/env bash
# Tests for verify-release-set.sh, on stand-in files. The passing case shows
# the verifier is not refusing everything.
set -euo pipefail

VERIFY="$(cd "$(dirname "$0")" && pwd)/verify-release-set.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILED=0

set_up() {
  local contents="$WORK/$1/Bright.app/Contents"
  mkdir -p "$contents/MacOS"
  cat >"$contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleDisplayName</key><string>Bright</string>
<key>CFBundleName</key><string>Bright</string>
<key>CFBundleExecutable</key><string>bright</string>
<key>CFBundleIdentifier</key><string>app.brightterm.Bright</string>
</dict></plist>
PLIST
  printf 'terminal with a window\n' >"$contents/MacOS/bright"
  printf 'worker\n' >"$contents/MacOS/lumen-decoder-worker"
  printf 'holder\n' >"$contents/MacOS/lumen-holder"
  chmod +x "$contents/MacOS/bright" "$contents/MacOS/lumen-decoder-worker" "$contents/MacOS/lumen-holder"
}

expect() {
  local desc="$1" want="$2" rc=0
  shift 2
  "$VERIFY" "$@" >/dev/null 2>&1 || rc=$?
  if [[ "$rc" -eq "$want" ]]; then
    echo "ok   $desc"
  else
    echo "FAIL $desc (exit $rc, wanted $want)"
    FAILED=1
  fi
}

set_up good
expect "a complete app passes" 0 "$WORK/good"

set_up noworker; rm "$WORK/noworker/Bright.app/Contents/MacOS/lumen-decoder-worker"
expect "an app without the decoder worker is refused" 1 "$WORK/noworker"

set_up noterm; rm "$WORK/noterm/Bright.app/Contents/MacOS/bright"
expect "an app without Bright is refused" 1 "$WORK/noterm"

set_up noholder; rm "$WORK/noholder/Bright.app/Contents/MacOS/lumen-holder"
expect "an app without the holder is refused" 1 "$WORK/noholder"

set_up nolist; rm "$WORK/nolist/Bright.app/Contents/Info.plist"
expect "an app without Info.plist is refused" 1 "$WORK/nolist"

set_up wrongid
sed -i.bak 's/app\.brightterm\.Bright/example.invalid.Bright/' "$WORK/wrongid/Bright.app/Contents/Info.plist"
rm "$WORK/wrongid/Bright.app/Contents/Info.plist.bak"
expect "a wrong bundle identifier is refused" 1 "$WORK/wrongid"

set_up nowindow
printf 'this build has no window: build with --features window\n' >>"$WORK/nowindow/Bright.app/Contents/MacOS/bright"
expect "a terminal built without the window feature is refused" 1 "$WORK/nowindow"

set_up notexec; chmod -x "$WORK/notexec/Bright.app/Contents/MacOS/lumen-decoder-worker"
expect "a worker that is not executable is refused" 1 "$WORK/notexec"

set_up hostile
cp "$WORK/hostile/Bright.app/Contents/MacOS/lumen-decoder-worker" "$WORK/hostile/Bright.app/Contents/MacOS/lumen-decoder-hostile"
expect "the test-only hostile worker is refused" 1 "$WORK/hostile"

set_up link
rm "$WORK/link/Bright.app/Contents/MacOS/lumen-decoder-worker"
ln -s "$WORK/good/Bright.app/Contents/MacOS/lumen-decoder-worker" "$WORK/link/Bright.app/Contents/MacOS/lumen-decoder-worker"
expect "a worker that is a link is refused" 1 "$WORK/link"

expect "a missing directory is refused" 1 "$WORK/nope"
expect "no argument is a usage error" 2
expect "an option is a usage error" 2 --bogus

exit "$FAILED"
