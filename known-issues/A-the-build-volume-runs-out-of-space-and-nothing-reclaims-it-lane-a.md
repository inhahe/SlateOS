## The build volume runs out of space, and nothing reclaims it (lane A)

**Status:** OPEN — 2026-08-18

**In short:** all three lanes build on `D:`, which is a 1.9 TB volume that sits
at 100% used. On 2026-08-18 it fell to 16 GiB free with three `cargo test
--workspace` runs live, which is below the floor `scripts/boot-test.sh`
requires, so lane A could not boot-test at all. Nothing in the tree reclaims
space; every lane's `target/` grows monotonically and the integration checkout's
is the largest single consumer.

### What actually happens

`scripts/boot-test.sh` refuses to run below 20 GiB free (`--min-free-gb` to
override). That guard exists because on 2026-08-15 the volume hit zero bytes
free and a half-written edit truncated a kernel source file to zero bytes — so
the guard is right and must not be routinely overridden. But it is a *detector*,
not a *remedy*: when it fires the agent is simply stuck, and the only advice it
can offer is to run `cargo clean` by hand in a worktree "nobody is building in".

Which worktree that is cannot be determined from the tree. Measured this
morning:

| Worktree | `target/` |
|---|---|
| `os` (integration) | 19.1 GB |
| `os-lane-a` | ~0.2 GB (host) + kernel target |

`os/target` is by far the biggest and is entirely regenerable, so it is the
right thing to prune — but at the time it had been written 157 s earlier by one
of three live `cargo test --workspace --target x86_64-pc-windows-gnu` processes,
and a `target/` that is idle for two minutes is *not* the same as one nobody is
using: a QEMU boot phase writes nothing to `target/` for ~8 minutes. Deleting it
under another lane costs that lane a ~14-minute rebuild and produces confusing
mid-build errors. So the one safe reclaim is also the one an agent cannot safely
perform without coordination it does not have.

Scratch also accumulates unattended: `os-lane-a/build/` held 1.16 GB of
`objdump` disassembly dumps (`dis-debug.txt`, 450 MB each) and ~80 MB of clippy
logs left by earlier sessions. That directory is gitignored, so nothing ever
prompts anyone to look at it. Pruned by hand on 2026-08-18; it will refill.

### Fix, part 1 — landed 2026-08-18: `scripts/reclaim-space.py`

Run `python scripts/reclaim-space.py` for a dry run, `--yes` to act, `--need N`
to set the target in GiB (default 20, the boot test's floor).

It answers "is this directory in use?" with a **fact rather than a heuristic**:
it *renames* the directory before deleting anything. Windows refuses to rename a
directory that has any file open inside it, and the rename is atomic, so a
success proves nothing held it at that instant and no observer ever sees a
half-deleted tree; a failure means "in use" and the candidate is skipped rather
than forced. That replaces the idleness check this entry originally proposed —
checking for a live `cargo`/`rustc` process — which was tried first and does not
work: `os/target` showed 122 s of write-idleness and was still locked 13 minutes
later, because a QEMU boot phase writes nothing to `target/` for ~8 minutes.

Order of attack: this lane's `build/` scratch older than `--scratch-age-days`
(default 3), then the integration checkout's `target/`, then **our own**, and
only with `--allow-lane-targets` any other lane's. Ours precedes theirs
deliberately: another lane's `target/` is that lane's rebuild exactly as ours is
ours, so a default run can only ever cost this lane and the integration tree.
Nothing outside a worktree root is touched and nothing git does not consider
ignored is touched; both are asserted rather than assumed, and the ignore query
is issued from the worktree that *owns* the path (`git check-ignore` fails
outright on a sibling worktree's path, which would otherwise read as "not
ignored").

Two things it deliberately keeps: `build/*.elf` (a `kernel-kasan-capture.elf` is
the symbol table an open bug's backtrace decodes against, and the commit that
produced it is gone from every build tree) and the boot test's disk images.

### Fix, part 2 — still to do: a retention rule for `build/`

`--scratch-age-days` prunes top-level `build/` files on demand, but only when
someone runs the script. The directory still has no standing policy, so it
refills silently between runs. Either the boot test should print `build/`'s size
when it exceeds a threshold, or it should age the directory out itself on every
run, so the accumulation stops being invisible.

### Status

**Status:** PARTIALLY FIXED — 2026-08-18. The reclaim helper exists and works;
the retention rule does not.

The underlying scarcity is not fixed and cannot be fixed by a script: three
lanes building concurrently consumed ~250 MB/min on 2026-08-18 and drove free
space from 19 GiB down to 14 GiB *while* the helper was retrying, with all four
`target/` directories locked the whole time. The helper's honest answer in that
window is "everything else is in use or not ours", and waiting is then the
correct behaviour — the space came back (14 GiB → 32 GiB) the moment the other
lanes' test runs finished.

**Severity:** high — it does not corrupt anything by itself (the guard sees to
that), but it stops the one test that gates merging to `main`, and the failure
mode it guards against has already destroyed a source file once.
