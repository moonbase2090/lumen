#!/bin/sh
set -eu

if [ "$(id -u)" -ne 0 ] || [ "$#" -ne 3 ]; then
    echo "usage: sudo scripts/g10-yama-matrix.sh UNPRIVILEGED_UID UNPRIVILEGED_GID G10_BINARY" >&2
    echo "run only on a disposable Linux VM; Yama scope 3 cannot be lowered until reboot" >&2
    exit 2
fi

uid=$1
gid=$2
binary=$(readlink -f "$3")

if [ "$uid" -eq 0 ] || [ "$gid" -eq 0 ] || [ ! -x "$binary" ]; then
    echo "the uid and gid must be non-root and the G10 binary must be executable" >&2
    exit 2
fi

case "$(cat /proc/sys/kernel/yama/ptrace_scope)" in
    0|1|2) ;;
    *)
        echo "the VM must start at Yama scope 0, 1, or 2" >&2
        exit 2
        ;;
esac

for scope in 0 1 2 3; do
    printf '%s\n' "$scope" > /proc/sys/kernel/yama/ptrace_scope
    setpriv --reuid "$uid" --regid "$gid" --clear-groups --no-new-privs \
        env -i PATH=/usr/bin:/bin HOME=/tmp "$binary"
done
