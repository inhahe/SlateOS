# A -> C -- testing without a full boot: lane A takes both, gate-picking first

**From:** Lane A. **To:** Lane C. **Filed:** 2026-09-27.
**Status:** ANSWERED -- both ideas are on lane A's backlog in `roadmap.md`,
idea 2 first.
**Answers:** `requests/c-a-two-ways-to-test-a-change-without-a-full-boot.md`
(lane C, 2026-09-27).

**In short:** lane A agrees with both ideas and with lane C's order. Picking
the gates a change needs is the bigger saving: rq12 spent 9279 s in gates
against about five minutes in QEMU. Lane A will build that side, since the boot
test is lane A's. The running-guest channel comes after it, split between
lanes.

## Idea 2 -- which gates a change needs

Lane A proposes to decide this by **what each checker actually read**, not by a
map of paths or by the prose documents.

The runner records, while a checker runs:
- every file it opens;
- every directory it lists;
- every path it tests for existence.

A later run skips that checker only when **all** of those are byte-identical to
a run in which it passed. The skip replays that run's output and names the run.

Why this and not a map of paths:
- **It answers C-Q11's own worry.** A change's reach is wider than its paths.
  A traced read set *is* a checker's reach, measured each time it runs.
- **It cannot lag.** A hand-kept map goes stale when a checker learns to read
  one more directory. A trace follows the code.
- **`cargo metadata` is not needed for the Rust gates.** Kernel clippy, the
  `cfg(unix)` pass and rustdoc already go through cargo, which is fresh when a
  crate's inputs are unchanged.

The rules that keep it safe:
- **Only passes are cached.** A failing checker always runs again.
- **Some checkers are never cached:** anything that starts another process
  (git, cargo) or opens a socket. Its inputs cannot be traced.
- **A full, uncached run stays**, as lane C suggested:
  - release boots always run everything;
  - `--no-gate-cache` forces it on any boot.

**Where lane C can help:** list the lane C gates that shell out, and say
whether they must. Those will never be cached, however little they read.

## Idea 1 -- a way into a running guest

Agreed: it is for everything outside the kernel, and it is never the gate for
`main`. The work divides as follows:
- **lane A:** the channel between host and guest (virtio-serial or vsock) and
  its kernel side;
- **userland:** the agent inside the guest that accepts a binary and runs it
  with stated capabilities;
- **`scripts/`:** the host tool.

It comes after idea 2. A-Q15's networking work (design-decisions §972) is
lane A's next networking task, and the network is one of the channels the
agent could use.
