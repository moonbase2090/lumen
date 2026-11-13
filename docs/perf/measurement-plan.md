# Terminal performance: measurement plan

Status: plan. No numbers here yet; every number later added to this folder
comes from a run of the commands in the plan, with the raw output kept beside it.

The question: how does the Lumen terminal compare with Foot, a very fast
Wayland terminal, on the work a terminal does all day? Both draw on the CPU
(Foot with pixman, this terminal with a software buffer), so the comparison is
fair without a GPU.

## Target

On the grid and VT workloads, the median is within about 10% of Foot's median
on the same machine, for every metric below. Where a metric is a latency or a
cost, "within" means no more than 10% worse. If a workload cannot meet that bar
without weakening rendering quality or a security gate, the gap is recorded
with its cause and the bar is not met; speed is never bought with either.

Rich blocks (proportional paragraphs, images) have no Foot equivalent. They are
reported separately, and the rule is one-sided: adding rich blocks to a
session must not slow the grid path. The check is the grid workloads measured
with and without a rich block on screen.

## Machine and settings

- One throwaway Linux VM, x86-64, fixed instance type, dedicated for the run
  (no other load), launched fresh, terminated when the run ends.
- Wayland: a headless compositor with a fixed output size and scale 1. Both
  terminals run in the same compositor, one at a time, never side by side.
- Same font, size, cell grid (for example 120x40), window size, colours and
  scrollback for both. Foot's config is a short file kept in the repo; the
  terminal's equivalent options are the command line it already has.
- CPU governor pinned and recorded; the terminal and its child shell pinned to
  the same cores for both; a warm-up run discarded before the counted runs.
- Recorded with every run: instance type, CPU model, kernel, compositor and
  Foot versions, this repo's commit, rustc version, build profile and flags.
- Foot is the current release at the time of the run, installed from the
  distribution package or built from the tagged source; the exact version and
  source are recorded.

## Workloads and metrics

Each is run at least 20 times per terminal; the report gives median and p95
(and min/max in the raw data). Runs alternate between the terminals so drift in
the machine hits both equally.

1. Throughput. A vtebench-style set: scrolling a large file with `cat`, dense
   truecolour cells, wide Unicode and combining marks, cursor motion, and
   alternate-screen redraws. The metric is the wall time the writing program
   spends in its blocking writes to the PTY for a fixed input of known size,
   giving MB/s. The terminal's read speed throttles the writer, and what is
   still unread at the end is at most one PTY buffer, small against a payload
   of hundreds of megabytes. No cursor-position round trip is used to wait for
   the terminal: the terminal answers no status query from a program, by design,
   and the measurement does not ask it to.
2. Input latency. Key to screen with no camera. A test client injects a key
   through the compositor's virtual keyboard at a known time; a small program
   in the terminal echoes it; the compositor-side capture records when the
   first frame containing the change is committed. The metric is the
   difference, measured with one monotonic clock on one machine. The method,
   its resolution and its own error are written out next to the result. If
   Typometer can be run unchanged under the headless compositor it is run too,
   as a cross-check.
3. Startup to first frame. Process start to the first committed frame, taken
   from the compositor's side for both terminals, so it uses the same clock
   and the same definition of "first frame".
4. Memory. Idle resident set after startup, and peak resident set during the
   throughput workloads, read from the kernel for the whole process tree.
5. CPU. CPU time per second while idle for 60 s, while a steady scroll runs,
   and per megabyte of output during the throughput workloads. Idle wakeups
   per second are recorded as well, because they are what idle CPU is made of.
6. Frame pacing. During a scripted scroll and a scripted resize, the intervals
   between committed frames from the compositor's side: median, p95, p99, and
   the count of frames later than twice the refresh interval. Output is
   produced faster than the display, so the terminal chooses what to skip.
7. Rich blocks, reported separately. A document of proportional paragraphs
   and a document with images, measuring the same startup, memory, CPU and
   frame-pacing metrics, plus time to lay out and paint a fixed document. The
   with/without comparison against the grid workloads is the "must not slow the
   grid" check.

## Regression check

Wall-clock numbers from a shared CI runner are noisy, so CI does not assert
them. CI asserts quantities that do not depend on timing:

- bytes allocated and number of allocations while parsing and painting a fixed
  input, counted with a counting allocator in a test;
- number of full-frame repaints, pixel writes and buffer copies for a fixed
  scripted session;
- work-per-byte bounds checked as ratios (for example, painting a second
  identical frame costs no more than a small multiple of the first).

Each bound is set from a measured value with wide headroom and fails only on a
change of kind (an algorithmic regression), not on a few percent of drift. The
wall-clock harness runs on a scheduled VM and reports; a drop past the bar
opens an issue instead of failing a build.

## Findings

Each profiling finding is filed as its own change with the evidence that found
it: the profile, the workload, the number before and the number after. A fix
is accepted only with the rendering output unchanged (the existing pixel
oracles pass) and with every security gate still in force.

## Open items

- Choice of headless compositor and virtual-keyboard client for the latency
  measurement, decided after a first trial on the VM.
- The exact Foot version, recorded when the first run is made.
