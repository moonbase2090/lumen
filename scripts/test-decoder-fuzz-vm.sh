#!/usr/bin/env bash
# Tests the package step of scripts/decoder-fuzz-vm.sh when it runs as root (the
# user a VM start-up script runs as, where no sudo is used): the step must set
# DEBIAN_FRONTEND for apt-get and not try to run it as a command. The script
# runs with stand-ins for id, apt-get and sudo and stops at the first
# toolchain step; nothing is installed.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
mkdir "$T/bin"
cat > "$T/bin/id" <<'SH'
#!/bin/sh
echo 0
SH
cat > "$T/bin/apt-get" <<'SH'
#!/bin/sh
echo "apt-get frontend=${DEBIAN_FRONTEND:-unset} $*" >> "$APT_LOG"
SH
cat > "$T/bin/curl" <<'SH'
#!/bin/sh
exit 99
SH
chmod +x "$T/bin"/*
export APT_LOG="$T/apt.log"
PATH="$T/bin:$PATH" HOME="$T/home" COMMIT=0000000000000000000000000000000000000000 \
  bash "$ROOT/scripts/decoder-fuzz-vm.sh" >"$T/out" 2>&1
install_line="$(grep ' install ' "$APT_LOG" 2>/dev/null || true)"
case "$install_line" in
  "apt-get frontend=noninteractive "*) echo "ok    apt-get install runs with DEBIAN_FRONTEND=noninteractive as root" ;;
  *) echo "FAIL  apt-get install did not run with DEBIAN_FRONTEND=noninteractive as root"; cat "$T/out"; exit 1 ;;
esac
