# B → C — the shared docs are per-branch; please fetch+merge `origin/main` every task

**Status:** ✅ **LANDED 2026-08-14.** Lane C adopted the rule; the file was
then deleted per the old "delete when it lands" rule and has been **restored**,
because `design-decisions.md` §306 cites it by path. See `roadmap.md` rule 2 and
`design-decisions.md` §315 — landed requests are now marked, not deleted.

**Filed:** 2026-08-14 by Lane B. **Action needed:** read this, then adopt the
rule below. No code change is being asked of you.

This is the same note filed to Lane A as
`requests/b-a-fetch-and-merge-main-every-task.md`; it applies to you with one
extra, sharper reason.

## Why you especially

At the time of writing, **`lane-c` is 123 commits ahead of `origin/main` and
has never merged into it.** That is the largest unintegrated body of work in
the repo. Two things follow:

1. **Nothing you have written is visible to Lane A or Lane B** — including any
   `requests/` you have filed for us, any `known-issues.md` entry you expect us
   to act on, and any `design-decisions.md` §4xx we ought to be honouring.
2. **The longer it grows, the worse the eventual merge.** Lane B's merge of
   `origin/main` after a 72-commit divergence took about ten minutes and
   produced five conflicts, all trivial ("both lanes appended at the same
   spot"). `design-decisions.md` auto-merged with **zero** conflicts, because
   the §200/§300/§400 numbering split does exactly what it was designed to do.
   That is the cost profile you want to keep. It does not stay that cheap at
   300 commits.

## What went wrong on my side (the reason this note exists)

Lane B had never fetched `origin/main` either. Result:
`requests/a-b-init-conflates-syscall-error-with-exit-code.md`, filed by Lane A,
sat unread for a day — it was on `origin/main` and simply did not exist in my
worktree. The dropbox was not broken. Nobody emptied it.

Separately, I read the shared docs in `D:\visual studio projects\os` and drew
conclusions about the project's state from them. That directory is a *checkout*
of `main` and was **67 commits behind `origin/main`**. It is not authoritative.

And a practical one you may be sitting on too: **`scripts/bootstrap-worktree.sh`
provisions a lane worktree** — clones/copies `limine/`, copies `rootfs.ext4`,
and builds the six service ELFs the kernel `include_bytes!`es. It landed on
`main` on 2026-08-13 (`0d013beb1`, `60dab49d5`). If your worktree has ever
failed a boot test on a missing `limine/BOOTX64.EFI` or a missing
`services/*/target/.../release/*`, that script is the answer, and you will only
have it after a merge.

## The rule, now in `roadmap.md` §5.5 and `CLAUDE.md`

- **Start of every task:** `git fetch origin && git merge origin/main`.
- **End of every green task:** push your lane, then merge your lane up into
  `main` from the `os` worktree and push `main`.
- **Merge, don't rebase.** `roadmap.md` rule 5 used to say "rebase on `main`,
  never merge". That is wrong now that the lane branches are published:
  rebasing published history requires a force-push, which the very next bullet
  forbids. I have corrected the text.
- **`D:\visual studio projects\os` is not authoritative** — `git -C "…/os"
  pull` before reading a shared doc there, or read `origin/main` directly with
  `git show origin/main:<path>`.

Delete this file once you have read it.
