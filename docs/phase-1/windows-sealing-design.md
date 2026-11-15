# Sealing on Windows: design note

Status: a design for review. It changes no code and no specification text. A specification revision follows only after this note is signed off.

Today the specification builds sealing for Linux only ([platforms](../../spec/v0/platforms.md#platforms.windows) says v0 creates no keys on macOS, the Windows section says images are unsupported, and the sealing conditions in `crates/lumen-seal/src/conditions.rs` begin with "the terminal runs on Linux"). Bright is meant to run on macOS, Windows and Linux. This note is the Windows half of sealing. It says what each Linux condition becomes on Windows, what Windows cannot give, how a sealed frame travels through the Windows console, and what to build, in what order.

"Sealed" has its specification meaning: a layer whose command structure, exit status and capability grants come from frames authenticated with per-layer keys that only a separate key holder has. Everything below keeps that meaning and keeps the parity rule of the [threat model](../../spec/v0/threat-model.md#threat.parity): nothing added for Windows may give a same-user process more than writing bytes to the terminal already gives it.

## Summary

- **Sealing on Windows is feasible for PowerShell, with two gaps stated below.** A shell that the terminal starts directly, with a restricted security descriptor and token, in a terminal whose own token has no debug privilege, can meet the same intent as the Linux conditions. The Windows mechanisms are different, so the conditions are re-derived, not copied.
- **The carrier is the hard part.** Sealed frames are binary escape strings on the PTY. On Windows the PTY is the pseudo-console, which drops the escape string (system build) or changes every byte at or above 0x80 and turns LF into CR LF (the build shipped with the terminal). A binary frame cannot survive either. The proposal is a 7-bit envelope around the unchanged sealed frame, used only where the console is a text channel. The frame, its header and its authentication do not change; the envelope is new and needs a specification revision.
- **Gap 1: input injection.** Any process attached to the shell's console can write input events into it, with no kernel setting that turns that off, and no check can prove a command was typed here. The design shows a per-layer "input is not isolated" indicator and keeps a per-block marker as a partial signal; whether that is enough is an open question.
- **Gap 2: no `ptrace_scope`.** The Windows counterpart is a restrictive process security descriptor on the shell and the holder, set when the terminal creates them. It stops a same-user process that is not elevated. It does not stop a process holding the debug privilege, which is the same boundary as `CAP_SYS_PTRACE` on Linux.
- **Failure behaviour is the Linux one.** Any failed or unreadable check means no holder, no keys, a grid-only layer and a notice that names the check. A value below its floor latches the whole terminal.
- **Not covered in this first Windows release:** `cmd.exe`, shells in WSL (each is grid-only, see "Which shells"), and hardened mode.

## What is being protected

The Linux design has four parts, and each needs a Windows answer:

1. **A key holder** that is a separate process, that never starts another process, and that same-user programs cannot read or debug.
2. **A seal channel** between the shell's integration and the holder, on which the holder can tell the shell apart from every other process, including the shell's own children.
3. **Conditions** on the shell and the system, read at the layer's start and again at each command start, which decide whether the layer may have keys at all.
4. **A carrier** that takes the holder's sealed frame, written by the shell on the PTY, to the terminal unchanged.

## Windows facts the design rests on

These are properties of the platform, with the ones this note could not check from the repository marked for the first spike.

- A process object has a security descriptor. The `lpProcessAttributes` argument of `CreateProcess` sets it. The handle that `CreateProcess` returns has full access whatever the descriptor says, so the creator keeps control while other processes of the same user can be refused rights such as `PROCESS_VM_READ`, `PROCESS_VM_WRITE`, `PROCESS_VM_OPERATION`, `PROCESS_CREATE_THREAD`, `PROCESS_DUP_HANDLE`, `PROCESS_SET_INFORMATION` and `PROCESS_SUSPEND_RESUME`. The owner's implicit `READ_CONTROL` and `WRITE_DAC` are removed with an explicit `OWNER RIGHTS` entry.
- The debug privilege (`SeDebugPrivilege`) bypasses a process descriptor. A standard, un-elevated token of a user in the Administrators group holds it disabled and cannot enable it, but a token of an account with User Account Control off holds it enabled. A token can be created without it (`CreateRestrictedToken` with `DISABLE_MAX_PRIVILEGE`).
- Protected processes need a Microsoft-signed anti-malware identity. Bright cannot use them.
- Process mitigation policies can be set when a process is created (`PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`): no dynamic code, images only from the system directory and never from a remote or low-integrity location, no extension points, no win32k calls, no child processes (a job object limit), control-flow guard.
- A named pipe server reads the client's process id with `GetNamedPipeClientProcessId`. That is the process that opened the client end. A handle the client end passes to a child by inheritance or duplication does not change it, and the server sees no per-message sender. This is the main difference from `SCM_CREDENTIALS`, which the kernel attaches to every datagram.
- A handle to a process keeps its id from being reused while the handle is open. The terminal's handle from `CreateProcess` is the counterpart of the Linux pidfd.
- Programs attached to one console share its input buffer. Whether an unrelated process of the same user can attach to the shell's pseudo-console is checked in the first spike.
- The system pseudo-console and the one shipped with the terminal (Microsoft's `conpty.dll` and `OpenConsole.exe`) behave differently for escape strings: measured earlier on Windows Server 2022, the system one drops the string; the shipped one carries it, turns LF into CR LF and re-encodes every byte at or above 0x80 as UTF-8 from the OEM code page, with or without its pass-through mode. Strings of 7-bit bytes without LF or CR were not measured.

## The Linux conditions, one by one

Numbers are those in `conditions.rs`. "Gap" means Windows cannot provide the guarantee and the design says what it does instead.

| # | Linux condition | Windows counterpart | Status |
|---|---|---|---|
| 1 | The terminal runs on Linux | The terminal runs on Windows 10 version 1809 or later or Windows 11 (the versions with the pseudo-console), on x64 or Arm64, in a session that is not an AppContainer and not a Windows container silo | Mapped |
| 2 | Listed shell, run directly by absolute path, no wrapper, no `-c`, `comm` is the listed name | See "Which shells" below: the terminal creates the shell itself with a command line it built, the image path and file identity of the running process match a listed entry, and its Authenticode signer is the listed publisher | Mapped, different mechanism |
| 3 | Yama `ptrace_scope` at least 1 | The shell is created with a security descriptor that refuses the debugging and memory rights to every other principal, read back from the terminal's own handle at the ready point and at each command start | Mapped, per process, not a system setting |
| 3t | (new) the terminal is itself unreachable | The terminal is started with the restrictive descriptor (see "Protecting the terminal itself"), reads its own descriptor back at the ready point and at each command start, and finds no other process holding a handle to it with memory, thread, duplicate-handle or suspend rights | Mapped, new |
| 4 | `perf_event_paranoid` at least 1 | No counterpart: Windows has no per-task counter interface open to an ordinary user that reads another process's memory. Event tracing for Windows needs an elevated token. The condition is dropped and the reason is recorded here | Not applicable |
| 5 | `legacy_tiocsti` is 0 | Console input injection cannot be switched off and cannot be fully detected | **Gap 1** |
| 6 | The shell is in the terminal's own, initial user namespace | The shell's token is not an AppContainer token, its session and logon session are the terminal's, and it is not in a different silo or job that the terminal did not create | Mapped |
| 7 | `uid_map` and `gid_map` are the identity map | The shell token's user SID, integrity level and elevation type equal the terminal's, and it is not impersonating | Mapped |
| 8 | No `CAP_SYS_PTRACE` in `CapInh`, `CapPrm`, `CapEff` or `CapAmb` | The shell's token holds no `SeDebugPrivilege` (enabled or disabled) and no `SeTcbPrivilege`; the terminal's own token holds no `SeDebugPrivilege` either, because every program the user starts from other routes carries a token like the terminal's | Mapped |
| 9 | The terminal's pidfd for the shell is still unreadable | The terminal's process handle for the shell is still not signalled (`WaitForSingleObject` with a zero timeout) | Mapped |
| 10 | Start time, `comm` and `argv[0]` are recorded and still match | Creation time, full image path and command line are recorded at the ready point and compared at each command start. Windows has no `exec`, so a changed value means a different process at that id, which the handle already rules out; the comparison stays as a check on the handle and on the record | Mapped, mostly redundant |
| the latch | A value below its floor latches the terminal | Same: a token that holds `SeDebugPrivilege`, or a descriptor that no longer has the restrictions, latches every layer of the terminal until it exits. A read that fails fails the check and does not latch | Mapped |

### Reading the table

**Conditions 3 and 8 together are the Windows form of Yama.** On Linux, `ptrace_scope` of 1 means a process can trace only its own descendants, and the capability check keeps a descendant from gaining the right to trace anything. On Windows the shell's descriptor refuses other processes the memory and debugging rights, and the token check keeps every descendant of the shell (which inherit its token) from holding the privilege that bypasses a descriptor. The shell can still be terminated, queried and waited on, so Task Manager and `taskkill` keep working. A debugger cannot attach to the user's sealed shell. That is the same cost Yama has on Linux and it is stated in the user documentation.

**Condition 3t makes the boundary start at the terminal.** The terminal is the process that holds the creator handles and the key before it is delivered, so it has to be unreachable before either exists. It is created by a launcher with the restrictive descriptor, and it checks its own descriptor and the system's open handles to it. "Protecting the terminal itself" has the detail.

**Condition 8 needs the terminal's own token to lack the debug privilege.** An account that always runs elevated, or with User Account Control off, has an attacker-reachable token that can enable `SeDebugPrivilege` and open the shell whatever its descriptor says. The check is on the terminal's token because the attacker's token is its twin. When the privilege is present the layer is grid-only and the latch is set, with a notice that names User Account Control. This costs sealing for people who run Windows with UAC off, and it is the honest answer: on Linux the equivalent is a user running as root.

**Condition 4 has no counterpart and is dropped, not weakened.** The design does not invent a Windows setting to stand in for it.

**Condition 10 shrinks.** Linux needs it because a shell can `exec` another program at the same pid. A Windows process keeps its image for life, so the open process handle is the stronger check. The record and the comparison are kept as a second reading. The Windows form of the "same-name re-exec" residual is code loaded into the shell (a module, a DLL), which is same-user code execution and out of scope by default.

## Gap 1: console input injection

On Linux, `TIOCSTI` lets any process that has the terminal as its input push characters into the shell's input queue, so the shell runs a command the user did not type. Linux closes it with a sysctl. On Windows every process attached to the console can write input events into the shell's input buffer (`WriteConsoleInput`), and there is no switch. A same-user process outside the console may also attach to it (to be checked in the first spike).

What the gap lets an attacker do: make the sealed shell run a command, or change part of a command the user is typing. The shell reports the command honestly in a sealed command-start frame, so the trust margin shows what ran. The attack does not forge a frame or an exit status. It uses the shell as an ordinary same-user process could already use any shell it can start itself. The parity rule therefore judges it an accepted residual, not a defect, **provided** the user is told that the layer's input is not isolated.

**What can and cannot be detected.** A count of the Enter keys the terminal forwarded, compared with the command starts the shell reports, catches a command that arrives with no Enter behind it. It does not catch injected text that is mixed into a line the user then submits: the user's Enter is forwarded, the frame has an Enter behind it, and part or all of the command was injected. The terminal cannot compare the text it forwarded with the command the shell reports, because line editing, completion and history change the text between the keys and the command. So no check this design can make establishes that a command was typed in this window.

The design therefore has two parts, and states the limit:

1. **A per-layer indicator.** Every sealed layer on Windows shows, in the terminal's own chrome and never from output, that this layer's input is not isolated from other programs of the user. It is shown for the life of the layer and is part of what sealing means on Windows. It is a notice about the platform, not about a single command.
2. **A per-block marker for the case that can be detected.** A sealed command-start frame with no forwarded Enter behind it marks its block "input not typed in this window". It is a partial signal and is documented as one.

If the maintainers decide that an indicator is not enough, the alternative is to keep Windows layers grid-only until the console input can be isolated. That is a product decision, listed in the open questions. The first measurement covers the mixed interleavings (user keys and injected keys arriving in one line, injection between two forwarded keys, injection during a paste) so the limit is stated from measurements.

The two parts are a small change to the terminal and to the specification's trust page. The macOS note has the same gap with the same limit, and the same two parts apply there.

Injected keystrokes through the window system (`SendInput`, accessibility tools) reach the terminal's own window as if typed. No terminal on any system can tell them apart without hooks that this design does not add. They are out of scope, the same as on macOS and X11.

## Protecting the terminal itself

The shell and the holder get a restrictive descriptor when the terminal creates them. The terminal is the process that holds the creator handles to both, the key before it reaches the holder, and the handles the holder inherits. A process created by the user's shell or by Explorer gets a default descriptor derived from the user's token, which lets any process of the same user open it with memory, thread and handle-duplication rights. A same-user process could use that to duplicate a powerful handle out of the terminal, to write into it before a key is delivered, or to take a creator handle while it exists. The design closes this before any sealing handle or key exists:

- **The terminal starts protected.** The terminal binary the user starts is a small launcher. It creates the real terminal process with `CreateProcess`, the same restrictive descriptor, token and mitigation policies as the holder, and then exits. The real terminal therefore never exists with a default descriptor, which a change to its own descriptor after start could not guarantee: a handle another process opened in the first moments keeps the rights it was granted. The launcher holds no key and no sealing handle.
- **The terminal checks it.** At the ready point and at each command start, the terminal reads its own descriptor back and enumerates the system's open handles (the system handle table), and finds none that another process holds to the terminal, with the memory, thread, duplicate-handle, suspend or set-information rights. Any hit is a failed check. A descriptor weaker than set, or a handle found, is a value below its floor and latches. The handle-table call is an undocumented interface that the process tools use, so the first measurement confirms it works for an ordinary user on the supported versions, and the design fails closed where it does not.
- **Creator handles are downgraded.** After the shell and the holder are verified, the terminal duplicates each creator handle to the rights it still needs (wait, query, terminate) and closes the full-access handle. The key pipe's write end is closed when the key has been written. No full-access handle to the holder or the shell exists after setup.
- **Handles are created non-inheritable** and passed only through an explicit handle list, so no later child inherits one.
- **The same checks are tests:** an unrelated same-user process asking for each of those rights on the terminal is refused; a handle opened before the check is found and the layer is grid-only.

## Gap 2: same-user code with the debug privilege

A process that holds `SeDebugPrivilege` can open the holder or the shell whatever the descriptor says, read the key and forge frames. That is the Linux `CAP_SYS_PTRACE` and root case, which the threat model already puts out of scope in default mode. Condition 8 makes sure the privilege is absent from the terminal's token, so an ordinary non-elevated user does not have the capability to begin with. Hardened mode on Windows is a separate design and is not part of this note.

## The key holder on Windows

The holder is a small separate executable with the Linux holder's one job and the same restrictions on what it does.

- **Created by the terminal, directly.** `CreateProcess` with `EXTENDED_STARTUPINFO_PRESENT`, the full path of a holder binary that sits next to the terminal and whose hash is checked at start against the build's record.
- **Process descriptor.** A descriptor with the same restrictions as the shell's. Only the creator's handle has more than query and wait rights, and the creator is the protected terminal; its handle is downgraded after setup (see "Protecting the terminal itself").
- **Token.** A restricted token with all privileges removed except `SeChangeNotifyPrivilege`, medium integrity level, with the user SID kept (so the pipe checks can name the user).
- **Mitigations.** At creation: dynamic code prohibited, images only from the system directory and never remote or low-integrity, extension points disabled, win32k calls disabled, control-flow guard, strict handle checks.
- **Holder job.** A job of its own, never the job of the layer's shell and its programs. One active process, no child processes, no user interface, kill on close. The terminal holds the job handle, so the holder ends when the terminal ends. This is the counterpart of the Linux EOF pipe. The die-on-unhandled-exception limit used for the decoder worker is not relied on to prevent a dump: it only turns off the error dialog.
- **Handles.** The holder inherits exactly the handles named in `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` and nothing else: the key pipe, an event for the terminal's exit and a duplicate of the terminal's handle for the shell with query and synchronise rights only (the pidfd counterpart). The terminal never makes its handles inheritable in general.
- **Key delivery.** The terminal writes the key to the holder through an anonymous pipe that exists only between the two, and closes both ends. The key does not appear in the environment, in a command line or on the console.
- **Memory and crash dumps.** The key page is locked with `VirtualLock`, which keeps it out of the paging file but does not keep it out of a dump. Windows Error Reporting can write a full dump of a crashed process (the `LocalDumps` setting) into the user's own profile folder, whatever the job says, and a same-user process could read it. The design therefore has four parts:
  1. **A condition that fails closed.** At the ready point the terminal reads the Windows Error Reporting dump settings (the global and the per-program `LocalDumps` keys, which only an administrator can write). If dump collection is configured so that it could capture the holder, the layer is grid-only, with a notice that names the setting. It is a value below its floor and latches.
  2. **The holder never reaches the default fault path with the key in memory.** It installs a top-level exception filter and a panic hook that zero the key and end the process at once, and it is built so that an abort goes through the same hook rather than the fail-fast path that reports to Windows Error Reporting.
  3. **The key is not at rest in the clear.** Between requests it is held encrypted with `CryptProtectMemory` for the same process, whose key the system holds outside the process, and it is decrypted into a small buffer for one seal operation and zeroed. A dump taken between requests holds ciphertext.
  4. **What is left is stated.** A fault that bypasses the hook during a seal operation, with dump collection configured by an administrator after the ready point, could leave the key in a dump. The condition is read again at each command start, so the layer stops sealing at the next command, but that dump would still exist. That is a same-user key-recovery gap, accepted as a residual with its effect only, and the proof ticket tests the first three parts.
- **Packaging.** The holder is a statically linked binary with no dependency beyond the system libraries. The dependency audit and the unsafe-code report cover it like any other crate.

**Two jobs, never one.** The holder is in its own job with a one-process limit, no child processes and no user interface, because it must never start anything. The layer's shell and every program it starts are in a different job, the layer job, which has no process-count or user-interface limit (a shell starts many processes and some of them have windows), no breakaway, and kill on close. The holder is not in the layer job and the shell is not in the holder job. The terminal holds both job handles. The sealing checks use process ids and handles, not job membership; the layer job is used by the images design, whose pipe accepts a connection only from a process in it.

There is no seccomp counterpart. The holder job, the token with no privileges and the mitigation policies are the Windows way of limiting what the process can do, and the holder never starts anything, opens no file after start and has no network, because of its token and its job.

## The seal channel

On Linux the seal channel is a `SOCK_SEQPACKET` socketpair whose end the shell inherits, and the kernel attaches the sender's credentials to every message. Windows has no equivalent per-message credential. A pipe handle the shell passes to a child lets the child send messages that look as if the shell sent them. The Linux zsh case (the shell's children inherit the descriptor) is already a stated residual. On Windows it must not become worse, so the design does not hand the shell a connected handle.

**The seal channel is a named pipe, connected anew for every request.**

- The holder creates the pipe, with `FILE_FLAG_FIRST_PIPE_INSTANCE` so a name squatted earlier is refused, `PIPE_REJECT_REMOTE_CLIENTS`, message mode, and one instance at a time. Messages are the Linux ones: at most 128 bytes, a fixed layout, no free text, and the same `ready` and `seal` requests.
- The pipe name is `\\.\pipe\bright-seal-<random>`. It is not a secret. The terminal gives it to the shell in an environment variable that the shim removes before it runs any user file, which also stops later children from finding it by that route. A process can list pipe names, so the name is not relied on.
- The pipe's access list grants connect only to the user SID. Another user cannot open it.
- For each connection the holder reads `GetNamedPipeClientProcessId` and compares it with the process id of the handle the terminal duplicated into it, checks the creation time of that process against the one recorded at the ready point, and checks that the client token's user SID is its own (the connection is accepted at the identification level only). Any difference closes the connection and counts a refusal. The holder never acts on a message from a connection that failed these checks.
- The shell integration opens the pipe, sends one request, reads one answer and closes. It never keeps a connection, so a child of the shell cannot reuse one. A child can still open the pipe by name, and is refused because its process id is not the shell's.
- An unrelated process can open connections and refuse to close them, which makes the shell's integration wait. The holder serves one connection at a time, closes any connection idle for 100 milliseconds and answers one request per connection. The result matches the Linux residual: the layer can show less, never more.

A stronger variant adds a per-layer random restricting SID to the shell's token and grants only that SID on the pipe, so unrelated processes cannot connect at all. It changes every access check of the user's shell, so it is not in the first release. It is listed as an option below.

## Which shells

- **PowerShell (first).** Both PowerShell 7 (`pwsh.exe`) and Windows PowerShell 5.1 (`powershell.exe`), each identified by absolute path, file identity (volume serial and file index of the running image) and Authenticode signer. A path is not enough, because the Microsoft Store build runs from a versioned directory. The terminal resolves the listed shell to its path at start and reads the signer with the system trust check from the file it is about to run.
- **The shim.** The terminal starts the shell with `-NoProfile -NoExit` and a fixed startup script that lives beside the terminal. The script reads the pipe name, removes the variable, defines the hooks (prompt, command start, command end) and then loads the user's profiles in the shell's own order, so user code runs after the shim has finished its work. A shell that is not interactive, or one whose language mode is not full language (a constrained language mode set by policy), gets no hooks and so no keys. The `-NoProfile` form is the startup script, not a user command, and the terminal builds the whole command line; condition 2's "no wrapper, no `-c`" is read as "no command line the user supplied". The terminal reads the running process's command line at the ready point and compares it with the one it built.
- **`cmd.exe`.** It has no hook points for prompt and command boundaries that a script can set without a helper program in the path. It stays grid-only in the first release. A helper (a prompt that calls the holder) belongs to a later design and is outside this note.
- **WSL.** A shell in WSL is a Linux program inside a separate kernel. The terminal sees `wsl.exe` and cannot read the Linux conditions, so the layer is grid-only, with a notice. Running the Linux build of Bright inside WSL, with its own window and its own Linux conditions, is the supported way to get sealing there.
- **Other listed shells (Git Bash, MSYS2, Nushell)** are not listed in v0 for Windows.

## How a sealed frame gets through the console

The shell writes the sealed frame on the PTY, which is the pseudo-console on Windows. The system pseudo-console drops the frame's escape string. The one shipped with the terminal carries it, with LF turned into CR LF and every byte at or above 0x80 turned into UTF-8 text. A binary frame is damaged either way, and a damaged frame fails authentication. That means no sealed frame arrives, and the layer would show the failure notice and pin to grid mode.

The options:

| Option | What changes | Verdict |
|---|---|---|
| Use the system pseudo-console | Nothing | Does not carry any escape string. Not workable. |
| Ship the pseudo-console, send binary frames | Nothing in the frame | Bytes at or above 0x80 and LF are changed. Not workable. |
| Ship the pseudo-console, send the frame in a 7-bit envelope | A new carrier form for Windows. The sealed frame inside is unchanged. | **Proposed** for small frames. |
| A second channel (a named pipe) with an authenticated 7-bit marker on the console | A new channel and a new marker form | Needed for large content. It is the same design as the images note. |
| Avoid the pseudo-console | Not available: every Windows console program needs it | Rejected. |

**The envelope.** Where the console is a text channel, the shell writes the whole sealed frame, header and tag included, as base64url text with no padding inside one string with its own introducer (the exact introducer and grammar are for the specification revision; the shape is `ESC _ <marker> <base64url(sealed frame)> ESC \`). The string holds no LF, no CR and no byte at or above 0x80. The terminal removes the envelope in the single frame pipeline, before the header is read, and then handles the inner frame exactly as it handles a Linux frame. It grows a frame by one third, and it has to fit the existing caps on a string in flight (the Track 0 string cap is 4 KiB), which command frames do. A frame that fails to decode, fails authentication or is cut short is a failed frame with all the existing rules (inert rendering, the failure count, the pin to grid mode). The envelope carries no authority: it is a path for bytes, and anything a program writes in the same form is a forged frame like any other and is refused for want of a valid tag.

What the envelope depends on, and the first spike measures:

- Whether the shipped pseudo-console passes a string of printable 7-bit bytes unchanged, for the forms `ESC _`, `ESC ]` and `ESC P`, and at what length it starts to split, wrap or drop it.
- Whether the output re-rendering in the shipped console ever inserts a line break inside a long string at the buffer width.
- The rate at which it can carry them, and what resize and the alternate screen do to strings in flight.
- Whether the frame's own limit of 1 MiB is useful here. Command frames are well under 1 KiB, so the envelope is for them. Large frames take the second channel.

If the spike finds that the shipped pseudo-console cannot carry a printable string reliably, the in-band path is closed and sealing on Windows needs the second channel for everything, with a 7-bit authenticated marker as the ordering token. The sizing below allows for either outcome.

**Third-party code in the path.** The shipped `conpty.dll` and `OpenConsole.exe` are Microsoft code (MIT licence) in the path of every byte. They are a carrier: they cannot forge a frame, and they can drop or delay one, which fails closed to a grid-only layer. They need a pinned version, hash checks at start, a notice in the licence file and a place in the dependency audit. They are not a crate, so the supply-chain tooling does not cover them and the audit page must.

## Threat model

Adversaries, in the order the specification uses:

| Adversary | In scope by default | What stops them on Windows | What is left |
|---|---|---|---|
| A program that only writes bytes (local, or remote through SSH or WSL) | Yes | Frames need a valid tag under a key only the holder has. The envelope and the Track 0 rules treat anything else as inert. | Nothing: the same as Linux |
| A same-user process that cannot debug (a script, a downloaded tool, a child of the shell) | Yes, as a boundary (it is what Yama stops on Linux) | The descriptors on the terminal, the shell and the holder, set at creation; the handle check; the downgraded creator handles; the pipe checks (pid, creation time, user); the token without debug privilege. The holder is never reachable by handle. | Gap 1 (input injection); starving the pipe |
| A same-user process that holds the debug privilege, or an elevated process | No (as root and `CAP_SYS_PTRACE` on Linux) | Condition 8 refuses sealing when the terminal's own token has the privilege | The privilege is present only on UAC-off or always-elevated accounts, where the layer is grid-only |
| Another user, the system | No | The pipe's access list and the descriptors | Out of scope |
| A hostile or broken pseudo-console | Partly | Authentication; the carrier cannot forge | Dropped frames and a grid-only layer |
| A planted DLL in the holder or the shell | Same-user code execution, out of scope | The holder's mitigation policies and system-only image loading; the shell is not protected from its own modules | The shell's modules, as on Linux |
| Name squatting on the pipe | Yes | `FIRST_PIPE_INSTANCE` before the shell starts; the holder creates it | None |
| A crash dump of the holder | Yes, as key recovery by a same-user process | The dump-settings condition, the exception hook and the key encrypted at rest | A fault in a seal operation with dumps enabled after the ready point |
| Handle inheritance into children | Yes | No connected handle is handed to the shell; per-request connection; pid check | None for sealing; a child can try to connect and is refused |

**Parity check.** The added surface is a named pipe (reachable by name by any same-user process, which is refused after one cheap check), the holder process and the envelope. None of them gives a same-user process the ability to produce an accepted frame, to read the key, or to learn anything that it could not read from the process list. The pipe gives an unrelated same-user process a way to make the shell wait. That is the existing Linux residual (a child that holds the channel) and not worse.

## Failure behaviour

- **Any condition fails, or cannot be read:** no holder is started, no key exists, the shell runs as a plain grid shell, and the notice names the failed check (for instance "the terminal's token holds the debug privilege" or "the shell's security descriptor is not the one the terminal set").
- **A value below its floor** (the debug privilege in the terminal's token; a shell or terminal descriptor found to be weaker than set; a handle to the terminal found with the dangerous rights; dump collection configured for the holder) **latches the terminal:** every layer of this terminal, including later ones, is grid-only until it exits, even if the value changes back.
- **A check at a command start fails:** that layer stops sealing from then on and shows the notice. No frame sealed earlier is changed.
- **The holder ends, or the pipe or its checks fail:** frames stop; the layer shows less and never more.
- **A damaged or dropped frame:** the existing failed-frame, abort and pin rules apply, unchanged.
- **The shipped pseudo-console is missing, or its hash does not match:** the terminal runs the system one, which carries no strings, so every layer is grid-only, with a notice. No feature that depends on frames is offered.

## Implementation tickets, sized

S is a day or less, M is two to four days, L is a week or more. The order is the dependency order; 1 and 2 can run in parallel.

1. **Carrier and input spike on a clean Windows machine (M).** Measure what the shipped pseudo-console does to printable 7-bit strings of the three string forms, by length, rate, resize and alternate screen, whether an unrelated same-user process can attach to the shell's pseudo-console and write input, the mixed user and injected input interleavings (injected text inside a line the user submits, between two forwarded keys and during a paste), and whether the system handle table works for an ordinary user on the supported versions. Output: a measurements file under `docs/proof/`, and a decision on the envelope or the second channel for everything. No product code; a throwaway program is allowed.
2. **Shipped pseudo-console as a pinned component (M).** Pin the version, verify hashes at start, fall back to the system console with the grid-only notice, add the licence notice and the dependency audit entry, and the release packaging step.
3. **Windows probe and conditions (L).** A Windows implementation of the `Probe` trait (token privileges, token user, integrity and elevation, session and logon session, AppContainer, process security descriptor, creation time, image path and file identity, command line, signer), and the Windows conditions in `lumen-seal` with the same latch and decisions as Linux. Unit tests against fake probes for every row of the table, and Windows tests against a real process for the descriptor, the token and the signer checks.
4. **Protected terminal start and Windows holder process (L).** A launcher that creates the terminal with the restrictive descriptor, token and mitigations; creator handles downgraded after setup; the holder created with descriptor, token, mitigations, its own job and handle list; the dump-settings condition, the exception hook, the panic hook and the key encrypted at rest; key delivery over an anonymous pipe; `VirtualLock` and zeroing; exit on terminal exit, shell exit and channel loss. Tests that another same-user process cannot open the holder with memory or debug rights.
5. **Named-pipe seal channel (M).** The pipe with first-instance, remote-client rejection, message mode, per-request connections, pid, creation time and user checks, idle timeout, refusal counters. Tests for a squatted name, a child of the shell connecting, a replayed pid, and a flood.
6. **PowerShell shim and startup (L).** The startup script for `pwsh.exe` and `powershell.exe`, profile chaining, hooks, the constrained-language and non-interactive refusals, the command-line check, the signer and file identity checks for listed shells.
7. **Envelope in the frame pipeline (M).** Writing the envelope in the shell integration and removing it in the terminal's single pipeline, with the failed-frame rules unchanged. Needs the specification revision.
8. **Input indicator and marker (S to M).** The per-layer "input is not isolated" indicator in the chrome on Windows and macOS, and the forwarded-Enter count with the "input not typed in this window" marker as a partial signal, on every platform. Needs the specification revision for the trust page.
9. **Windows sealing proof on a clean machine (L).** The sealing gates and the new negative tests on a Windows machine: a same-user process refused the memory, thread, handle-duplication and debug rights on the terminal, the holder and the shell; a handle opened to the terminal before the check found and the layer grid-only; dump collection configured gives grid-only and a crash of the holder with it unset leaves no dump with the key; a token with the debug privilege gives grid-only and a latch; a child process refused on the pipe; a squatted pipe name; a dropped or damaged envelope; the shell started with a user-supplied command line gives grid-only; UAC off gives grid-only. Includes screenshots of the trust margin and of the notice.
10. **Optional, later: a restricting SID on the shell token (M)** to stop unrelated same-user processes from connecting to the pipe at all, after its effect on the user's shell is measured.
11. **Later: hardened mode on Windows, and `cmd.exe` integration (not sized here).**

Every ticket keeps the rules the Linux work keeps: features on by default, real behaviour tests, no new wire format beyond the envelope, no sealing claim until the proof in item 9 passes.

## What the specification would change

For the revision that follows sign-off, so the reviewers know the reach of this note:

- The platform page: the Windows section changes from "no keys" to the Windows conditions, the supported shells and the notice text.
- The sealed-layers page: the list of requirements is written for Linux (`PR_SET_DUMPABLE`, close-on-exec descriptors, a socketpair). It needs a platform-neutral statement of each requirement and a Windows column.
- The transport page: the envelope as a carrier form, its exact grammar and size limit, and the rule that the inner frame is unchanged.
- The trust page: the "input not typed in this window" marker.
- The threat model: the two gaps above as accepted residuals, with their effects only.
- The conformance page: Windows sealing gates, after the proof exists.

## Open questions for review

1. Does the first release require Authenticode for the listed shells, or the path and file identity only? Requiring it refuses a shell the user built from source.
2. Is losing sealing on accounts with UAC off acceptable? The alternative is to seal anyway and say in the notice that a same-user process with the privilege can read the key.
3. Should the shipped pseudo-console be a hard requirement for any Windows sealing, or should a Windows build without it run grid-only (the proposal)?
4. Is a per-layer "input is not isolated" indicator with a partial per-block marker enough for Windows and macOS, or should those layers stay grid-only until the console input can be isolated? (The same question is in the macOS note.)
5. Which Windows versions count as supported: 1809 and later (the first with the pseudo-console), or only versions with the pass-through mode?
