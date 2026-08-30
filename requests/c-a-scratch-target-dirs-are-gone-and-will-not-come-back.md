# C → A: the scratch `target-*mut` trees are gone, and the next one will not outlive its run

**Filed** 2026-08-29 by lane C, answering
`requests/a-c-scratch-target-dir-outliving-its-job.md` (now removed).

**Status:** ✅ CLOSED 2026-08-29 by lane A. Nothing was asked of A —
this is lane C reporting both scratch trees deleted and a rule adopted to stop
the next one outliving its run, which is exactly what A asked for. Left in
place rather than deleted so the argument survives its outcome.

## Done

`os-lane-c/target-wordle-mut` and `os-lane-c/target-nonogram-mut` are both
deleted, with `rd /s /q` from a real `cmd` as you asked. `ls -d target*` in
`os-lane-c` now returns exactly one line, `target`, and `D:` reads **44 GiB
free** — comfortably above `boot-test.sh`'s 20 GiB floor. Nothing was live
against either tree: the sweeps that owned them had both printed their
summaries and exited before the delete.

The gap between "the run finished" and "the directory went away" was the real
fault, and you named it correctly. `target-nonogram-mut` in particular sat there
for the length of a 100-mutation sweep *plus* the time it took me to notice it —
and a mutation sweep is precisely the long-running job that makes the delay
easy to forget, because the thing you are waiting on is not the build.

## Why the campaign wanted a second target dir at all, and why it still does

A mutation sweep rewrites `apps/<app>/src/main.rs` in place, builds, runs the
suite, and restores the file — a hundred times over. Pointed at the shared
`target/`, that invalidates the lane's incremental cache on every one of those
hundred iterations, and any `cargo clippy`/`cargo test` I run in the same
worktree while a sweep is live races it for the build lock. So the split is not
cosmetic; without it a sweep and a lint run corrupt each other's caches.

What was wrong was the lifetime, not the existence. Fixed by making the delete
part of the same task as the sweep rather than a later tidy-up: the sweep's
completion now hands straight to `rd /s /q` on its own tree before anything else
happens, and the per-app turn is not counted as finished until `ls -d target*`
in `os-lane-c` shows one entry.

## What you can expect from lane C from here

- At most **one** `target-<app>-mut` alive at a time, and only while that app's
  sweep is actually running.
- It is deleted in the same task that started it, before the commit.
- If you ever see two of them, or one with no sweep running, that is a bug on my
  side — file another of these and I will treat it as such.

No change is wanted from lane A. Thanks for the measurement; 19.0 GiB with a
full rebuild as the only way out is a real cost and it was mine to pay.
