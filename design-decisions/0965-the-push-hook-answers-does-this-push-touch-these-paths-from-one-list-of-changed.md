## 965. The push hook answers "does this push touch these paths?" from one list of changed paths when the push publishes no merge, and asks git per call only when it does

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A

**In short:** before each push, the checking script (`scripts/hooks/pre-push`)
asks about forty times "does this push change anything under these folders?",
once per check, to decide whether that check needs to run. Each question
started a new git program, and on this machine with six lanes busy each start
costs a second or more, so most of a push's time went on asking. The script
now lists the files a push changes once and answers every question from that
list, which gives the same answers far faster. The one kind of push where a
list could give a different answer from git -- one that includes a merge --
still asks git every time.

**Why now.** The integration boot of `0bede19e6` was stopped after more than
five hours, still inside the tooling suites, because
`test-checkers-honour-head.py` alone had run for 2 h 40 m. Profiling it
(2026-09-25, six lanes building):

* its direct cases take 5-10 s each; its end-to-end cases 65-140 s, of which
  95% is two pushes through the real hook;
* one fixture push spent 130 s of its 172 s inside `touches`: 49 calls, each a
  command-substitution fork, a pipeline fork, a `git` process and a `head`
  process, with MSYS emulating every fork by copying the shell;
* the suite makes 84 such pushes -- 49 of them only to seed a fixture's
  remote, whose verdict nothing read.

Every lane's boot runs this suite, and every lane's push runs the hook.

**What was decided.**

1. **A push that publishes no merge is answered from a list.** On its first
   call, `touches_prepare` asks git two questions: is any published commit a
   merge (`rev-list --min-parents=2 --max-count=1`), and if not, what paths do
   the published commits change (`git -c core.quotePath=false log
   --no-renames --root --name-only --format=`). Every `touches` call is then
   answered by `case` matching in the shell, with no process started.
2. **The answer is git's, not an approximation of it.** `rev-list -- <paths>`
   differs from "some published commit changes a matching path" only through
   history simplification, which acts at merges. With no merge among the
   published commits there is nothing for it to act on -- a commit reachable
   from the pushed shas only through an already-published merge is itself
   already published. The matching rules are git's own, checked against git
   before being written down: `dir/` matches below dir; a literal `name`
   matches name and below it; `*suffix` matches any path ending in suffix,
   since `*` crosses `/`, and a wildcard never matches a leading directory
   (`*.md` does not match `a.md/b.txt`); everything is case-sensitive.
3. **Everything else still goes to git, per call:** a push that publishes a
   merge, a path git had to quote (a double quote, backslash or control
   character in its name), a scope spelled with any other wildcard or with
   pathspec magic, or a list that could not be built. The per-call question is
   the old one with `--max-count=1` in place of `| head -n 1`: one fork fewer,
   and git stops at the first commit it finds.
4. **Fixture seed pushes skip the hook.** The four suites that seed a fixture's
   remote by pushing now pass `--no-verify` and fail loudly if the seed is
   refused. The seed is the remote's starting state, not a push under test;
   its verdict was never read, and a refused seed does not fail, it silently
   widens the next push's range (`test-pre-push-unixhalf-gate.py` had already
   found that the hard way).

**Alternatives considered.**

| Option | For | Against |
|---|---|---|
| Keep asking git per call | Nothing to prove | ~1-3 s per call under load, forty-odd calls per push, well over a hundred hooked pushes per boot |
| Test the checker's presence before `touches` in every gate | Exact without any argument; cheap in fixtures | Forty-odd call sites to reorder in a file six lanes edit; no help to a real push, where every checker is present |
| Split a scope into its paths and cache each path's answer | Real pushes share paths (`userspace/`, `gui/`) | Not exact: with a merge, `rev-list -- a b` can differ from `rev-list -- a` or `rev-list -- b` |
| Answer from the list even when a merge is published | The ordinary lane push merges `main`, so it would be fast too | Would need history simplification reimplemented in shell; the published merge is already excluded, so only a push carrying a new merge pays |
| **The list for merge-free pushes, git otherwise** (chosen) | Exact; the fixture pushes and every merge-free push stop paying | A second code path in a security-relevant predicate, which is why it has its own suite |

**What holds it to this.** `scripts/test-pre-push-touches.py` lifts the helper
out of the hook verbatim and, for fifteen kinds of push -- a root commit;
modify, delete, rename, mode change and empty commit together and alone; two
refs in one push; a change and its revert; a published and an unpublished
merge; a wildcard against a path whose directory ends in its suffix; four
names git quotes; nothing published; a sha the remote already has --
requires the hook's answer to equal git's for every spelling asked.
The fixture repository's config is set to defeat each defensive flag
(`log.showRoot=false` against `--root`, `diff.renames=copies` against
`--no-renames`, `core.quotePath=true` against `-c core.quotePath=false`), so
dropping a flag fails the suite rather than a push on somebody's machine.
Sixteen mutations of the helper were run against the suite; three got past
it at first, and each was answered. Dropping `--no-color` changed nothing --
git does not colour `--name-only` names even under `color.ui=always` -- so
the flag was removed rather than kept as a precaution nothing can test, and
the fixture keeps `color.ui=always` so that a git which starts colouring
them fails here. Matching `*suffix` anywhere in a path rather than at its end
got past because every changed path holding `.md` also ended in it; the
scenario of a wildcard against a leading directory was added for it. And
matching a name's "below it" anywhere in a path rather than from the top got
past because no scope had been asked about a path it occurs inside; `src`
against `posix/src/lib.rs` was added. The other thirteen were caught as
first written, and every one of them against the helper as it stands now. It also fails if any gate's scope is ever spelled
in a way the list cannot answer, since such a gate would be judged correctly
and paid for on every push, silently.

**Where this bites:** `scripts/hooks/pre-push` (`touches_prepare`,
`touches_from_list`, `touches`), `scripts/test-pre-push-touches.py`, and the
seed pushes in `scripts/test-checkers-honour-head.py` (`_push_fixture`),
`scripts/test-pre-push-doclinks-gate.py`, `scripts/test-pre-push-fmt-gate.py`
and `scripts/test-pre-push-unixhalf-gate.py`.
