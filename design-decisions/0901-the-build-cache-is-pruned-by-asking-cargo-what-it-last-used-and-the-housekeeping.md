## §901 — the build cache is pruned by asking cargo what it last used, and the housekeeping runs where it cannot delay or fail anything

**Date:** 2026-09-02. **Decided by:** Operator (operator asked for a surgical
prune rather than a blunt one; the mechanism, the thresholds and the placement
below are Claude's). **Lane:** A.

**In short:** Rust build output (`target/`) had grown to over 100 GB in two of
the four checkouts of this repo and just under 50 in a third, filling the
drive, even though the standing instructions have told every lane to clean up
after itself since August. The obvious remedy — delete the whole cache — makes
the next build take twenty minutes instead of thirty seconds, so nobody does
it until the disk is already full. This decision is about a way to delete only
the *dead* part, which costs nothing, plus where to run it so that it happens
on its own.

**The growth is structural, not negligence.** Cargo does not garbage-collect.
Every time a unit's inputs change it writes a *new* file named with a fresh
`-<hash>`, and it keeps the old one forever. So no individual build ever looks
wasteful, and the accumulation only becomes visible weeks later as a volume
that is mysteriously full. No amount of diligence catches that by hand, which
is why "the lanes were told to tidy up" did not work and was never going to.

### What counts as dead: `invoked.timestamp`, not mtime and not atime

Each unit has a `target/<profile>/.fingerprint/<name>-<hash>/` directory
holding a file named `invoked.timestamp`. Cargo touches it whenever a build
*involves* that unit — including builds where it found the unit already fresh
and compiled nothing. So "not invoked in fourteen days" means exactly "no
build in a fortnight needed this unit", which is the question being asked.

Two more obvious discriminators were tried and rejected as unsound:

| Discriminator | Why it is wrong |
|---|---|
| **mtime** (what `cargo-sweep` uses) | Dates the last *compile*. A stable dependency compiled once in July and linked by every build since looks two months idle. Deleting it is a rebuild of something in constant use. |
| **atime** | Would be right in principle, and this volume does have it enabled. But a *current* artifact that cargo only `stat`s — the overwhelmingly common case for a fresh unit — shows `atime == mtime`, so it degrades to the row above without saying so. |

A tempting third signal, "this crate has several hashes, so the extra ones are
garbage", is also wrong: `a2ps-cli-154eb5188e622895/` and
`a2ps-cli-fc15d6b797050592/` are the binary and the test binary. Two units of
one package, both current.

`incremental/` is a separate lever with a different failure mode, so it gets
its own (shorter, 7-day) threshold. It is never needed in order to *link* —
it is rustc's per-crate memoisation, read only when that crate is actually
recompiled — so a cold entry has no way at all to earn its space.

### Deleting in the order that survives an interruption

Fingerprint first, artifacts second. Of the two half-done states, only one is
unrecoverable: **fingerprint present, artifact gone** makes cargo read the
fingerprint, conclude the unit is fresh, and fail at link time. The reverse —
artifact present, fingerprint gone — is just a rebuild. Doing it in this order
means a run killed at any instant leaves the recoverable state.

"In use" is established as a fact rather than guessed from a timestamp: each
candidate is *renamed* first, Windows refuses to rename a directory holding
any open file, and the rename is atomic, so success proves nothing held it at
that instant. Renaming into one staging directory also turns twenty thousand
recursive deletes into a single `rd /s /q`, which on this volume is the
difference between a minute and hours.

### Where it runs, and why not somewhere more obvious

Two placements, answering two different problems:

**`reclaim-space.py` step 1.5** — before the existing step that deletes whole
`target/` trees. That step is safe but not cheap: it charges some lane a full
cold rebuild, picked by which directory sorted first. Pruning dead units
first frequently meets the target with nobody paying anything, and when it
does not, every GB it freed is a GB the destructive step no longer has to
take from a live tree.

**`boot-test.sh`, after a green run, when free space is under 100 GiB** — this
is the part that makes it happen without anyone deciding to. Three placements
were considered:

| Where | Why not |
|---|---|
| Head of every run | The scan is minutes of metadata I/O. It would sit in front of every build's feedback, every time, and would promptly be disabled in every invocation. |
| At the 20 GiB floor | Too late to be the gentle option — that is where the destructive remedy already lives. |
| **After a green finish, below a watermark** | The run is over, nobody is waiting on the next line, the tree is in a known state, and 100 GiB (≈ under two full builds of headroom, at ~40 GiB each across four worktrees) is late enough to be rare and early enough that the cheap remedy still has room to work. |

**On by default, unlike `--reclaim-space`,** and the asymmetry is the point:
`--reclaim-space` can only help by deleting *another* tree's output, so a run
should not do that merely because it happened to notice. This prunes only the
tree it just built, and cannot cost anybody a rebuild, because a unit that no
lane's builds have asked for in fourteen days is not one cargo is about to
want.

**It cannot author a verdict.** Its exit status is ignored and the `PASSED`
banner is printed regardless. A boot test reporting FAILED because a disk
cleanup hit a locked file would be read as the *kernel* having failed, which
is a far worse outcome than the space not being reclaimed.

### The report prints an age histogram, and that is not decoration

A pruner that reports "nothing to prune" is indistinguishable, from outside,
from one whose staleness test silently returns nothing. That is not a
hypothetical failure mode — earlier the same night, a mangled regex in a
measurement script reported `incremental 0.0 GB` for a directory holding
31.6 GB, and the wrong number was believed and published. So every profile
prints its age distribution (`<1d:14 <3d:15 …`) alongside the verdict: a
broken discriminator produces an empty or degenerate histogram, and a genuinely
hot cache produces a full one clustered at the near end. The claim is
falsifiable from the log rather than merely asserted.

The first real scan is a case in point: lane A came back **0 stale units and
0 of 12,301 cold incremental caches** across all five profiles, with every age
under three days. That inverted the expectation this work started from — lane
A was assumed to be the offender at 48 GB — and the histogram is what makes
"0 stale" readable as *the cache is entirely hot* rather than as *the scan
found nothing*.

### What reversing this looks like

The discriminator is one field: raise or lower `--age-days`, or pass
`--no-incremental` to leave rustc's memoisation alone. Disabling the automatic
run is `--no-prune-cache` or `BOOT_TEST_PRUNE_CACHE_BELOW_GB=0`, per run or
per environment. Removing the idea entirely is deleting `prune-build-cache.py`,
its suite, the step 1.5 block in `reclaim-space.py` and the hook in
`boot-test.sh` — after which the volume goes back to filling silently, which
is the state this replaced.
