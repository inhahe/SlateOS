### [A] TOOLING-A-FULL-DISK-TRUNCATES-THE-FILE-YOU-WERE-EDITING-TO-ZERO-BYTES — ⚠️ RECOVERED 2026-08-15, HAZARD STANDS

**Status:** the one damaged file was restored from git the same minute; the
underlying hazard is not fixed and cannot be fixed from inside this repo.

**What happened.** Mid-edit of `kernel/src/cap/table.rs`, the edit failed with
`ENOSPC: no space left on device, write`. `D:` was at **100% full, 0 bytes
free**. The failure was not atomic: the editor had already opened the file for
truncation, so the write that could not proceed left **`table.rs` at 0 bytes** —
the whole 18 636-byte file gone, not just the edit. `git status` showed it as an
ordinary ` M` modification, which is exactly what a file emptied by a crash
looks like to git.

Recovery was `git checkout -- kernel/src/cap/table.rs` plus re-applying the
edit, and cost nothing because the file was committed. **That is the entire
mitigation, and it is luck, not design:** had the truncated file been one of the
uncommitted ones in the same change set (`cap/mod.rs`, `proc/pcb.rs`,
`syscall/handlers.rs` — the `SYS_CAP_QUERY` work, then ~2 hours old and not yet
committed), the content would have been unrecoverable.

**Why this is worth an entry rather than a shrug.**

1. **The failure mode is silent-by-shape.** A zeroed source file still compiles
   *in the sense that cargo will report errors somewhere else* — every user of
   `CapTable` breaks, so the diagnostics point away from the emptied file. The
   one signal that names the real cause is the `ENOSPC` string in the tool
   error, which scrolls past in the same turn as fifty other lines.
2. **Every other file written in that window is suspect.** After an ENOSPC, the
   correct response is not "retry the edit" but "check the size of everything I
   have touched" — which is what was done here (`ls -la` over all six edited
   files confirmed only `table.rs` was hit). Skipping that check would have left
   a second zeroed file to be discovered by a confusing build error later.
3. **The disk did not fill up by accident and will do it again.** `D:` is
   1.9 TB, ~1.8 TB of it the operator's data, and the OS project's build
   artefacts are the elastic part. Deleting `os-lane-a/target` alone returned
   **13 GB**, which is the measure of how much a single lane's `debug` +
   `release` + `x86_64-unknown-none` trees hold. Three lanes plus the `os`
   integration tree carry four such directories, and nothing prunes them.

**Prescription (not yet done, and the first item needs the operator).**

- **[operator] Decide whether the three lane worktrees should share one
  `CARGO_TARGET_DIR`.** Sharing would cut the artefact footprint by roughly the
  redundancy factor, at the cost of serialising the lanes' builds against one
  lock — which on this machine is arguably a *feature*, since concurrent lane
  builds are already the leading cause of the benchmark contamination this file
  documents elsewhere. Logged as an open question rather than decided here
  because it changes how all three agents build, not just Lane A's.
- **[A] Have `boot-test.sh` refuse to start below a free-space floor** and say
  so, instead of failing deep inside a link step with a truncated artefact. A
  build that dies at 90% completion on a full disk can leave a *stale but
  plausible* kernel image staged in the ESP, and `--no-build` would then
  cheerfully boot it — a wrong-binary run that reports as a normal one.
- **[A] Prefer committing before a long edit chain over trusting the editor.**
  The only reason this cost minutes rather than hours is that `table.rs` was
  committed. The five files that were *not* committed survived by chance.

**The object store was checked, not assumed.** The same ENOSPC can hit `git`
itself — a failed `.git/index.lock` write, a half-written loose object — and the
working tree looking fine says nothing about that, because the damage would only
surface on a later `checkout` of the object that failed to land. `git fsck` was
run immediately after recovery: **no `error`, `missing` or `broken link` lines —
only `dangling` objects**, which are the ordinary detritus of amends and resets
and mean nothing is *unreachable that should be reachable*. The repository took
no damage from this incident.

Two notes on how that was established, because the first attempt did not
establish it: the run was piped through `head -20`, and dangling objects filled
all twenty lines — so "no errors in the output" was a statement about the first
twenty lines, not about the check. It was re-run as
`git fsck --no-progress | grep -v '^dangling'`, which returned nothing. A filter
that can be saturated by benign output is not a check; recording the *reassurance*
("the repo stayed responsive") instead of the *result* would have been worse
still, since it is evidence of nothing at all.

**Update 2026-08-31 (lane A) — the 20 GiB floor is now unreachable by anything a
lane can do, and the reason is that its premise no longer describes the
machine.** `D:` is at **11.4 GiB free of 1.9 T, 100% used**. The floor was sized
on 2026-08-15 against "138 GB of build output across four worktrees (59.1 / 40.4
/ 35.0 / 3.5)". Today there is **exactly one `target/` on the volume** —
`os-lane-a/target`, ~6.9 GiB — and none in `os`, `os-lane-b` or `os-lane-c`.
`reclaim-space.py --need 20` finds that one candidate and reports it would reach
18.4 GiB: still under the floor, and only by spending the very cache the run
about to happen needs, so a cold rebuild puts the volume straight back where it
started. The elastic part the prescription above counted on has already been
squeezed out; the remaining 1.8 T is the operator's own data (Dropbox, Hyper-V,
IDriveLocal, backups) and is not ours to delete.

**What was done for the run at `c50b8e234`:** committed and pushed everything
first, then `--min-free-gb=8` for one run. That is defensible on the floor's own
stated terms and not a way around them — the floor exists "to leave enough room
that the *editor* and git keep working while a build is refused", because
"losing an uncommitted file costs the work", and against a clean, pushed tree
there is no uncommitted file to lose. The other half of the floor's rationale (a
link step dying part-way and leaving a stale-but-plausible kernel image in the
ESP for a later `--no-build` to boot) is unaffected by the lowering: 8 GiB is
still far more than a link step needs, and the run was not `--no-build`.

**This is not a fix, and it should not become the habit.** The override is only
sound while the tree is clean; running it with edits in flight reinstates
precisely the 2026-08-15 failure. The two structural prescriptions above still
stand and neither has been done:

- **[operator] the shared `CARGO_TARGET_DIR` question** — now with a stronger
  argument than in August, since the redundancy it would remove is currently
  being paid for by refusing to boot-test at all.
- **[A/operator] the floor needs to be a function of what is actually on the
  volume**, not a constant sized against a four-worktree world that no longer
  exists. A floor no run can ever clear stops being a guard and becomes a step
  every run learns to skip — which is worse than a lower floor honestly set,
  because a skipped guard protects nothing on the day the tree *is* dirty.
