### [A] A-RELEASE-STALENESS-SELFTEST-WROTE-INTO-THE-PUSHED-REPOSITORY: a checker's self-test committed fixtures onto lane-a and set core.bare=true on the shared config -- 2026-09-26
**Status:** FIXED. The self-test was fixed in bc1bbad54, and the net under every gate is af5906d71. The config damage was repaired by lane C at 20:16, not by lane A.

**In short:** a test inside one of lane A's checking scripts builds a
throwaway git repository to count commits in. When the push hook ran it, git
pointed that test at the real repository instead (a hook's environment names
the repository being pushed, and that outranks the test's own directory). The
test wrote seven junk commits onto lane-a and switched a shared git setting,
`core.bare`, to "this repository has no working files". The commits were
removed within minutes. The setting was not noticed, and it broke `git status`
and `git pull` in the operator's own `os` checkout for six hours.

**What happened.** 13:48 -- `check-release-staleness.py` (6ff91aa8a) gained a
`--selftest` that makes a scratch repository with plain `git init`/`commit`
calls, without `scripts/gitenv.py`. 14:08:45 -- its first run inside
`pre-push` inherited `GIT_DIR`. `git init` therefore re-initialised the real
repository, and with no work tree in view it set `core.bare=true` in the
*common* config that every worktree shares. Seven fixture commits (`README`,
`kernel/src/a.rs`, `bench/*.jsonl`, `scripts/x.py`) landed on lane-a. The push
had already resolved its ref, so none was published. 14:10:58 -- lane A
stopped the push and rewound lane-a (`git reset --mixed 79da08f3a`).
14:17 -- bc1bbad54 fixed the self-test: `gitenv.clean_env()` everywhere, a
check before the first write that `--absolute-git-dir` is the scratch
directory's own, and `scripts/test-check-release-staleness.py`.

**What the repair missed.** Only the refs and the index were checked.
`core.bare=true` sat in `E:\visual studio projects\os\.git\config` from 14:08:45
until lane C found and reset it at ~20:16. Linked worktrees -- all six lanes --
keep working with it set, which is why no lane noticed. The main checkout does
not: the operator's `git status` and `git pull` in `os` failed with "this
operation must be run in a work tree", and a push from there was rejected.
Lane C also confirmed the rest: no stray `user.*` in the shared config, all
commits since 14:00 are the operator's identity, and none of the seven
fixtures is on any branch, local or remote.

**Lesson, now structural.** A repository escape damages refs, the index *and
the shared config*, and a repair that checks only the first two leaves the
third to be found by whoever uses the main checkout next.
`scripts/run-checker.sh`'s repository guard (af5906d71, `CHECKER_REPO_GUARD`,
set by `pre-push`) fingerprints this worktree's HEAD, its index and the
shared config around every gate -- the config through `git rev-parse
--git-common-dir`, the file every worktree shares. A gate that changes any of
them stops the push by name. Not the rest of the refs: its first version
fingerprinted every ref, and on its first push stopped a clean one because
lane E moved its own branch during a ten-minute gate -- six worktrees share
one refs namespace. `scripts/test-pre-push-run-checker.py` covers it both
ways:
- stopped: a gate that commits, one that stages a file, one that switches
  this worktree's branch, one that sets `core.bare` in a plain repository,
  and one that sets it from a linked worktree -- this incident's shape;
- passed: another lane committing on its own branch meanwhile, and a
  `branch.*` tracking entry written meanwhile;
- a mutant fingerprinting `--git-dir`'s config fails that last case, because
  a linked worktree's own git directory has no `config`.

This is the second escape of its kind. The first, 2026-08-29
(`check-requests-not-deleted`), set the same `core.bare=true` and was
published. `scripts/gitenv.py`'s docstring tells both stories.
