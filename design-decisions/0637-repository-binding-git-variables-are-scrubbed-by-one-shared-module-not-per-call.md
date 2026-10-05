## 637. Repository-binding git variables are scrubbed by one shared module, not per call site

**Date:** 2026-08-29
**Lane:** A
**Decided by:** Claude (autonomous, lane A) — after the `pre-push` gate
described in `known-issues.md`
(`A-A-PUSH-GATE-DELETED-THE-REPOSITORY-IT-WAS-GATING`) wrote two
tree-deleting commits onto `lane-a` and published them.

**In short:** when a program runs `git`, three different things claim to say
*which* repository it means: the directory the program is sitting in, an
explicit `-C <dir>` argument, and an environment variable called `GIT_DIR`.
The environment variable wins over both. Git sets that variable automatically
whenever it runs a hook — so a script that builds a scratch repository to test
itself against gets a scratch repository when a human runs it, and gets *the
real one* when the hook runs it. Ours did, and committed to it. The decision
here is where the defence lives: one shared module every caller uses, rather
than each caller cleaning its own environment.

**Context.** `scripts/check-requests-not-deleted.py --selftest` built a fixture
with `git init` / `git add -A` / `git commit` in a temp directory, each call
passing `cwd=<tmp>`. Run from `pre-push`, `GIT_DIR` pointed every one of them
at the repository being pushed. Two other suites
(`test-src-digest.py`, `test-boot-test.py`) build fixture repositories the same
way and carry the same latent bug, so the fix had at least three call sites
from the start, and the set of variables to remove is about twenty names —
`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`,
`GIT_QUARANTINE_PATH`, the `GIT_CONFIG_*` family and its numbered
`GIT_CONFIG_KEY_n` / `GIT_CONFIG_VALUE_n` pairs, among others.

**Decision.** `scripts/gitenv.py` owns the list. It exposes `clean_env()` for a
subprocess that should pick its repository by `cwd`/`-C`, and
`scrub_environ()` for a test harness that should never touch the ambient
repository at all. It is an explicit allow-through, not a blanket
`del GIT_*`.

**Rationale.**

- *One list, not three.* Three copies of a twenty-name denylist is three
  chances to omit `GIT_QUARANTINE_PATH`, and the omission is invisible until a
  hook runs. The list also grows: git has added repository-binding variables
  before and will again.
- *Two functions, because there are genuinely two situations.* A production
  script wants a scrubbed environment for the child it is about to spawn, and
  nothing more. A *test harness* has no business talking to the ambient
  repository at all, so scrubbing the process once is both stronger and
  unforgettable — it covers a call site added later by someone who never read
  this entry, and it covers non-git children, which matters concretely:
  `test-boot-test.py` shells out to `bash`, and the `bash` runs `git`, where a
  per-call `env=` on the Python side would never have reached.
- *Not a blanket `GIT_*` delete.* `GIT_EXEC_PATH` is on some installs the only
  thing telling git where its own subcommands live, and `GIT_SSH`, `GIT_TRACE`
  and the proxy settings are all things a caller may legitimately have set. The
  goal is to choose the repository, not to run git in a vacuum — a denylist
  that overreaches breaks git rather than redirecting it, which is a louder
  failure but still a failure.

**Alternatives considered.**

- **`env=` at each call site, no module.** Rejected: see "one list, not three".
  It also puts the twenty names in the diff of every script that spawns git,
  where they read as noise and get "tidied".
- **Pass `--git-dir` explicitly everywhere instead of scrubbing.** This does
  work for `GIT_DIR`, but not for `GIT_INDEX_FILE` or
  `GIT_OBJECT_DIRECTORY`, which have no command-line equivalent — so it fixes
  the loud shape of the bug and leaves the quiet one.
- **Have the fixtures not be git repositories.** The checker's self-test
  genuinely needs to exercise `git diff --name-status -M` against real
  commits; a mocked git would test the mock.
- **Rely on the self-test to notice.** It cannot: its verdict is a statement
  about the fixture, and in the failure the fixture *was* the repository.
  Every assertion it made was true while it was destroying the tree. This is
  why the regression test is a separate suite that snapshots a sacrificial
  repository from outside.

**Where it lives.** `scripts/gitenv.py` (the list and both functions);
`scripts/check-requests-not-deleted.py` (`clean_env()` on `_git()` and on the
self-test's fixture driver); `scripts/test-src-digest.py` and
`scripts/test-boot-test.py` (`scrub_environ()` at import);
`scripts/test-check-requests-not-deleted.py` (the outside-observer regression
suite).

**How to reverse.** Inline `clean_env()` into each caller and delete the
module. Note that doing so re-opens the question of who keeps the list current;
if this is ever reversed, the regression suite is the thing that must not be
deleted with it, because it is the only part that fails when the list is wrong.
