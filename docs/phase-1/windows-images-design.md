# Images on Windows: design note

Status: a design for review. It changes no code and no specification text. A specification revision follows only after this note is signed off.

Bright shows images on Linux and macOS. On Windows a build has no gate record, stays locked, and shows the alt text ([platforms](../../spec/v0/platforms.md#platforms.windows)). The reason is the console: a program's image frames are binary, and the Windows pseudo-console either drops the escape string they travel in or changes the bytes inside it. This note designs a way to deliver the frames intact. It is written to go with the note on sealing on Windows (`docs/phase-1/windows-sealing-design.md`, a separate change), because both need the same two things: a carrier that survives the console, and a channel beside it.

## Summary

- **Two paths, one pipeline.** A 7-bit text envelope carries the small control frames in-band, and a named pipe carries the image bytes out of band. Both feed the terminal's existing image reassembly and the same sandboxed decoder, so the budgets, limits, ordering rules and decoder gates are unchanged.
- **The pipe is tied to the layer by the layer job.** The terminal puts each shell it starts in a job object of its own, the layer job, which holds the shell and every program it starts. A connection to the image pipe is accepted only from a process in that job. A program outside the layer cannot feed it. The key holder in the sealing note is in a different job, so the two sets of limits never meet.
- **The in-band envelope is needed in either case.** The image start frame is binary too, so on Windows it must travel in the envelope. If the first measurement shows the pseudo-console also carries long printable strings, the envelope alone could carry the bytes, and the pipe becomes an optimisation. This note recommends designing for the pipe now, because the length the console accepts is not known.
- **The pipe does not share the seal channel.** The seal channel connects the shell to the key holder and carries 128-byte requests from one process. The image pipe connects any program in the layer to the terminal and carries bulk, unsigned bytes. Mixing them would put unsigned bulk next to the key holder. They share a design pattern and a measurement, not an instance.
- **Side-channel security.** The pipe adds no authority: bytes arriving on it are handled exactly like the same frames arriving on the PTY, as inert, labelled, budgeted unverified output, and a same-user process can already write to the PTY.
- **The decoder sandbox stays.** The worker already runs in an app container and a job object on Windows. Turning images on needs a Windows gate record, which is a maintainer decision this note lists.

## What the console does to a frame

Measured earlier on Windows Server 2022 with raw bytes 0x00 to 0xFF inside an escape string:

- The **system** pseudo-console drops the string.
- The pseudo-console **shipped with the terminal** (Microsoft's `conpty.dll` and `OpenConsole.exe`) carries the string, turns LF into CR LF and re-encodes every byte at or above 0x80 as UTF-8 from the OEM code page, with or without its pass-through mode.

Image frames are binary. The start frame (0x83) holds a `u32` id, a `u32` size pair and a `u32` length, all big endian, and the data frame (0x84) holds a `u32` id, a `u32` offset and up to 262,144 bytes of the encoded image. Those bytes include values at or above 0x80 and LF, so both console builds damage them. What was not measured, and the first measurement must, is how the shipped console treats strings made of printable 7-bit bytes only: their length limit, line wrapping, rate and behaviour on resize.

## What the images use

From the specification and the terminal today:

- An image is one start frame and then data frames. All are unsigned Track 0 frames that any program may write, and they count only inside the unverified-output container.
- A producer has at most one image being received, with 1 to 16,777,216 encoded bytes, and holds at most 32 MiB of encoded bytes at once. Images in sealed layers are not defined in v0.
- Decoding is out of process, in the worker, with the memory and CPU limits of the decoder contract. Images over SSH are refused when unsigned.
- Ordering: the PTY decides where an image is placed in the document, because the start frame arrives in the PTY stream. Nothing from outside the PTY can create a block.

## Options

| Option | What it is | Verdict |
|---|---|---|
| A. 7-bit envelope for everything | The start and data frames travel as base64url text inside an escape string, with no LF, CR or byte at or above 0x80. The terminal removes the envelope in front of the existing detector. | Works for every producer that can write to the console, including WSL. Costs one third more bytes, and depends on the length and rate the shipped console accepts, which is unmeasured. The Track 0 string cap is 4 KiB, so a data frame would shrink to about 3 KiB of image bytes and a 16 MiB image would be about 5,600 strings. |
| B. Named pipe for the bytes, envelope for the control | The start frame travels in the envelope. The data frames travel over a named pipe that the terminal serves, byte for byte the same payload. | **Proposed.** The bulk bytes avoid the console. The in-band part is small and bounded. |
| C. A file or a shared memory section handed over by name | The producer writes the image to a place and names it in the start frame. | Rejected. Paths and names can be swapped between the check and the use (junctions, symbolic links, a replaced file), temporary files outlive a crash and leave image bytes on disk, and a shared section needs its own naming, size and clean-up rules. A pipe has none of these. |
| D. Skip the pseudo-console for images | Not available: every console program runs behind it. | Rejected. |
| E. Do nothing | Windows stays locked. | The current state. |

**Why the envelope stays even with the pipe.** In the pipe design the in-band start frame is what places the image in the document and what the pipe data is checked against. It is binary, so it needs the envelope. Its size is tens of bytes, far under any string limit the console is likely to have.

## The proposed design

### Pieces

1. **The layer job.** When the terminal starts a shell it puts it in a job object that the terminal alone holds a handle to: breakaway not allowed, kill on close, and no process-count, user-interface or other limit that would stop a shell and its programs from working, because they start many processes and some have windows. Every process the shell starts is in the job and cannot leave it. A process that was not started by the shell cannot join it without a handle to the job. Job membership is the Windows answer to "which layer does this writer belong to".

   **The holder is not in the layer job.** The sealing note puts the key holder in a separate job with a one-process limit, no child processes and no user interface, which a shell could not run under. Each process is in one of the two jobs and never both, and the terminal holds both handles. The pipe described here checks membership of the layer job only. The holder's checks use process ids and handles and do not use job membership.
2. **A pipe per layer, served by the terminal.** `\\.\pipe\bright-image-<random>`, created before the shell is started with first-instance and remote-client refusal, in byte or message mode with a 64 KiB buffer. The terminal gives the name to the shell's tree in an environment variable, `BRIGHT_IMAGE_PIPE`. The name is not a secret and is not relied on as one.
3. **A producer side.** A program that wants to show an image reads the variable. If it is set and the pipe opens, it sends the data over the pipe. If not, it uses the envelope for everything, or shows no image. `emit-image` is the reference producer, and the helper library used for it is what other producers would use.

### Per image

1. The producer writes the **start frame in the envelope** on its console output, as today but with the envelope on Windows. The terminal places the image block at that point, with its alt text, and records the expected id and length for this layer's producer.
2. The producer opens the pipe, sends data messages, and closes it. Each message is the payload of a data frame: `id`, `offset`, then 1 to 262,144 bytes. It may open a connection per image and keep it until the image is complete. A write blocks while the terminal is not reading, which is the backpressure described below.
3. **For each connection the terminal checks:** the client's process id from `GetNamedPipeClientProcessId` is a process in this layer's job (`IsProcessInJob`), and the process can be opened with query rights at all. A process outside the job, another user's process or an elevated one is refused and the connection is closed.
4. **For each message the terminal applies the existing rules unchanged:** the data is accepted only for the incomplete image of this producer with that id, only when the offset equals the bytes received so far, and not past the declared length. Anything else ends the image with its alt text, as for the in-band data frame. The pipe message has no new fields.
5. When the bytes equal the length, the image is submitted to the decoder exactly as an in-band image is.

### Why the order and the budgets stay correct

- **The PTY stays the ordering authority, and the terminal does not read the pipe until the start frame has arrived.** The pipe is faster than the console, so a producer can have its data ready long before its start frame comes out of the console. The terminal accepts the connection and runs the connection checks, and then reads nothing. The pipe is created with a small buffer (64 KiB), so the producer's first writes fill it and block. That is the backpressure: no image bytes are held in the terminal's memory before the start frame, whatever the scheduling. When the start frame for that layer's producer arrives and is accepted, the terminal starts reading, in order, and applies the ordering rules to each message. A producer that wrote the start frame and then connected, and one that connected first, behave the same. This replaces any staging cap: there is no staged data to cap, so a legal image of 16 MiB cannot fail for outrunning its start frame.
- **Bounded waiting.** A connection with no start frame is left blocked and counts against the layer's connection cap and its idle timeout. The idle limit is set from the first measurement of the worst-case delay the console adds, with load and with other output in flight, and is not the specification's 5 seconds unless the measurement supports it. When it passes, the terminal closes the connection, the producer's write fails, and the producer shows no image (it prints its alt text, see "Failure behaviour"). A start frame with no data after it ends under the existing end-of-command and reset rules.
- **There is no acknowledgement.** The gate is the terminal not reading, so the pipe needs no reply and sends nothing back.
- **Budgets are the same ones.** Staged and received bytes count against the producer's 32 MiB held-bytes bound and the unsigned share of the session budget. A connection that would pass a bound is closed and the image degrades as in the budget section.
- **The decoder is unchanged.** The worker gets the same bytes through the same runner, in the same app container, and its reply is validated as before.

## Security impact of the side channel

The pipe is reachable by name by any process of the same user. What each reach gives:

| Concern | Effect | Control |
|---|---|---|
| A process outside the layer sends image data | It could try to put its own image in a block started by a program of the layer | Refused at connect: the client is not in the layer's job. A same-user process can already write to the PTY by duplicating a handle, so the parity rule is met even if the check were absent: the block would be the same labelled, inert, unverified one. |
| A process in the layer sends data for another layer | Not possible | One pipe name per layer, one job per layer; a process in layer A is not in layer B's job and does not know B's pipe. |
| Name squatting before the terminal creates the pipe | A hostile server takes the name | The terminal creates it first, with first-instance, before the shell exists. |
| Flooding: many connections, large data | Memory and time | Nothing is read before a start frame, so at most the 64 KiB pipe buffer per connection is held and one message in flight. At most four open connections per layer, one image being received per producer, an idle timeout on a connection, and every byte counted against the budgets. Excess connections are closed. |
| Confidentiality | The pipe carries image bytes into the terminal only | It sends nothing back. No terminal state or reply goes over it. |
| A child process using a handle inherited from the producer | It sends data as the producer | The child is in the same job, so it has the producer's authority already. |
| The decoder gets different bytes than on Linux | Parser surface | None. The same bytes reach the same worker. The pipe reader adds a small length-checked reader on the terminal side, and its source belongs to the image-path build identity. |
| Pipe messages that lie about size | Memory | Declared lengths are capped before any allocation, as the specification already requires for every length. |

**Parity.** The channel adds no way to create a block, no authority, and no secret that a same-user process could not already read from the PTY path. It adds a performance path and a smaller place for a bug. The terminal-side reader is covered by the same hostile-input tests as the in-band frame parser.

## Can it share the seal channel?

No, and the sealing note's design is why. The seal channel:

- has one expected sender, the shell, identified by process id and creation time;
- carries at most 128 bytes in a message with fixed fields and no free text;
- is served by the key holder, a process that never evaluates what it is sent and holds the layer's keys.

The image pipe:

- has many possible senders, all programs in the layer;
- carries up to 256 KiB in a message of unsigned bytes;
- is served by the terminal.

Putting unsigned bulk bytes in front of the key holder would weaken the property that the holder reads only small fixed messages from one process. What the two share is the pattern: a pipe created first with first-instance, refusal of remote clients, an identity check at connect from a process id, and the PTY as the ordering authority. They also share the per-layer job object and the first measurement. Sealed images (an image inside a sealed layer, which the `images` capability would grant) are not defined in v0. When they are, the holder relays frames over its own channel under the specification's Track B rules, which this note does not change.

## What else turning images on needs

1. **A Windows decoder gate record.** Images are unlocked only for a build identity with a record. Today `release/gate-records.txt` has no Windows line on purpose, and the release-gate test requires that. A Windows line needs: the sandbox escape suite passing on a clean Windows machine for the app-container worker (the suite exists and the sandbox is built); the same decoder identity (the decoders are pure Rust and the code is the same on every platform); and a decision by the maintainers on whether the carried-forward fuzz basis that covers Linux and macOS also covers Windows. This note does not make that decision and asks for it. The release-gate test and the release criteria change in the same step.
2. **Identity records.** The pipe reader, the envelope reader and the image drawing are image-path sources, so any change to them moves the image-path identity and needs the gate records carried forward in the same change, as in earlier changes.
3. **The decoder worker.** It already starts in an app container with no capability and a one-process job with memory and CPU limits. It needs no change. The terminal needs the pipe reader and the envelope reader.
4. **Throughput.** A 16 megapixel image is up to 16 MiB of encoded bytes. Over the pipe that is 64 messages of 256 KiB. In the envelope it is about 22 MiB of text in about 5,600 strings, through a console that re-renders. The first measurement gives both numbers, and the design sets a time limit per image from them.

## Failure behaviour

- **Any check fails (client not in the job, bad message, over a bound, timeout):** that image shows its alt text. Nothing else changes. No layer is pinned for an image fault; the existing degradation rules for a producer apply.
- **The pipe cannot be created:** the variable is not set. Producers use the envelope or show no image.
- **The shipped pseudo-console is missing, or the terminal is not using it:** the system pseudo-console drops the escape string, so the terminal receives neither the start frame nor its alt text and cannot show either. It shows no image block, and no frame of any kind arrives. The producer, not the terminal, is what shows the alt text: the terminal sets a variable (`BRIGHT_IMAGES`, with the value `pipe` or `envelope`) in a layer's environment only when the shipped console is in use and that path is available. A producer that finds the variable absent writes the alt text as one line of ordinary text outside any escape string and sends no frame. The sealing note's notice appears in the chrome.
- **A producer that writes raw binary frames on Windows (an old producer, or one that ignores the variable):** the console drops or damages the frame. What survives is ordinary output, and whatever the detector cannot read as a frame is handled as it is for any bytes today. No image block appears and no alt text is promised. The producer is expected to use the variable.
- **After a decoder crash:** the producer's images show alt text for the rest of the command, as on Linux.

## Implementation tickets, sized

S is a day or less, M is two to four days, L is a week or more. Items 1 and 2 are shared with the sealing note and are listed there as well.

1. **Carrier spike on a clean Windows machine (M, shared).** Adds to the sealing measurement: long printable strings (length at which the console splits, wraps or drops them), the rate for a 16 MiB payload as text, the same payload over a named pipe, the cost of the job-membership check, and the worst-case interleavings of the start frame and the pipe data (pipe data ready long before the console delivers the start frame, a loaded console, other output in flight, a start frame that never arrives), with a 16 MiB image in every case. Output: a measurements file and the time limit per image.
2. **Layer job, holder job and shell creation (M, shared).** Create the shell inside the layer job (breakaway off, kill on close, no limit on processes or user interface) and the holder inside its own job (one process, no children, no user interface). Tests that a child of the shell cannot leave the layer job, that an outside process is not in it, that the shell cannot start a window or a process under the holder's limits and the holder is not in the layer job.
3. **Envelope in the pipeline (M, shared).** Remove the envelope before the detector for every frame type; the existing failed-frame rules unchanged.
4. **Image pipe server (L).** Creation with first-instance and remote refusal and a small buffer, connection cap, the layer-job check, the read gate that waits for the start frame, the idle limit from the measurement, pairing with the start frame, budget accounting, close on violation. Hostile and interleaving tests: a client outside the job, a name squat, data ready before the start frame (with a 16 MiB image that must succeed), data with no start frame, a start frame that arrives late, a length lie, a flood, a slow sender.
5. **Producer side (M).** The `BRIGHT_IMAGES` and pipe variables, the pipe client, the envelope fallback and the plain-text alt fallback in the helper library, and `emit-image` using them. Tests: a real producer in the layer and one outside it; a producer with the variable absent printing its alt text with no escape string (the missing-shipped-console path); an old producer writing raw binary frames, with the terminal staying up and showing no image block.
6. **Windows decoder gate record (M, plus the maintainers' decision above).** Run the sandbox escape suite on a clean Windows machine at the build, add the Windows line, change the release-gate test and release criteria together.
7. **Windows image proof on a clean machine (L).** The terminal showing a PNG, a JPEG and a WebP from a producer in a PowerShell layer over the pipe, and the same over the envelope; the refusals above; a 16 megapixel image with its measured time and the data-before-start-frame case; the missing-shipped-console path and the old-producer path; the decoder crash case; screenshots; clean machine deleted afterwards.
8. **WSL producers (S to M, later).** Programs inside WSL cannot open a Windows pipe. They use the envelope. A small Windows-side helper that the WSL program can run through interop is the alternative if the envelope is too slow. Decide after item 1.

Every ticket keeps the rules the image work keeps: images on by default where supported, real behaviour tests, no new fuzzing without a decision, and no claim that images work on Windows until item 7 passes.

## What the specification would change

For the revision that follows sign-off:

- The platform page: the Windows section changes from "not supported" to the envelope, the pipe and the unlock basis.
- The image page: the platform unlock paragraph, and a statement that a pipe message is a data frame payload under the same ordering and budget rules.
- The transport page: the envelope, shared with sealing, and the Windows pipe as a local side channel for unsigned image data, with its job-membership rule.
- The conformance page: the Windows decoder gate once the record exists.

## Open questions for review

1. Is a Windows gate record on the existing carried-forward evidence acceptable, as for macOS, or does Windows need its own fuzz campaign?
2. Should the pipe be required, or should the envelope alone be allowed if the measurement shows it is fast enough? One path is simpler to maintain and review; two cover more producers.
3. What time limit per image is acceptable for the envelope fallback?
4. Do producers inside WSL matter for the first Windows release?
