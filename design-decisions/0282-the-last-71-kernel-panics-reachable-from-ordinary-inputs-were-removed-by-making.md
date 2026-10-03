## §282 — The last 71 kernel panics reachable from ordinary inputs were removed by making the panic *impossible*, not by catching it

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Scattered through the kernel were 71 places that said "if this
step fails, crash the whole machine" — `unwrap()` and `expect()`, Rust's way of
saying "I am certain this cannot fail." Some of them were wrong: you could
crash the kernel by typing a shell command with a name that did not exist. They
are now all gone. The interesting part is *how*: for most of them the right fix
was not to handle the failure but to restructure the code so the failure has
nowhere to occur, which leaves less code behind than catching it would.

### What was wrong

`CLAUDE.md` has said from the start that "every `unwrap`/`expect` in non-test
code is a potential DoS if an attacker can shape the input." That was policy
without enforcement. `scripts/scan-unwrap.py` (see §283 below on the classifier
itself) made the count observable for the first time: **71 sites in production
paths**, as against ~1900 in tests, where a panic on bad data is the point.

At least one was reachable from a keyboard. `notifprefs app <anything>` at the
kernel shell looked the app id up and unwrapped the result, so a typo — or a
name that was simply never registered — was a one-line kernel panic typed at a
prompt. Several more were reachable from an allocator that had run out of
memory, which is a state the system is otherwise designed to survive.

### The decision

Four shapes of fix, chosen by what the site actually was, in preference order —
each one preferred over the next because it leaves less code behind:

| Shape | When it applies | Example |
|---|---|---|
| **Delete the possibility** | The failing call's inputs are compile-time constants | `Layout::from_size_align(64, 8).expect(…)` → `Layout::new::<[u64; 8]>()` |
| **Delete the second copy of the fact** | The unwrap leans on two pieces of state agreeing | kchannel's `sent: Cell<bool>` restated "the cell is empty"; reading the cell once by value makes it structural |
| **Construct on first use** | The `Option` is an initialization-order artifact, not a state | the five namespace/container tables |
| **Return the error** | The failure is a genuine outcome | every kshell command; the bench suite |

Only the fourth adds error-handling code. It was used least.

### The alternative that was rejected

**Wrap each site in a check and log.** This is the mechanical fix, and it is
what "remove the unwraps" usually means. It was rejected because it treats 71
unrelated sites as one problem. Of the four shapes above, three make the failure
*unrepresentable*; a check would have preserved every one of those failures as a
live branch, plus added a log line that can never fire. The 25 sites fixed by
the first three shapes ended up **shorter** than they started.

### Two things this exposed that a mechanical fix would have hidden

1. **Converting a panic to an error exposes latent leaks.** Under `.expect()`
   the panic ended everything, so a handle leaked on the way out did not matter.
   A bare `?` in the same place leaks one handle *per iteration*. This bit twice
   in the benchmark suite (`bench_service_connect` leaking a channel,
   `bench_ipc_shm` leaking a 16 KiB mapping) and once in `bench_page_fault`,
   which maps 220 pages in its timed loop and had to learn to tear down exactly
   the prefix it had built.

2. **A guard that exists only to dodge a panic is a band-aid, and removing the
   panic retires the guard.** `netns::is_initialized()` had five callers. Four
   were dodging the panic — one said so in a comment: *"netns accessors panic
   (not `Err`) when the table is `None`; guard so a mis-ordered boot can't panic
   here."* The fifth was load-bearing for a different reason (`net::init()` runs
   first and pushes root interface config in), which is what forced `init()` to
   become idempotent rather than merely leaving lazy accessors in place. Four
   guards deleted; one real ordering constraint made explicit.

### The benchmark harness got a fallible sibling

`bench::run` cannot fail by construction. Eight benchmarks needed to, so
`try_run` was added and `run` became a wrapper over it. Two details are load-
bearing:

- **The `Result` is inspected *after* `let end = rdtsc();`.** The added branch
  therefore falls outside the measured window, so `run` and `try_run` time the
  same instructions. `bench_page_fault`'s hand-rolled loop does the same.
- **`run`'s unreachable `Err` arm returns an inert empty result, not a panic.**
  A benchmark harness must not be the thing that panics the kernel. Its `seq` is
  deliberately out of range, so if one ever *is* scored the mismatch is counted
  and printed rather than silently crediting an unrelated window.

`try_run`'s built-in warmup (`max(iterations / 10, 5)`) also retired four
hand-written warmup blocks that existed only to be unwrapped twice more. One of
them (pipe) used *blocking* `read` where the measured body used `try_read` — a
warmup that can block is a warmup that can hang the boot.

### `report()`, and why a skipped benchmark must say so

`run_all` is the top-level driver and has nothing to propagate to, so each
now-fallible benchmark is wrapped in `report(name, outcome)`, which logs
`SKIPPED (<err>)` and continues. Two rejected alternatives:

- **Propagate out of `run_all`.** A benchmark that could not allocate its
  fixtures would cancel the twenty benchmarks after it.
- **`let _ = bench_foo();`.** Worse than either. A suite that quietly stopped
  measuring something reads *exactly* like one that measured it and found
  nothing wrong.

The same reasoning suppresses the verdict on the interleaved A/B allocator
experiment when any round failed: a failed alloc returns much sooner than a
successful one, so it drags `min` down on whichever arm hit it. Reporting PASS
off such a window would be a silently wrong answer, which is worse than
reporting nothing.

### What this does not claim

Zero `unwrap` sites in production code is not zero panics. `panic!`,
`assert!`, indexing and unchecked arithmetic remain, and the defensive clippy
lints that would surface them still report ~18 000 warnings tree-wide. This
closes one enumerable category and leaves the scanner behind so it stays closed.
