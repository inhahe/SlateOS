## §308 — A private file stays out of GitHub via a pre-push hook plus an orphan branch, not by being untracked

**Date:** 2026-08-14
**Decided by:** Claude (operator-approved scope) — the operator set the
requirement verbatim ("todo2.txt can be committed to the local git, but i don't
want it on github"); the mechanism below is my call and is mine to revisit.

**The requirement, and why it is not trivially satisfiable.** Git has no
per-file push filter. A pushed branch carries every file its commits contain,
and every earlier version of that file that its history contains. So "tracked
locally but never published" is not a property you can attach to a *file* — it
has to be enforced at the boundary where publication actually happens.

That matters more here than in a normal repo, because three autonomous agents
push freely and often, under a standing instruction to "push often, on your own
volition". Anything tracked on a shared branch reaches GitHub within minutes,
by design. A convention would not survive that.

**Decision — three parts, each covering a gap the others leave:**

1. **`/todo2.txt` stays in `.gitignore` on the shared branches.** This is the
   cheap guard: it stops the file being swept up by a `git add -A`, which is
   how it would realistically get committed by accident.
2. **A `pre-push` hook (`scripts/hooks/pre-push`) refuses any push whose new
   commits add or touch a guarded path.** This is the guard that actually
   implements the operator's requirement, because it holds even when the file
   *is* committed. It is deliberately not a blanket block: it inspects the tip
   tree of each ref being pushed and the commits not yet on any remote-tracking
   branch, so ordinary lane pushes are untouched and only an actual leak is
   stopped. `ALLOW_PRIVATE_PUSH=1` bypasses it for the day the operator changes
   their mind.
3. **Local history lives on the orphan branch `private/todo2`,** appended to by
   `scripts/snapshot-todo2.sh` using plumbing (`hash-object` / `mktree` /
   `commit-tree` / `update-ref`) so no worktree HEAD ever moves. This is the
   part that delivers "can be committed to the local git" for real — the file
   gets genuine version history and diffs — without that history riding on a
   branch anyone pushes.

**Why the file cannot simply be committed on `main` or a lane branch.** This is
the option that first looks right and is in fact the worst one. Those branches
are pushed constantly; with the hook in place, committing `todo2.txt` on `main`
would make *every subsequent push of `main` by any of the three lanes* fail
until someone removed it. The requirement would be met and the project would be
bricked. An orphan branch shares no commits with the project, so it can never be
dragged along by a merge or a push of something else.

**Alternative considered: a separate local-only repository** (a bare repo
outside the tree, driven with `--git-dir`/`--work-tree`). Strictly safer — there
is no remote at all, so no hook to forget. Rejected because it puts the
operator's file under a second VCS with its own path incantations for every
read, and because the failure it prevents (a hook lost to a fresh clone) is
already handled: the hook source is *tracked* at `scripts/hooks/pre-push` and
`scripts/install-hooks.sh` re-arms it in one command. Worth revisiting if the
set of private files ever grows beyond one.

**Alternative considered: rely on `.gitignore` alone** (the status quo before
this entry). Rejected because it answers a different question. Ignoring a file
prevents it being *staged*; it does nothing once the file is tracked — and the
file *was* tracked on all five branches until 2026-08-14, which is exactly how
it reached GitHub in the first place. `.gitignore` guards the input; the hook
guards the output; the operator asked about the output.

**Limits, stated plainly.** The hook stops *future* publication. It does not
remove `todo2.txt` from history already on GitHub — every revision pushed
before 2026-08-14 is still reachable there, and purging it would require
rewriting published history and force-pushing, which this project forbids and
which would break all three lanes. Hooks are also per-clone and not carried by
`clone`/`fetch`; all four worktrees share one `.git`, so one install covers
every lane today, but a fresh clone starts unarmed.

**Where it bites:** `.gitignore`, `scripts/hooks/pre-push`,
`scripts/install-hooks.sh`, `scripts/snapshot-todo2.sh`, the orphan branch
`private/todo2` (never merge it into anything), and
`requests/b-a-todo2-untracked.md` / `requests/b-c-todo2-untracked.md`, which
tell lanes A and C why a push of theirs might be refused.
