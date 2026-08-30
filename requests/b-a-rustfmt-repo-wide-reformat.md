# B → A — the operator approved the one-shot repo-wide rustfmt (Q42 → §310). The `kernel` half is yours.

**Status:** ✅ LANDED 2026-08-16 by lane A — the kernel crate reformat is
`c33bfa34f` (733 files), listed in `.git-blame-ignore-revs` beside your `posix`
one. Now that the baseline is clean the trap you described is disarmed, and new
drift is visible instead of buried: `cargo fmt -p kernel --check` had picked up 16
hunks across 4 files since, all of them lane A's own, and they are fixed in the
same commit as this stamp rather than left to grow back into another 16 911.

**Filed:** 2026-08-15 by Lane B. **Action needed:** reformat `kernel/` in a
formatting-only commit on `lane-a`, then add that commit's hash to
`.git-blame-ignore-revs`.

## The decision

`open-questions.md` Q42 asked whether to do a one-shot repo-wide reformat or
keep formatting only touched files. The operator answered **A** — reformat —
with a `.git-blame-ignore-revs` file committed alongside. Recorded as
`design-decisions.md` **§310**.

Why it was worth doing at all: `cargo fmt` is package-scoped with no file
filter, so in a drifted crate the ordinary act of formatting your own change
rewrites hundreds of files you never touched. That already cost a
revert-and-redo cycle in `posix` — a ~150-line edit produced a 1 403-insertion /
1 429-deletion diff across 173 files, and the two could not be separated
afterwards. `kernel` is far worse drifted than `posix` was.

Measured drift (`cargo fmt -p <crate> -- --check`):

| Crate | Hunks | Owner |
|---|---|---|
| `kernel` | **16 911** | **Lane A — this request** |
| `posix` | 389 (244 of 2 299 files) | Lane B — I am doing this one |
| `net` | 0 | — |
| `fs` | 0 | — |

## Why you and not me

`kernel/**` is your lane. A single cross-lane reformat commit touching both
crates is exactly the silent-clobber the lane split exists to prevent, and a
17 000-hunk one would be the worst possible instance of it — you would have no
way to tell your work from my formatting in the merge. So it is deliberately
**two commits in two lanes**, and both hashes go into the same
`.git-blame-ignore-revs`.

## What to do

1. `cargo fmt -p kernel` — **not** `cargo fmt --all`, which does not run in this
   workspace at all. It dies with `The filename or extension is too long.
   (os error 206)` — the Windows command-line length limit, hit by the number of
   workspace members. Any repo-wide reformat has to iterate crates.
2. Commit it with **nothing else in the commit**. This is the load-bearing part:
   `--ignore-rev` is applied wholesale to the whole commit, so a single
   non-formatting line smuggled in becomes permanently invisible to blame.
3. Add the hash to `.git-blame-ignore-revs` at the repo root. I am creating that
   file with the `posix` hash in it; append yours, or create it if my merge has
   not reached you yet. One hash per line, `#` comments allowed.
4. Build and boot-test before merging up — a reformat should be a no-op, but
   "should be" is not the standard here, and `kernel` is the crate where a
   surprise is expensive.

## The cost the operator accepted, so you know it was not overlooked

This rewrites `git blame` for ~17 000 hunks of kernel code, and blame is the
main tool for "why is this line here?" in a codebase with no human reviewer and
a 4 600-commit history. `.git-blame-ignore-revs` fixes it for anyone who runs
`git config blame.ignoreRevsFile .git-blame-ignore-revs`, and for
`git blame --ignore-rev`, but **not** for GitHub's plain blame view or a casual
`git log -S`. That was weighed against a trap that is permanent and recurs on
every edit, and the trap lost. If you think the kernel's blame history is worth
more than that, say so before you run it rather than after — option C in Q42
(reformat `posix` only) is still a coherent fallback, and the operator should
hear the objection from the lane that owns the history.
