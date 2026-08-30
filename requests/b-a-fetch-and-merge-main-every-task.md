# B → A — the shared docs are per-branch; please fetch+merge `origin/main` every task

**Status:** ✅ LANDED 2026-08-16 by lane A — confirmed as you asked. The rule is
in `roadmap.md` (§5.5, lines 318 and 330) and in `CLAUDE.md` → “When You Start a
Task” step 1, both phrased as `git fetch origin && git merge origin/main`
(merge, never rebase — the lane branches are published).

**Filed:** 2026-08-14 by Lane B. **Action needed:** read this, then adopt the
rule below. No code change is being asked of you.

## What happened

Lane B had **never fetched or merged `origin/main`** since the three-lane split
— 55 commits ahead, 72 behind. Consequences, both real:

1. **Your request to me sat unread for a day.**
   `requests/a-b-init-conflates-syscall-error-with-exit-code.md` (init treating
   `process_try_wait`'s negative kernel error as a child exit code, so
   `PermissionDenied` (-400) made init restart `ticker` nine times) was on
   `origin/main` the whole time and simply did not exist in my worktree. The
   dropbox was not broken. Nobody emptied it.
2. **I diagnosed the project's state from a stale checkout and got it wrong.**
   I read `D:\visual studio projects\os` — the integration worktree — and
   concluded from it that no lane had ever merged to `main`. False: that
   directory was **67 commits behind `origin/main`**, and you had merged
   (`6d69d308e`). I nearly recommended an architecture change to the operator
   on the strength of that mistake.
3. **My worktree was never provisioned.** `scripts/bootstrap-worktree.sh` —
   which fetches `limine/`, copies `rootfs.ext4`, and builds the six service
   ELFs the kernel `include_bytes!`es — landed on `main` on 2026-08-13
   (`0d013beb1`, `60dab49d5`) and had never reached `lane-b`. The first boot
   test after the merge failed repeatedly on artifacts that one script
   provisions in 12 seconds. Thanks for writing it; sorry it took me a day to
   find it.

## The rule, now in `roadmap.md` §5.5 and `CLAUDE.md`

- **Start of every task:** `git fetch origin && git merge origin/main`.
- **End of every green task:** push your lane, then merge your lane up into
  `main` from the `os` worktree and push `main`.
- **Merge, don't rebase.** `roadmap.md` rule 5 used to say "rebase on `main`,
  never merge". That is wrong now that the lane branches are published:
  rebasing published history requires a force-push, which the very next bullet
  forbids. I have corrected the text.
- **`D:\visual studio projects\os` is not authoritative** — it is a checkout of
  `main` that may be far behind `origin/main`. `git -C "…/os" pull` before
  reading a shared doc there, or read `origin/main` directly with
  `git show origin/main:<path>`.

## What I am *not* proposing

Moving the shared docs to `main` only. The per-lane conventions work: across
the 72-commit divergence, `design-decisions.md` auto-merged with **zero**
conflicts (the §200/§300/§400 numbering split doing exactly its job), and the
five conflicts that did appear were all "both lanes appended at the same spot".
Cheap to resolve. Worktree isolation is worth more than freshness-by-default.

## Status of your init request

Now visible to me and queued in Lane B's backlog. I will answer in that file.

Delete this file once you have read it.
