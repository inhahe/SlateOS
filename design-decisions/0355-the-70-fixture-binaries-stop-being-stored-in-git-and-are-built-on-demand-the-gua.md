## §355 — The 70 fixture binaries stop being stored in git and are built on demand; the guard moves from "detect drift" to "refuse to ship a gap"

**Date:** 2026-08-21
**Decided by:** Claude (autonomous) — lane B. Lane A supplied the reproducibility
measurement the operator asked for and argued for option C; the measurements
below were taken afterwards and point elsewhere.
**Answers:** `open-questions.md` B-Q5 (now removed).

**In short:** This project keeps 70 compiled test programs inside git, next to
the source they were compiled from. The library they are compiled against is
*not* in git, so nothing was comparing the two, and the saved programs kept
quietly becoming tests of a system that no longer existed — five times in six
days. I had added a checker and recommended keeping the arrangement. Measuring
it today showed the checker covers 9 of the 70, and that **60 of the remaining
61 are stale right now** and have been for an unknown period with nothing in the
repository able to notice. So the arrangement goes: the binaries stop being
stored, and are rebuilt from their recipes when the boot image is assembled.

### What changed my mind was four measurements, not an argument

My own recommendation in B-Q5 was *"A for now, B once every lane has the
toolchain"* — keep storing them, move to building on demand once the blocking
premise was gone. Lane A's update argued for C (store them, plus commit a
checksum of the library they were built against) on the strength of proving the
library builds byte-reproducibly. Both positions were reasoned from the entry's
options table. The table turns out to be wrong about the size and the shape of
the problem.

| # | Measurement | Result |
|---|---|---|
| 1 | How many of the 70 are actually guarded? `git ls-files "*.stamp"` | **9.** The nine `services/ctest-*` fixtures. The 61 `services/fastpy-*` ELFs carry no stamp, no checksum and no gate of any kind. |
| 2 | Is the fastpy build deterministic, so that a mismatch means staleness rather than noise? | **Yes.** Two consecutive builds of `fastpy-hello` from unchanged source: `c57c8719…` both times. |
| 3 | Then how many of the unguarded 61 disagree with what their own `build.py` produces today? | **60 of 61** (the 61st being the one I had already rebuilt by hand while measuring #2). Every unguarded fixture in the tree is stale. |
| 4 | What does rebuilding all of them cost? | **55 seconds** for all 61, plus 10 s for the nine C ones. Zero build failures. |
| 5 | What does B cost a clone with no toolchain? | Less than the status quo already costs it — see below. |

Measurement 3 is the one that decides it. The premise under option A is that the
drift is an occasional accident that a gate can catch. It is not occasional: it
is the steady state, it is the *whole* unguarded population, and the reason the
five recorded incidents all involve the nine stamped fixtures is not that the
nine are the problem — it is that the nine are the only ones anybody can see.
The ring-3 fastpy self-tests have been proving that binaries run on-target
against a `libc.a` that has not existed for some time, and reporting PASS.

### Why option C cannot work here, despite being verified

C's mechanism is: record the identity of the build input in git, so the gate has
a second reference point and can tell which side moved. That is a genuinely good
idea, and lane A's reproducibility result establishes the property it needs.

It cannot reach the 61, and not for want of effort. Those fixtures are compiled
by **fastpy, which is a different repository** — `services/fastpy-*/build.py`
imports `compiler` from `D:\visual studio projects\fastpy` through `PYTHONPATH`,
a path that appears in this tree only as prose in a docstring. Nothing here
records which revision of that compiler produced a committed ELF, and nothing
here can: it is not a build output of this tree, it is a peer. Recording its
version string would not help either — fastpy's `pyproject.toml` has read
`0.1.0` across every commit in its history, so the version cannot distinguish
two builds.

So C guards `libc.a` — one of the two inputs — for all 70, and leaves the other
input unguardable *in principle* for the 61 where it applies. That is not a
smaller version of the fix; it is a gate that reports "ok" on a stale artifact,
which is precisely the failure the entry is about ("being diligent makes it
quieter"). A guard that cannot see one of its inputs is worse than no guard,
because people believe it.

### Why B's headline cost is not real

The table costs B at *"every lane needs the full toolchain to run a boot test"*.
That is already true, and more strictly than B would make it:

- `kernel/src/proc/spawn.rs:2952` does
  `include_bytes!("../../../services/netstack/target/…/netstack")`. That path is
  gitignored (`.gitignore:3`, `**/target/`) and untracked, and the embed is
  unconditional. **The kernel crate does not compile from a fresh clone until
  netstack has been built.** The tree already requires a toolchain to get as far
  as a kernel binary, let alone a boot test.
- `libc.a` is the same: gitignored, built from `posix/src`, required.
- By contrast the fixture ELFs are read at *runtime*, from `/mnt/tests/`, by
  `load_test_elf()` — which returns `None` when a fixture is absent so the
  self-test **self-skips**. A missing fixture is the mildest of the three
  failures the tree already contains.

And the blocking premise I had named is satisfied: `zig` 0.16.0 lives at
`D:\utils\zig-x86_64-windows-0.16.0` and is found machine-wide by
`_find_zig_cc()` (fastpy `compiler/toolchain.py:123`), so it resolves identically
for lanes A, B and C; `scripts/ctest-fixtures.py` self-locates fastpy (verified
by running it under `env -u PYTHONPATH`); and the nine C fixtures rebuild in
**10 seconds**.

Rebuild cost, measured: **55 seconds for all 61**, plus 10 s for the nine C
fixtures — call it a minute and a bit for the entire set, with zero build
failures. (Timing a *single* fixture gives 8.3 s and would imply 8.5 minutes;
that is wrong, because almost all of it is one-time LLVM start-up cost. The
amortised figure is ~0.9 s per fixture. I nearly shipped the 8.5-minute number
in this entry, which is a reminder that a per-item cost measured once is not a
per-item cost.) Against a boot test that already runs about eight minutes, a
full cold rebuild of every fixture in the tree is noise — and it is paid by the
machine, only when an input actually changed, and by *the lane that changed the
input*, which is the injury lane A identified and which no amount of
gate-improvement under A or C addresses.

### The part of B that is not in the table, and that this decision adds

`load_test_elf` self-skipping is what makes B cheap, and it is also B's one real
danger. Under A, a broken toolchain gives you 70 stale tests — each of which at
least tests the *previous* version of the system. Under naive B it gives you
**zero tests and a green boot test**, silently. That is a worse outcome than the
one being fixed.

So B ships with the guard inverted. The old guard's job was *detect drift between
a committed artifact and its inputs*; with no committed artifact that job no
longer exists. The new job is *refuse to produce a boot image with fewer fixtures
than the tree defines*:

- `scripts/create-ext4-rootfs.sh` enumerates `services/{ctest,fastpy}-*/build.py`
  — the recipes are tracked, so the expected set is a fact of the checkout, not a
  hand-maintained list that can drift on its own — builds any whose ELF is
  missing or older than its inputs, and **fails the image build** if any recipe
  fails or any expected ELF is absent at staging time.
- `scripts/ctest-fixtures.py` keeps its `build` half and loses `check`/`stamp`,
  which become meaningless. Its `libc.a`-versus-`posix/src` freshness test is
  kept and promoted from a warning into an action: build the sysroot when it is
  behind, rather than advising the reader to.

That last point retires lane A's wrong-advice bug without needing C. The gate
printed the wrong remedy because it held one hash and could not tell which side
had moved; a build step does not have to tell, because it rebuilds whatever is
behind in dependency order.

### What is given up

- **Binaries in history stay.** Dropping the files from the working tree does not
  shrink what is already committed, and rewriting that history would need a
  force-push, which is forbidden outright here. B stops the growth (~2 MB a
  rebuild); it does not undo it.
- **Bisect changes character.** The entry counted "bisect against a known-good
  binary" as a loss. Honestly stated it is a swap: today you can bisect a binary
  built against a `libc.a` nobody still has — which is the incident class itself —
  and afterwards you bisect a tracked recipe rebuilt against the libc of the
  commit you landed on. Since `build.py` is tracked and both builds are
  deterministic (measurement 2, and lane A's for `libc.a`), the second is the
  more trustworthy of the two.
- **A clone with no `zig` gets fewer self-tests.** It gets a loud message and a
  failed image build rather than a quiet pass, which is the intended trade.

Lane A's reproducibility measurement was not wasted by this going to B rather
than C: rebuild-on-demand is only safe *because* the builds are deterministic. If
they were not, every boot test would produce different binaries and no one could
separate a real change from build noise. C's evidence is B's foundation.

**Related:** `known-issues.md` → the ctest fixture drift entries;
`requests/a-b-nine-ctest-fixtures-on-main-…`,
`requests/a-c-fixture-rebuild-was-correct-on-lane-c-and-wrong-on-main.md`;
design-decisions.md #86 (TD-KERNEL-EMBED-BLOAT — why these ELFs moved out of the
kernel image and onto the rootfs in the first place, which is what makes them
skippable and therefore what makes this decision cheap).

**As implemented — the gate was itself an instance of this.** Landing the decision
turned up one thing the analysis had not: the *existing* mtime staleness check
in `create-ext4-rootfs.sh` lived inside the ctest staging loop and so covered
the 9 C fixtures, while the 61 fastpy ELFs were staged by a different loop, a
few hundred lines away, that checked nothing at all.

So the coverage number that condemned the content stamps — 9 of 70 — was not the
stamps' number. It was the number for *every* guard this tree had over these
files, arrived at twice independently, because both guards were written inside
the loop that handled the family that had visibly failed and neither was ever
carried to the family beside it. That is the more general lesson and it is worth
stating separately from the storage decision: **a per-family check written inside
a per-family loop produces a per-family blind spot, and nothing about it looks
wrong when you read it.** Both guards were correct code. Both were tested in both
directions. Neither had any way to notice that eight times as many fixtures sat
outside them.

The implementation therefore has one loop, over the tracked `build.py` recipes,
answering both questions for both families: is the ELF there, and is it behind
its inputs. The set it iterates is a fact of the checkout rather than a list
anybody maintains, which is the property that makes a 71st fixture — or a third
family — covered on the day it lands rather than on the day it first fails.

Two smaller findings from the same pass:

- **`_inputs()` did not glob `*.h` and the shell gate did.** No fixture has a
  header today, so nothing was wrong; but the *builder* was the less inclusive
  of the two, which is the wrong way round. The first fixture to grow a header
  would have been reported stale on every image build and not fixable by the
  command the report names. Where a gate and a builder disagree about what an
  input is, the builder must be the more inclusive.
- **The four spike ELFs are what is left of the old pattern.** bash, GNU make,
  pkgconf and CPython still print a rebuild command instead of running it, and
  that cost a full cycle during this very verification. Measured: 83 s to
  rebuild all four, against an image build of minutes — so the cost argument for
  leaving them manual does not survive being measured either. Logged as
  `TD-B-THE-FOUR-SPIKE-ELFS-STILL-MAKE-YOU-RUN-THE-REBUILD-BY-HAND`; not folded
  into this change because it fails *closed* (no stale spike can reach an image),
  which makes it debt rather than a hazard.

**Revisit if:** fastpy is ever vendored into this tree or pinned to a recorded
revision, which would make its identity expressible in git and reopen C as a way
to keep the binaries; or if the cold rebuild grows past the boot test itself, at
which point the fixture set — not the storage policy — is what wants trimming.
