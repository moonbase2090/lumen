+++
title = "Optional hardened mode"
weight = 170
+++

> Lumen Protocol v0, revision 25 (frozen snapshot). © Moonbase 2090 LLC. Licensed under CC-BY-4.0. Report security issues privately via GitHub private vulnerability reporting or security@lumenprotocol.org.

# Optional hardened mode {#hardened}

Hardened mode adds protection against same-uid code on Linux systems that meet its requirements. It is optional and off by default. When any check fails, no keys are created, and the layer is grid plus Track 0 with a notice. Track 0 behaviour does not change.

## Enabling {#hardened.enable}

Hardened mode is turned on with `hardened = true`, either in a root-owned system configuration file or in the user policy file. The user file can only turn hardened mode on. It can never turn off a system setting.

## Shell and namespace checks {#hardened.shell}

These checks run at layer start, on the pid and pidfd that the terminal got from its own spawn:

1. **Direct exec.** The terminal executes the listed shell binary directly, by its listed absolute path, with no wrapper, no `-c` command and no user program in between. `/proc/<pid>/comm` must equal the listed basename at the ready point.
2. **User namespace.** The shell's user namespace is the terminal's own, and `NS_GET_PARENT` on it fails with `EPERM`. A terminal inside a user namespace therefore fails this check.
3. **ID maps.** `uid_map` and `gid_map` are each parsed numerically as exactly one line: `0 0 4294967295`.
4. **Capabilities.** `CapInh`, `CapPrm`, `CapEff` and `CapAmb` all lack bit 19 (`CAP_SYS_PTRACE`).
5. **Pidfd.** After these reads, the shell pidfd is still not readable, meaning the shell is alive and has not been replaced.

## Kernel requirements {#hardened.kernel}

These are checked at the ready point and at every command start:

- Yama `ptrace_scope` is at least 1. The optional setting `require_yama_scope = 2` raises this to 2.
- `dev.tty.legacy_tiocsti = 0`.
- `perf_event_paranoid` is at least 1.
- With the optional `require_perf_block` setting, the running kernel must be positively identified as a family that fully blocks unprivileged per-task perf, and `perf_event_paranoid` must be at or above that family's minimum. Unknown kernels fail.
- **Latch.** The latch is terminal-wide. Once any check in a terminal process sees a value below its floor, every layer of that terminal, including later ones, runs without keys until the terminal exits.

`require_yama_scope` and `require_perf_block` can be set in a root-owned system config or in the user config. The user config can only turn them on.

## Installation {#hardened.install}

`sudo bright-term install-system` performs the root-owned install:

- It copies the statically linked (static-pie) terminal and holder binaries, read through the running image's own executable fd, into `/usr/local/libexec/bright-term/`. They are root-owned and mode 0711, so the kernel marks them non-dumpable from exec.
- It refuses if any parent directory is not root-owned or is group- or other-writable, and it refuses a binary that has a program interpreter.
- It runs the startup checks against the installed files and prints the result and the SHA-256 of each file.
- It never sets setuid or file capabilities, installs no service, changes no sysctl, and exits.
- The user-level launcher executes the system binary when it is present. Any other install runs without keys, with a notice that names this command.

## Startup checks {#hardened.startup}

1. The terminal's first action is `close_range(3, ~0U, 0)`. After that, every fd is created close-on-exec, and the terminal reopens its own fds 0 to 2 to `/dev/null`.
2. Before it creates any key or spawns anything, the terminal checks:
   - `PR_GET_DUMPABLE` returns 0.
   - `/proc/sys/fs/suid_dumpable` is 0.
   - Its executable is root-owned and not readable by the user. This is checked with `fstat` on an `O_PATH` fd from `/proc/self/exe`.
   - Every parent directory, opened with `O_PATH | O_NOFOLLOW` from the root down, is root-owned and not group- or other-writable.
3. Each holder starts with an empty environment and `/dev/null` on fds 0 to 2. It repeats the dumpable and executable checks before it accepts the seal fd. It exits if any `LD_*` or `GLIBC_TUNABLES` variable is present.
4. Any failure means the terminal runs without keys for its whole life, with a notice.

## Holder principal checks {#hardened.holder}

Holders verify their peer with `SCM_CREDENTIALS` plus the pid and process start time. A request from any other principal is refused.

## Conformance {#hardened.conformance}

Hardened mode is available on a platform only after the [hardened-mode gates](@/spec/v0/conformance.md#gate.hardened) pass.
