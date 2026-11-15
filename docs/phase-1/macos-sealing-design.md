# Sealing on macOS: design note

Status: a design for review. It changes no code and no specification text. A specification revision follows only after this note is signed off.

Today v0 creates no keys on macOS: a layer there is grid plus Track 0 ([platforms](../../spec/v0/platforms.md#platforms.macos)), and the first sealing condition in `crates/lumen-seal/src/conditions.rs` is "the terminal runs on Linux". Bright is meant to run on macOS, Windows and Linux with the same trust features. This note is the macOS half of sealing, written in the same shape as the note on sealing on Windows (`docs/phase-1/windows-sealing-design.md`, a separate change): what each Linux condition becomes on macOS, the key holder and the seal channel, a threat model, failure behaviour, sized tickets and the specification changes.

"Sealed" has its specification meaning: a layer whose command structure, exit status and capability grants come from frames authenticated with per-layer keys that only a separate key holder has. The parity rule of the [threat model](../../spec/v0/threat-model.md#threat.parity) holds: nothing added for macOS may give a same-user process more than writing bytes to the terminal already gives it.

## Summary

- **Sealing on macOS is feasible for the system zsh, and in two respects it is stronger than on Linux.** A hardened-runtime binary without the debugging entitlement could not be attached to or have its task port taken by another process of its user, in the measurements below, even by one that signed itself with the debugger entitlement, with no system setting to get right. An audit token carries a process-version number that changes when a process execs, so a same-name re-exec is detected, which the Linux conditions cannot do.
- **The terminal and the holder must be signed.** The protection above comes from the hardened runtime, which is a property of the code signature. A binary built from source and signed only by the linker has none, so a source-built terminal is grid-only with a notice that says how to sign it.
- **The shell is the system zsh.** `/bin/zsh` is an Apple platform binary under System Integrity Protection, which other processes cannot attach to. A shell that is neither a platform binary nor hardened (a Homebrew zsh, for instance) is grid-only in the first release.
- **The seal channel is a Unix socket by path, connected anew for each request**, because macOS gives a stream socket the peer's pid and audit token at connect time and gives nothing per message. A descriptor the shell passes on to a child would let the child send as the shell.
- **Gap 1: input injection.** `TIOCSTI` works for any process on the shell's controlling terminal and there is no setting that turns it off. The design accepts it and adds the same "input not typed in this window" marker the Windows note proposes.
- **Condition 4 has no macOS counterpart** and is dropped.
- **Failure behaviour is the Linux one.** Any failed or unreadable check means no holder, no keys, a grid-only layer and a notice that names the check. A system value below its floor latches the whole terminal.

## What is being protected

1. **A key holder**: a separate process that never starts another process and that same-user programs cannot read or debug.
2. **A seal channel** on which the holder can tell the shell apart from every other process, including the shell's own children.
3. **Conditions** on the shell and the system, read at the layer's start and again at each command start, which decide whether the layer may have keys.
4. **A carrier** that takes the sealed frame, written by the shell on the PTY, to the terminal unchanged. On macOS the PTY passes bytes through, which is why images already work there, so no envelope is needed.

## Measurements

The mechanisms below were checked, not recalled, on a clean macOS virtual machine with a throwaway probe program (not product code; the program and its output are kept with the proof files). The tables in the next section quote it.

Setup: a fresh virtual machine from a clean macOS image (macOS 26.6.2, Apple silicon, System Integrity Protection enabled, developer mode first disabled and then enabled), the default admin account, ssh only. The probe is a small C program built on the host. For the task-port tests it ran as an unrelated process of the same user against four targets, each a sleeping process of that user:

- the system zsh (`/bin/zsh`);
- a **plain** binary, ad hoc signed as the linker does it (no hardened runtime);
- a **hardened** binary, ad hoc signed with `--options runtime` and no entitlements;
- a **get-task-allow** binary, hardened and carrying that entitlement.

It ran as two attackers: an **unsigned-style** one (ad hoc signed, no entitlements), and a **debugger** one that signed itself ad hoc with the `com.apple.security.cs.debugger` entitlement. That entitlement needs no approval from Apple, so any same-user program can give it to itself.

| Target | Unsigned-style attacker | Debugger-entitled attacker |
|---|---|---|
| system zsh (platform binary) | task port refused, `ptrace` attach refused | task port refused, `ptrace` attach refused |
| hardened, no `get-task-allow` | task port refused, attach refused | task port refused, attach refused |
| plain (no hardened runtime) | task port refused, attach refused | task port refused, **attach succeeded** |
| `get-task-allow` | task port refused, attach refused | **task port granted, memory read succeeded, attach succeeded** |

The results were the same with developer mode disabled and enabled. Reading them:

- The default policy already refuses an ordinary unsigned same-user program every task port.
- A program that gives itself the debugger entitlement gets past that for any target that is not a platform binary and not hardened. That is the same-user adversary the design has to handle, and the hardened runtime without `get-task-allow` is what stopped it, as did being a platform binary.
- A plain, ad hoc signed binary, which is what a source build produces, was attachable. The measurement did not go on to read its memory, so the design treats a successful attach as the failure.
- A binary with `get-task-allow` is open to anyone, which is why the design refuses it.
- The signing flags `csops` reports were `0x26010b01` for the system zsh (valid, hard, kill, runtime, platform binary), `0x22011311` for the hardened binary (runtime set, no platform bit), `0x22000201` for the plain binary (no runtime) and `0x22011315` for the get-task-allow binary (the debugged-allowed bit). An ad hoc signature with `--options runtime` is honoured.
- System policy as read: the task-for-pid flag check of System Integrity Protection reports the protection on (`csr_check` fails for it), and the kernel's task-for-pid policy value is the default of 2. No boot arguments were set.

Other measurements, on the same machine:

- **Audit token across exec.** One process read its own audit token's process version before and after an `exec` of itself: the pid stayed the same and the version went from 2207 to 2208.
- **Descriptor inheritance.** A process connected a Unix stream socket to a listener and then forked a child that wrote on the same descriptor. The listener's `LOCAL_PEERPID` and `LOCAL_PEERTOKEN` named the parent for both writers. A child that opened its own new connection was named correctly. For a `socketpair`, the peer pid on each end, including in a forked child, was the process that made the pair. This is why the design connects anew for every request.
- **Stream socket by path from zsh.** The system zsh (5.9) loads `zsh/net/socket` and `zsh/system`. A listener saw the connecting zsh's pid (`LOCAL_PEERPID` and the audit token agreed) and the data it sent.
- **`TIOCSTI`.** A grandchild of a process that held the pseudo-terminal as its controlling terminal issued `TIOCSTI` on its input; the process read the injected byte. It was not refused, and there is no setting to turn it off.
- **`kqueue`.** A process filter for a child reported an exec event and then an exit event.
- **Not measured:** a standard (non-admin) account (the guest could not create one non-interactively); Intel and Rosetta; macOS releases other than 26.6.2; whether an attach to the plain binary yields a memory read. The first implementation ticket covers them.

The probe's source, its output and the signing flags are kept with the proof files.

## The Linux conditions, one by one

Numbers are those in `conditions.rs`. "Gap" means macOS cannot provide the guarantee and the design says what it does instead.

| # | Linux condition | macOS counterpart | Status |
|---|---|---|---|
| 1 | The terminal runs on Linux | The terminal runs on macOS 13 or later (the version floor is the lowest release that has every call this design uses, to be confirmed in the first spike), on Apple silicon or Intel, with System Integrity Protection on | Mapped |
| 2 | Listed shell, run directly by absolute path, no wrapper, no `-c`, `comm` is the listed name | The terminal starts `/bin/zsh` itself with `posix_spawn`, with a command line it built. The running process's path (`proc_pidpath`) is the listed one, its code signature is Apple's platform signature (checked from the process by its audit token, not from the file), and its name matches | Mapped, stronger |
| 3 | Yama `ptrace_scope` at least 1 | A process is protected from other processes of its user when it is an Apple platform binary or runs with the hardened runtime and without `get-task-allow`. The terminal reads the shell's and the holder's signing flags with `csops`, and the system policy that makes this hold (see condition 3a) | Mapped, per process |
| 3a | (new) the system setting behind condition 3 | System Integrity Protection's task-port protection is on (`csr_check` for the task-for-pid flag fails) and the kernel's task-for-pid policy is the default. Below that floor the terminal latches | Mapped |
| 4 | `perf_event_paranoid` at least 1 | No counterpart. The facilities that read another process's behaviour (`dtrace`, `ktrace`, `fs_usage`, `powermetrics`) need root, and SIP restricts them further. Dropped, and the reason is recorded here | Not applicable |
| 5 | `legacy_tiocsti` is 0 | `TIOCSTI` cannot be switched off | **Gap 1** |
| 6 | The shell is in the terminal's own, initial user namespace | macOS has no user namespaces. The counterpart is "not in a sandbox": neither the terminal nor the shell is under the App Sandbox (entitlement and `sandbox_check` read from the process), and both are in the same audit session | Mapped |
| 7 | `uid_map` and `gid_map` are the identity map | The shell's real, effective and saved user and group ids equal the terminal's (`proc_pidinfo`, `PROC_PIDTBSDINFO`), it has no set-id flag, and its audit token's user and session match the terminal's | Mapped |
| 8 | No `CAP_SYS_PTRACE` | macOS has no capabilities. What lets a process take another's task port is root, or the entitlements held only by Apple-signed tools. The counterpart is that neither the terminal, the holder nor the shell holds `get-task-allow` or `com.apple.security.cs.debugger`, and that the terminal is not running as root | Mapped |
| 9 | The terminal's pidfd for the shell is still unreadable | The shell is the terminal's own child and the terminal has not reaped it, so the pid cannot be reused; and a `kqueue` filter for the process (`EVFILT_PROC`, `NOTE_EXIT`) has not fired | Mapped |
| 10 | Start time, `comm` and `argv[0]` are recorded and still match | The audit token's process-version number, the start time (`pbi_start_tvsec`), the name and `argv[0]` are recorded at the ready point and compared at each command start. The version number changes on exec, and a `NOTE_EXEC` event is also watched | Mapped, stronger than Linux |
| the latch | A value below its floor latches the terminal | Same: SIP's task-port protection off, the task-for-pid policy below its default, or the terminal's own signing flags below the floor latch every layer of this terminal until it exits. A read that fails fails the check and does not latch | Mapped |

### Reading the table

**Conditions 3 and 3a together are the macOS form of Yama.** On Linux the kernel setting decides whether an unrelated process can trace a process. On macOS the decision is made per target, from its signature: the kernel refuses a task port for a platform binary or a hardened-runtime process that does not carry `get-task-allow`, with no authorisation prompt and for every ordinary caller. System Integrity Protection and the task-for-pid policy are the system-wide floor under it, so those two are what the terminal reads and latches on.

**The signing requirement is the new cost.** On Linux a user can build Bright and run it sealed. On macOS a terminal built from source has no hardened runtime unless it is signed with `--options runtime`, even with an ad hoc identity. Without it, a same-user process that signs itself with the debugger entitlement (a free, ad hoc signature) could attach to the terminal in the measurements, and the root key is in the terminal's memory. The first release therefore seals only a terminal and a holder that report the hardened runtime and no debugging entitlement when the terminal reads its own flags, and shows a notice that names the signing command otherwise. Release builds are signed and notarised, so this costs only people who build from source, and a documented command signs the result.

**Condition 8 is a check on entitlements, not on a privilege bit.** A debugger gets its power from entitlements that only Apple-signed tools hold, or from root. The terminal confirms its own and the holder's entitlements contain neither `get-task-allow` nor the debugger entitlement, and refuses to seal when it is running as root.

**Condition 10 improves.** Linux compares a start time, a name and `argv[0]`, and cannot see a program that execs another with the same name (a stated residual). The macOS audit token's version number changes on exec, so the case becomes detectable. It is not made impossible to defeat by a process that already runs code in the shell (that is same-user code execution inside the shell and stays out of scope), but the residual shrinks.

**Condition 4 has no counterpart and is dropped, not weakened.**

## Gap 1: input injection

On Linux, `TIOCSTI` lets any process that has the terminal as its controlling terminal push characters into the shell's input queue, so the shell runs a command the user did not type. Linux closes it with a sysctl. macOS has the same call and no switch. A process that is a descendant of the shell, and so has the terminal as its controlling terminal, can use it (measured, below).

What this lets an attacker do is make the sealed shell run a command. The shell would then report that command honestly in a sealed command-start frame, so the trust margin shows what ran. The attack does not forge a frame or an exit status. The parity rule therefore treats it as an accepted residual, provided the user can see that the command did not come from the keyboard.

The mitigation is the rule proposed in the Windows note, on every platform: the terminal counts the Enter keys it forwarded to the shell since the last command start, and a sealed command-start frame with no Enter behind it marks its block "input not typed in this window". The terminal draws the marker in the trust margin. It is one small change to the terminal and to the trust page, shared by all three platforms.

Synthetic keystrokes through the window system (accessibility permission granted to another application, `CGEvent`) reach the terminal window as typing. They need the user's consent in System Settings and are out of scope, as on every terminal.

## The key holder on macOS

The holder is a small separate executable with the Linux holder's one job and the same limits on what it does.

- **Started by the terminal, directly.** `posix_spawn` with the full path of a holder that sits beside the terminal. Both are inside the same signed application bundle or install directory. Only the descriptors the holder needs are passed, with the others closed on exec.
- **Signature.** The holder is signed with the hardened runtime, with the same team identity as the terminal, and with no entitlements at all. After the spawn, the terminal reads the holder's signing flags and entitlements with `csops` and confirms the hardened runtime is set, the signature is valid, `get-task-allow` is absent, and the team identity (or the code directory hash, for an ad hoc build) is the terminal's own. A holder that fails any of these is killed and the layer is grid-only.
- **Same protection for the terminal.** The root key lives in the terminal, so the terminal is held to the same flags, which it reads about itself at start.
- **Sandbox.** The holder enters a Seatbelt profile that denies everything (`(version 1)(deny default)`) after it has its descriptors, as the decoder worker already does. It then cannot open files, use the network or start a process. This is the counterpart of the Linux seccomp filter.
- **Limits.** Core dumps are off (`RLIMIT_CORE` is 0) and the key page is locked with `mlock` and is not inherited. Crash reports do not include memory. As on Linux this is hygiene, not a boundary.
- **Key delivery.** The terminal writes the key to the holder over a pipe that exists only between them and closes both ends. It never appears in the environment, a command line or on the terminal.
- **Lifetime.** The holder ends when the terminal's end of an EOF pipe closes, when the shell exits (a `kqueue` process filter on the shell's pid, as the Linux holder uses the pidfd), or when the seal channel closes.
- **No entitlements.** The design needs none beyond the hardened runtime itself. In particular it never asks for `get-task-allow`, `com.apple.security.cs.disable-library-validation`, `com.apple.security.cs.allow-dyld-environment-variables` or any of the memory-protection exceptions. The first spike confirms that the terminal's window and graphics code run under the hardened runtime without them.

## The seal channel

On Linux the seal channel is a socketpair whose end the shell inherits, and the kernel attaches the sender's credentials to every datagram. macOS has no per-message credentials on a Unix socket. What it has is the peer's pid, user and audit token at the time of `connect`, and for a socketpair the pid is that of the process that made the pair. A descriptor the shell passes to a child therefore lets the child send as the shell (measured, below). The Linux zsh case is already a stated residual. On macOS it must not become worse, so the design does not hand the shell a connected descriptor.

**The seal channel is a Unix stream socket by path, connected anew for each request.**

- The terminal creates a directory under the per-user temporary directory with a random name and mode 0700, checks it with `lstat` (owner, mode, not a symlink), and the holder binds the socket inside it before the shell starts. The path is under 104 bytes. This is the rule the Track B text already gives for macOS.
- The shell gets the path in an environment variable that the shim removes before it runs any user file, which also keeps later children from finding it that way. The path is not a secret and is not relied on as one.
- For each connection the holder reads the peer's audit token (`LOCAL_PEERTOKEN`): the process id must be the shell's pid that the terminal passed to it, the version number must equal the one recorded at the ready point, and the user must be its own. Any difference closes the connection and counts a refusal. The holder never acts on a message from a connection that failed these checks.
- Messages are the Linux ones: at most 128 bytes, a fixed layout, no free text, the same `ready` and `seal` requests, one request and one answer per connection.
- The shim opens the socket with zsh's own socket module for each request and closes it afterwards. A child of the shell can open the same path and is refused, because its pid is not the shell's.
- An unrelated process can open connections and hold them, which makes the shell's integration wait. The holder serves one connection at a time and closes any connection idle for 100 milliseconds. This matches the Linux residual: the layer can show less, never more.

An alternative is an XPC service, which authenticates every message by audit token and can require a code-signing requirement of the peer. It needs the service registered with the system's launch daemon and a private interface for the per-message token, which a command-line terminal cannot ship as a plain binary, so it is not proposed.

## Which shells

- **zsh, `/bin/zsh` (first).** The system zsh is the default login shell, a platform binary and protected by System Integrity Protection. The shim is the existing zsh `ZDOTDIR` shim, changed to open the socket by path (the descriptor-3 check is replaced by a check that the path variable is set and names a socket). It is the only listed shell.
- **`/bin/bash`** is a platform binary too, but the system bash is version 3.2 and the integration has no shim for it. It is grid-only in the first release.
- **A shell that is not a platform binary and not hardened** (Homebrew zsh or bash, fish, Nushell) can have its task port taken by a same-user debugger when developer tools are enabled, so it is not listed. If a user's shell is on the list but the check of its signature fails, the layer is grid-only with a notice.
- **A login shell chosen by the user** that is not `/bin/zsh` is grid-only, with a notice that names the listed shell.

## Threat model

| Adversary | In scope by default | What stops them on macOS | What is left |
|---|---|---|---|
| A program that only writes bytes (local, or remote through SSH) | Yes | Frames need a valid tag under a key only the holder has. Everything else is inert. | Nothing: the same as Linux |
| A same-user process, with no debugging entitlement (a script, a downloaded tool, a child of the shell) | Yes, as a boundary | The hardened runtime and platform-binary protection of the terminal, the holder and the shell; the socket checks (pid, version, user); no descriptor is handed to the shell | Gap 1 (input injection); starving the socket |
| A same-user process that signs itself with the debugger entitlement, or a debugger the user authorised | Yes, for processes it can attach to | Terminal, holder and shell are not attachable: platform binary, or hardened runtime without `get-task-allow` (measured). A terminal or shell that is not hardened is refused sealing | None for the listed processes |
| Root, or code running with a debugger entitlement | No (as root and `CAP_SYS_PTRACE` on Linux) | The terminal refuses to seal when it runs as root; SIP's task-port protection is a latched floor | Out of scope |
| A process holding accessibility permission | No: needs the user's consent | None | Synthetic keystrokes, as on every terminal |
| Another user | No | Directory mode 0700, user check on the socket | Out of scope |
| A planted dynamic library in the holder or the terminal | Same-user code execution, out of scope | Hardened runtime with library validation (no entitlement lifts it), environment variables for loading ignored | The shell's own modules and dotfiles, as on Linux |
| Name squatting on the socket path | Yes | The terminal creates and checks the 0700 directory and the holder binds before the shell starts | None |
| Descriptor inheritance into children | Yes | No connected descriptor is handed to the shell; per-request connection; pid and version check | None for sealing: a child can connect and is refused |

**Parity check.** The added surface is the holder, a Unix socket in a 0700 directory, and a signing requirement. None gives a same-user process the ability to produce an accepted frame, to read the key, or to learn anything it could not already learn from the process list. The socket gives an unrelated same-user process a way to make the shell wait. That is the existing Linux residual (a child that holds the channel) and not worse.

## Failure behaviour

- **Any condition fails, or cannot be read:** no holder is started, no key exists, the shell runs as a plain grid shell, and the notice names the failed check (for instance "the terminal is not signed with the hardened runtime" or "System Integrity Protection's task-port protection is off").
- **A value below its floor** (SIP's task-port protection, the task-for-pid policy, the terminal's own signing flags, running as root) **latches the terminal:** every layer of this terminal, including later ones, is grid-only until it exits, even if the value changes back.
- **A check at a command start fails** (the audit token version changed, the shell's start time or name changed, `NOTE_EXEC` fired, a debugger is attached to the shell as the process flags show): that layer stops sealing from then on and shows the notice. No frame sealed earlier is changed.
- **The holder ends, or the socket or its checks fail:** frames stop; the layer shows less and never more.
- **A damaged frame:** the existing failed-frame, abort and pin rules apply, unchanged.

## Implementation tickets, sized

S is a day or less, M is two to four days, L is a week or more. The order is the dependency order; 1 and 2 can run in parallel.

1. **Extend the measurements (S to M).** Repeat the probe on the oldest supported macOS and on the newest, on Apple silicon and Intel (Rosetta included), with a standard account, and with a read attempt after the attach to a plain binary, and check that the terminal's window and graphics code run under the hardened runtime with no entitlements. Fixes the version floor in condition 1. Output: a measurements file under `docs/proof/`.
2. **Signing and packaging (M).** Sign the terminal and the holder with the hardened runtime and no entitlements, for release (Developer ID, notarised) and for source builds (a documented command that signs with `--options runtime`). A build step that fails if an entitlement appears. The terminal reads its own flags at start.
3. **macOS probe and conditions in `lumen-seal` (L).** A macOS implementation of the `Probe` trait (`csops` flags and entitlements, `proc_pidinfo`, `proc_pidpath`, `sysctl`, the SIP check, the audit token, `kqueue` process events) and the macOS conditions with the same latch and decisions as Linux. Unit tests against fake probes for every row, and macOS tests against real processes: a platform binary, a hardened binary, a plain binary and a get-task-allow binary.
4. **macOS holder (L).** Spawn with a minimal descriptor set, the Seatbelt deny-default profile, `RLIMIT_CORE` 0, `mlock`, the self-check of signature and entitlements, exit on terminal exit, shell exit and channel loss. Tests that another same-user process cannot take its task port or read its memory.
5. **Path-socket seal channel (M).** The 0700 directory with the `lstat` checks, the stream socket, per-request connections, the audit-token checks, idle timeout, refusal counters. Tests for a squatted path, a child of the shell connecting, an inherited descriptor, a replayed pid and a flood.
6. **zsh shim on macOS (M).** The existing shim changed to connect by path with the socket module, the path variable removed before user files, the listed shell checked by path and platform signature, a non-listed shell refused.
7. **Input provenance marker (S to M, shared with the Windows note).** The forwarded-Enter count and the marker in the trust margin, on every platform. Needs the specification revision for the trust page.
8. **macOS sealing proof on a clean machine (L).** The sealing gates and the negative tests: a same-user process refused the task port and memory read of the terminal, holder and shell; an unsigned terminal gives grid-only; a Homebrew-style shell gives grid-only; a child refused on the socket; an inherited descriptor refused; an exec of the same name detected by the version number; SIP-off simulated by the fake probe in unit tests (not on the machine). Includes screenshots of the trust margin and the notice.
9. **Later: hardened mode on macOS, other shells, and the question of the Mac App Store sandbox (not sized here).**

Every ticket keeps the rules the Linux work keeps: features on by default, real behaviour tests, no new wire format, no sealing claim until the proof in item 8 passes.

## What the specification would change

- The platform page: the macOS section changes from "no keys" to the macOS conditions, the supported shell and the notices. A statement that a terminal built from source needs signing to seal.
- The sealed-layers page: the requirement list is written for Linux (`PR_SET_DUMPABLE`, close-on-exec descriptors, a socketpair). It needs one platform-neutral statement of each requirement with a Linux, a macOS and a Windows column. This is shared with the Windows note.
- The transport page: the macOS seal channel as a path socket, and the rule that no connected descriptor is passed to the shell.
- The trust page: the "input not typed in this window" marker (shared).
- The threat model: Gap 1 and the signing requirement as accepted residuals, with their effects only.
- The hardened-mode page: nothing now; a macOS hardened mode is a separate design.
- The conformance page: the macOS sealing gates, after the proof exists.

## Open questions for review

1. Is the first release allowed to seal only for a signed terminal, with source builds grid-only? The alternative is to seal an unsigned terminal and say in the notice that a same-user process with developer tools can read the key.
2. Is `/bin/zsh` as the only listed shell acceptable for the first release? Supporting a hardened, user-signed shell is possible later.
3. Which macOS versions are supported? The design wants the audit-token socket option, the task-port policy and `kqueue` process events; the first ticket fixes the floor.
4. Is the input provenance marker wanted on all platforms, or only where the platform cannot close injection? (The same question is in the Windows note.)
