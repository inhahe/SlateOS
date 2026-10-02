## `A-A-PUSH-GATE-DELETED-THE-REPOSITORY-IT-WAS-GATING` (lane A, 2026-08-29) — **FIXED 2026-08-29**, published damage

**Status:** FIXED 2026-08-29 (`f0534726e` repair, `31eb8c6bd` root cause,
`93fb3227b` the same latent bug in two more suites, and a later config repair
-- see "The residue" below, which is the part that was still broken after this
entry first said FIXED)

**In short:** a safety check that runs just before `git push` built itself a
scratch repository to test itself against. It did not get a scratch
repository — it got *this* one, and its scratch commits landed on `lane-a`
and were published to `origin/main`. The commit it wrote has a tree
consisting of one `requests/` directory: as far as those two commits are
concerned, the entire operating system was deleted. The gate reported
success throughout, because it had genuinely verified a fixture and the
fixture was the repository.

### What actually happened

`pre-push` gate 9 runs `check-requests-not-deleted.py --selftest` before
trusting the checker. The self-test builds a fixture with `git init` /
`git add -A` / `git commit` in a `tempfile.TemporaryDirectory`, each call
passing `cwd=<tmp>`.

**`GIT_DIR` outranks both `cwd=` and `git -C`.** And git *exports* `GIT_DIR`
— along with `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_WORK_TREE`,
`GIT_QUARANTINE_PATH` and the `GIT_CONFIG_*` family — into the environment of
every hook. So on the first real invocation, every fixture command operated
on the repository being pushed:

| Fixture command | What it did to this repository |
|---|---|
| `git init` | re-initialised it and set `core.bare=true` on the shared config, which made the `os` integration worktree unusable — `git status` there answered "this operation must be run in a work tree" |
| `git add -A` | replaced the index with the fixture's three files |
| `git commit` | wrote `7f6a6b446` ("base") and `71f164f7e` ("delete one, sweep another") onto `lane-a` |

The commit *messages* are what identified it: "base" and "delete one, sweep
another" are the fixture's own strings, appearing in the history of an OS
kernel. `git ls-tree 71f164f7e` confirmed it — a single `requests` entry.

Both commits were then pushed to `origin/lane-a` and `origin/main`, because
the gate passed.

### The repair, and why it is a merge and not a force-push

Force-pushing is forbidden outright here, and the bad tip was already
published. The fix was to make a commit whose *content* is correct but whose
first parent is the bad tip, so both refs fast-forward onto it:

```
git commit-tree "79abf2546^{tree}" -p 71f164f7e -p 79abf2546   # -> f0534726e
```

Verified with `git diff --stat 79abf2546 f0534726e` (empty — the tree is
exactly the good one) and `git merge-base --is-ancestor 71f164f7e f0534726e`
(true — so it fast-forwards). Then a single push updated both refs
`71f164f7e..f0534726e`. `core.bare` was set back to `false` in `os`.

**The two junk commits remain in `main`'s ancestry, permanently.** They are
ancestry, not content: no file in the tree reflects them. Removing them would
require rewriting published history, which needs a force-push. That is the
price of the no-force-push rule and it is the right trade — but it means a
`git log` of this repository will always show two commits that appear to
delete everything, and this entry is the explanation a future reader will
need.

### Root cause fix

`scripts/gitenv.py` is now the single place that knows which variables bind a
repository. `clean_env()` for a subprocess that should choose its repository
by `cwd`/`-C`; `scrub_environ()` for a test harness that should never touch
the ambient repository at all. It deliberately does **not** drop every
`GIT_*` — `GIT_EXEC_PATH` is on some installs the only thing telling git
where its own subcommands live, so a blanket denylist would break git rather
than redirect it.

### Why the self-test could not have caught this, and what does

A self-test cannot detect that it corrupted the repository it runs from: its
verdict is a statement about the fixture, and here the fixture *was* the
repository. Every assertion it made was true. The regression test therefore
had to be an **outside observer** —
`scripts/test-check-requests-not-deleted.py` snapshots a sacrificial repo
(HEAD, branch, refs, `core.bare`, index, log, status), runs the checker
against it under three hook environments, and diffs the snapshots.

Two things about that test are load-bearing and easy to get wrong:

- **`GIT_DIR` alone is the shape that matters**, because it is what `git
  push` actually sets, and it is the shape that fails *quietly* — the fixture
  succeeds and writes commits. Adding `GIT_INDEX_FILE` makes git crash early,
  so the run is caught by exit code. A test that only covered the loud shape
  would have proved the least.
- **The probes must not raise.** The first version called `git` and let
  failures propagate; against a repository the mutant had just made bare, the
  snapshot threw and killed the harness with a traceback, losing the other
  two environments. `_probe()` now returns a sentinel string, so a repository
  too broken to answer still produces a *diff* rather than a crash.

Every fix was mutation-tested by reverting it individually. Two mutants
initially escaped, and each escape was a real weakness in the test rather
than a nuisance: the snapshot-crash above, and an assertion that checked only
for `"OK"` in the output, which a *foreign* repository satisfies just as well
— now replaced by comparing the computed merge-base sha in both directions.

### The generalisation, which is the part worth keeping

Two other suites (`test-src-digest.py`, `test-boot-test.py`) build fixture
repositories the same way. Neither is reachable from a hook today, so neither
had fired — but "not currently reachable from a hook" is not a property
either file states, nor one a reviewer can check, and `git bisect run`,
`git rebase --exec`, `git filter-branch` callbacks and `git submodule
foreach` export the same variables. Both were fixed anyway, via the shared
module rather than three copies of a twenty-name denylist.

**The check that would have caught this within seconds:** `git rev-parse
lane-a` immediately after a push, compared against what was pushed. A push
gate is the one program guaranteed to run with a repository named in its
environment, and it is also the one nobody watches.

### The residue: closing the leak did not undo what it had already written

Found later the same day, by running exactly the post-push check recommended
just above. It printed `core.bare: true` for a repository that is not bare.

The fixture does three things before it commits anything, and through the
leaked `GIT_DIR` all three landed in the *real* shared config at
`os/.git/config` rather than in its temporary directory:

| Fixture line | What it wrote into the real config | Effect |
|---|---|---|
| `run(tmp, "init", ...)` | `core.bare = true` | the `os` integration worktree stopped being a worktree |
| `run(tmp, "config", "user.email"/"user.name", ...)` | `[user] selftest <selftest@example.invalid>` | the commit identity of **all three lanes** |
| `run(tmp, "config", "diff.renames", "false")` | `[diff] renames = false` | rename detection off in every diff and log, everywhere |

Worktrees share one config, so none of this was confined to lane A.

`core.bare` was the loud one: `git status` in `os` answered `fatal: this
operation must be run in a work tree` and `git worktree list` reported the
tree as `(bare)`. Nothing was lost -- all 107 top-level entries were present
and `HEAD` still named `refs/heads/main` -- but `os` is the tree the
"merge your lane up to `main`" step is performed in, so that step had been
quietly impossible since the incident.

`user.email` was the quiet one, and therefore the expensive one. **33 commits
are permanently authored `selftest <selftest@example.invalid>`** -- 16
reachable from `lane-a`, 9 from `lane-b`, 3 from `lane-c`, 5 from `main`, all
pushed. The oldest is the fixture's own `base` commit at 22:43:17; every
commit any lane made afterwards inherited it, including the commits that
fixed the incident. They cannot be repaired: changing an author rewrites the
commit, which requires a force-push to published branches. That is the same
decision already queued for the operator over the two junk commits, and it is
recorded there rather than duplicated here.

`diff.renames = false` deserves its own note, because the gate's own source
comment names it as the configuration that "would otherwise turn every sweep
into a refused push" -- the fixture set the hostile condition it was written
to simulate, on the real repository, and then the gate went on passing.

Repaired by unsetting the three; the operator's identity resolves again in all
four worktrees and `os` is a checkout of `main` once more. **The leak itself
was verified closed rather than assumed:** the self-test was re-run with
`GIT_DIR`, `GIT_WORK_TREE` and `GIT_INDEX_FILE` all pointed at the real
repository -- the precise conditions of the incident -- and the config hash,
the ref hashes and `HEAD` were byte-identical afterwards.

**The lesson, which is not the same as the one above.** That one was about a
check that could not observe its own damage. This one is about what happens
*after* a fix: closing a leak does not undo what leaked through it. The
commits were found immediately because a tree with the whole OS missing is
impossible to miss; the config was not found for another hour because a
config setting makes no noise at all. So when a bug is found to have written
to shared persistent state, the fix is only half the work -- the other half is
diffing that state against what it should be, and *the quiet damage is the
part that is still there*.

**Addendum 2026-08-30 (lane B) -- "the count cannot grow" was true and not
enough, so there is now a gate.** The entry above, and the open question it
points at, both close on the reasoning that the poisoned config is repaired and
so the 33 commits are a bounded, historical blemish. That is correct as far as
it goes. What it does not cover is recurrence, and the history has a plain
answer about recurrence that neither write-up mentions: **it already recurred
once.** `786139a5a` ("base") and `2b7d0f0a3` ("delete one, sweep another"), on
`lane-b`, are a second pair of fixture commits whose tree is the repository
deleted -- made at 23:47, **twenty-five minutes after** the 23:22 repair
(`31eb8c6bd`, "Stop the request-deletion self-test writing to the repo being
pushed"), and repaired again in `6caf6467e`. So the interval between "the cause
is fixed" and "the cause fires again" was measured here at 25 minutes.

That is the argument for a mechanism rather than a corrected assumption. As of
this entry the push boundary carries **gate 10**: a commit whose author *or*
committer address sits in a domain the standards reserve for testing (RFC 2606
`.invalid`/`.test`/`.example`/`example.com|net|org`, RFC 6761 `.localhost`) is
refused before it can be published. It would have stopped both halves of this
incident -- the four junk commits and all 33 misattributions carried the same
`selftest@example.invalid`. It is a denylist rather than an allowlist of the
operator's identity specifically so that it has no false positives to be worn
down by; the full trade-off is `design-decisions.md` §708, and the behavioural
coverage (which drives the real hook over real pushes, and is mutation-tested
in both directions) is `scripts/test-pre-push-identity-gate.py`.

Nothing here changes the operator's pending decision, which is only about
whether to rewrite the history that already exists. This is about the history
that does not exist yet.

### Addendum 2026-09-04 (lane B) -- it recurred a third time, and gate 10 held

The addendum above was written to argue that a mechanism was needed because the
count *could* grow. It grew. This is the first recurrence since gate 10 existed,
and it is the first one that cost nothing, so the comparison is the point.

**What happened.** Lane B's commit at 23:03:31 was authored
`selftest <selftest@example.invalid>` -- the same address as 2026-08-29, so the
same fixture, not a new one. Lane B did not notice at the time and had no reason
to: nothing in `git commit`'s output, `git log --oneline`, `git status` or the
diff shows an author. It surfaced 45 minutes later as a **refused push**, which
is the entire value of putting the check at the push boundary rather than
anywhere earlier.

**The window is bounded, which is what makes this entry evidence rather than an
anecdote.** Both neighbouring commits are correctly attributed, so the shared
config was poisoned and then cleaned inside a known interval:

| Time (UTC) | Event | Evidence |
|---|---|---|
| 02:52:34 | lane B commits `251efc144`, authored correctly | commit metadata |
| 02:58:01 | **lane A runs five gate self-tests in a loop**, `test-check-requests-not-deleted` among them | lane A session log |
| 03:03:20 | lane B commits, authored `selftest` | commit metadata |
| ~03:04 | **lane A's own worktree is found damaged** -- HEAD and index wrong, recovered with `git reset --hard 0ab4b55bc` | lane A session log |
| 03:07:40 | lane A unsets `user.name`/`user.email` from `os/.git/config` | `os/.git/config` mtime + lane A session log |

So the writer was a self-test running from **another lane's worktree**, and the
damage crossed lanes because worktrees share one config -- the same coupling the
original entry identified. Lane B's investigation initially looked for the cause
in lane B's own tree and its own recent commands, and found nothing, correctly:
*there was nothing there to find*. **When shared state is poisoned, the search
must start from the state's mtime and not from your own history**, because on a
three-lane tree the prior probability that you were the writer is one in three.

**What it cost, versus what it cost last time.** 33 commits, permanently, across
three lanes, plus two published tree-deleting commits -- against one commit,
caught before publication, repaired in full. The repair was a `filter-branch
--env-filter` over the nine unpushed commits, keyed on the bad address:

```bash
FILTER_BRANCH_SQUELCH_WARNING=1 git filter-branch -f --env-filter '
if [ "$GIT_AUTHOR_EMAIL" = "selftest@example.invalid" ]; then ... fi
if [ "$GIT_COMMITTER_EMAIL" = "selftest@example.invalid" ]; then ... fi
' -- HEAD --not --remotes
```

`--not --remotes` is the load-bearing part: it confines the rewrite to commits
no remote has, so this is a rewrite of private history and **not** a force-push.
It was chosen over the recipe the hook itself suggests
(`git rebase --exec 'git commit --amend --no-edit --reset-author'`) for two
reasons the hook's text does not anticipate: the range contained a merge commit
with hand-resolved conflicts, which a rebase would have flattened or replayed;
and `--reset-author` would have moved the author *dates* of eight commits that
were never wrong. Verified content-neutral by comparing all nine trees
pairwise against a backup ref -- every one identical, including the merge's.

**Two things this corrects elsewhere.**

- During the `A-27` / CRLF work of the same day, `check-eol` run against lane
  A's worktree refused with `enumerated 1 of 1 tracked files, floor is 500`.
  That was put down at the time to a live renormalisation and never written up,
  so this is its only record. The attribution was probably wrong: lane A's index
  was demonstrably being overwritten by fixture runs in that window, and "one
  file in the index" is precisely the shape of a fixture repository. The reading
  was most likely the same incident seen from the outside. It is left as a
  probable rather than a proven cause because the two explanations were never
  distinguished at the time.
- `check-eol.py`'s `DISCOVERY_FLOOR = 500` is what turned that into a visible
  refusal instead of a clean bill of health. A gate that had trusted
  `git ls-files` would have reported lane A's tree as having zero CRLF findings
  and been *right about the file it read*. **A guard against an implausibly
  small input set earns its keep on the day another process is writing to your
  input**, which is not the failure it was written for.

**What is not fixed, and whose it is.** The writer is a self-test in `scripts/`,
which is lane A's tree and lane A's ongoing work -- they committed
`scripts/test-selftests-are-repo-safe.py` ("prove no gated self-test can damage
the repository it runs from") within the hour. Lane B has deliberately not
duplicated that. What lane B adds is the timeline above, because a *bounded*
recurrence window is the one thing neither previous write-up had.
