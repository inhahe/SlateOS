## B-THE-TRACKED-FIXTURE-BINARIES-DRIFT-FROM-THEIR-SOURCES, AND THE STALENESS GATE CANNOT SEE IT

**Status: CLOSED 2026-08-21 by removing the arrangement** (`53571a004`, design-decisions.md §355).
The binaries are no longer stored in git at all, so there is no second copy left
to drift. Superseded — see "How this was actually closed" at the very end.

**Status: FIXED 2026-08-16** (binaries re-committed in `169d3a242`; the
content-based gate — the part that actually matters — landed as
`scripts/ctest-fixtures.py`, wired into `scripts/create-ext4-rootfs.sh`, and is
verified in both directions. See "Fix as landed" at the end of this entry.)

**In short:** nine test programs are checked into git twice — once as C source
and once as the compiled binary built from it. Nothing verifies that the second
was built from the first. Today the committed source of one of them contained
33 checks that its committed binary did not, and every boot test passed anyway,
because each working copy happened to hold a locally-rebuilt binary that had
never been committed. We already have a check meant to catch stale fixtures,
but it compares file timestamps — and a fresh `git clone` gives every file the
same timestamp, so the check is structurally incapable of noticing.

**The concrete instance.** Commit `6c89903d0` added 236 lines to
`services/ctest-jobctl/main.c` (the 33 new `waitid` checks) and did not touch
the tracked `services/ctest-jobctl/ctest-jobctl.elf`. Sizes at that point:

| | tracked in git | correct rebuild | delta |
|---|---|---|---|
| `ctest-jobctl.elf` | 2,603,328 | 2,627,416 | +24,088 |
| the other eight `ctest-*.elf` | — | — | +~19,400 each |

The ~19,400 bytes common to all nine is the same `libc.a` growth that
`B-BASH-SLATEOS-ELF-WAS-EXEMPT-FROM-THE-STALENESS-GATE` found in bash earlier
the same day; the extra ~4,700 on `ctest-jobctl` is the new checks. So *every*
tracked fixture was linking an old libc, and one was additionally missing its
own source's content.

**Why the existing gate does not catch it — and cannot.**
`scripts/create-ext4-rootfs.sh` compares each `.elf`'s **mtime** against
`toolchain/sysroot/lib/libc.a`'s. Two independent reasons that misses this:

1. **The gate reads the working tree, not the index.** Rebuilding the fixture
   locally satisfies it permanently. Nobody has to commit the rebuild, and if
   nobody does, git stays exactly as stale as it was while every local gate
   reports green. That is not a hypothetical: `06d6d1f69` ("relink every ctest
   fixture against the current sysroot libc.a") and `94d036ee2` ("rebuild stale
   ctest fixtures and make staleness fatal") are *the same relink*, committed
   before and then allowed to drift again.
2. **On a fresh checkout, mtime carries no information.** `git clone` /
   `git checkout` stamps every file with the checkout time, so `main.c`,
   `libc.a` and the `.elf` are all the same age and no ordering exists to
   compare. A clean clone of `main` would have boot-tested the 33 new checks
   against a binary without them and reported OK — the exact false-green the
   gate was written to prevent, in the one environment where the gate is silent.

Note the gate is not *wrong*; it answers "was this rebuilt after the library
changed?", which is the right question for a build directory. It is being asked
to answer a different question — "was this binary built from this source?" —
which no timestamp can answer.

**Proper fix: make the invariant a content invariant.** Have each fixture's
`build.py` write a small tracked stamp file next to the ELF recording the
SHA-256 of every input that determines the binary — `main.c`, the
`toolchain/sysroot/lib/libc.a` it linked, and the compiler/link flags — and
have `create-ext4-rootfs.sh` recompute those hashes and compare, instead of
comparing mtimes. Properties this buys that mtime cannot:

- It survives a fresh checkout, because content hashes do not depend on file
  timestamps.
- It fails in **CI and on the operator's machine identically**, rather than only
  where the build outputs happen to live.
- It distinguishes the two failure modes we have now conflated: "binary predates
  the library" and "binary was not built from this source" are different
  hashes, and the message can say which.
- The stamp is diffable, so a commit that changes `main.c` without the ELF shows
  an unchanged stamp next to a changed source in review.

Keep the mtime check as a fast local pre-filter if desired, but it must not be
the authority.

**Second, smaller defect found alongside, already fixed.**
`services/ctest-ctty/main.o` was tracked, even though the other eight fixtures
each carried a `.gitignore` saying "only the linked ELF fixture is tracked" —
ctest-ctty was simply the one directory that never got a copy of that file.
Replaced the nine per-directory copies with one pattern rule in
`services/.gitignore` (`ctest-*/main.o`), verified with `git check-ignore`
across all nine. A rule replicated per directory is a rule a new directory opts
out of by not having it.

**Standing lesson.** Checking a build product into version control creates an
invariant — *this artifact was built from that source* — and version control
does not enforce it. If we are going to track binaries (and we should here: the
boot test must run on a machine with no zig/WSL toolchain), then the invariant
needs an explicit, content-based check, because the natural one people reach for
is timestamps, and timestamps are precisely the thing a checkout destroys. This
is the third artifact family in one day to be stale for a slightly different
reason; the common thread is that each gate was verifying something adjacent to
the property actually wanted.

**Fix as landed (2026-08-16).** `scripts/ctest-fixtures.py`, with three
subcommands (`check`, `build`, `stamp`), called from `create-ext4-rootfs.sh`
immediately after the existing mtime gate. It hashes `build.py`, `main.c` and
the linked `libc.a` into a tracked `<fixture>.stamp`, plus the ELF itself, and
on mismatch names *which* input moved — the diagnosis differs by input
(`main.c` = a source edit committed without its rebuild; `libc.a` = needs a
relink; the ELF alone = the binary was replaced behind the build's back).

Three design points worth keeping if this is ever rewritten:

- **`build.py` is hashed as a stand-in for the compile and link flags.** They
  live nowhere else, so this means a change to `-O2`, the code model or the
  entry symbol invalidates the fixture exactly as a source edit does, with no
  second list of flags to keep in sync with the first.
- **The fixture list is a glob over `services/ctest-*/`, not nine names,** so a
  tenth fixture is covered the day it lands. This is deliberate: the sibling
  defect in this same entry (`ctest-ctty`'s missing `.gitignore`) happened
  precisely because a rule was replicated per directory and one directory never
  got a copy.
- **A missing stamp is a failure, not a skip.** An unstamped fixture is one we
  can make no statement about, and "could not verify" must never render as
  "fine" — that is `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` all over again.

**Verified in both directions, and the negative direction found a real bug in
the fix itself.** Positive: all nine stamp and check clean, in Windows Python
and again under WSL inside a full image build. Negative: editing
`ctest-jobctl/main.c` and running the real `create-ext4-rootfs.sh` gave
`ROOTFS_EXIT=1`, named `input main.c` specifically, produced **no** `DONE` line
(so no image was written), and still checked the remaining eight rather than
stopping at the first failure.

The bug the negative pass caught: the gate was probing for `python`, and the
rootfs script runs under **WSL Ubuntu, which ships `/usr/bin/python3` and no
`python` at all** — so on its first real run the check silently skipped itself.
The only thing that revealed it was the deliberately loud
"skipped the content-stamp check" warning in the else-branch. That is the entire
argument for writing that branch loudly rather than letting an absent
interpreter fall through quietly, and it is the fourth instance today of "the
check did not run" wearing the costume of "the check passed". Now probes
`python3` before `python`, and the remediation hint it prints uses
`sys.executable`, so a command printed inside WSL is a command that works
inside WSL.

### How this was actually closed — 2026-08-21, `53571a004` (design-decisions.md §355)

The 2026-08-16 fix above was real, and every claim it makes about the nine
stamped fixtures is still true. It was also **partial in a way nobody measured
at the time**, and the measurement is the whole story:

| | fixtures | guarded by the stamp gate |
|---|---|---|
| `services/ctest-*` | 9 | 9 |
| `services/fastpy-*` | 61 | **0** |
| | **70** | **9** |

When that coverage was finally measured on 2026-08-21, **60 of the 61
unguarded fixtures were stale in the working tree at that moment** — the ring-3
self-tests had been proving that binaries ran against a `libc.a` that no longer
existed, and reporting PASS, for as long as the fastpy family had existed.

The gate was built for the population that had already failed. Every fixture it
covered was a fixture whose failure is what caused it to be written; the 61 that
had never visibly failed were never added, and nothing about the gate's design
would have made anyone notice. **The gate was measuring the population it was
made from.** That is the part worth carrying forward: "verified in both
directions" (which it was) says nothing about *how many things* were verified,
and a coverage number is a different measurement from a correctness one.

So the fix is no longer to widen the gate to 70. The second copy is gone: the
ELFs are gitignored and built on demand from the tracked `build.py` beside each
one, the nine `.stamp` files are deleted along with the `compute`/`stamp`/`check`
machinery that maintained them, and `create-ext4-rootfs.sh` no longer asks "was
this built from that?" — it asks "is any of the 70 missing?" and refuses to pack
a short image, naming what is absent. A compiled artifact that is not stored
cannot drift from its source, and the "which side moved?" ambiguity that made
the old gate's remediation advice a coin flip does not arise for a build step,
which simply rebuilds whatever is behind.

The two supporting sub-bugs are closed by the same commit, without needing to be
fixed on their own terms:

- the gate's *wrong-advice* failure (it told you to relink when the correct
  action was to rebuild the sysroot, or the reverse) — `build` now rebuilds the
  sysroot itself rather than printing which rebuild to run;
- mtime being uninformative on a fresh checkout — no longer relevant, because
  git no longer writes these files at all. On a fresh clone the ELF is simply
  *absent*, which is both the truth and the signal, and absence is exactly what
  the new gate is built to catch.
