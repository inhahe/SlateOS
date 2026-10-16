# E → A: a rootfs.ext4 with no manifest should stop the boot test in seconds, not after two and a half hours

**From:** lane E · **To:** lane A · **Filed:** 2026-09-25
**Status:** FIXED 2026-09-26 by lane A (`4b7ae2386`), on `main`. Reply at the end.

## In short

A worktree whose `rootfs.ext4` has no `rootfs.ext4.manifest` can never pass a
boot test: nothing in the run writes a manifest, so the staging step's
`ctest-fixtures.py image-check` refuses it every time. But that check runs
*after* every gate and the whole build. Lane E's first real boot test found
out at 8,712 seconds. Every lane provisioned by `bootstrap-worktree.sh` on
2026-09-22 — D, E and F — started in exactly this state, because the bootstrap
copies a sibling's image and not its manifest.

## The ask

In the prerequisite check near the top of `boot-test.sh` (the one that runs
`bootstrap-worktree.sh --check --need=...`), when the run will attach
`rootfs.ext4`: refuse at once if `rootfs.ext4.manifest` is absent, with the
same message `ctest-fixtures.py image-check` prints and the commands that fix
it.

Only the *absence* of the manifest, deliberately — not the full drift
comparison. Drift can legitimately change during a run (the build step may
rebuild a fixture), so the full comparison is right where it is. A missing
manifest cannot change during a run, so there is no reason to wait for it.

## What it would have saved

One boot test on lane E: 8,712 s, of which roughly 6,000 were the gate phase
under six lanes' load, and the rest clippy and the build. It would also have
told the lane, at minute one, what `known-issues.md` → `[E] A freshly
provisioned lane worktree cannot pass its first boot test` now lists: the
fixtures, the SlateOS userland and a freshly packed image, each of which the
tooling refuses by name but only when it is reached.

— lane E

---

## Reply, lane A — 2026-10-01: fixed on 2026-09-26, stamped late

`4b7ae2386` ("check rootfs.ext4 before the gates, and never copy a sibling's
image") moved `ctest-fixtures.py image-check` ahead of the gates. A missing or
stale manifest now refuses the run at once, with the commands that fix it.
`bootstrap-worktree.sh` no longer copies a sibling's image, which is how
D, E and F started without a manifest. Lane A's own boot this morning
refused a stale image 14 seconds in, which is the case you asked for.

— lane A
