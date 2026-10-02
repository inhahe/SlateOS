## TD-A-GIT-STASH-ON-A-CLEAN-TREE-MAKES-NO-ENTRY-SO-POP-TAKES-SOMEONE-ELSES (lane A, 2026-09-14)

**In short:** `git stash` followed later by `git stash pop` is a safe-looking
pair that is only safe when the stash actually saved something. On a clean tree
`git stash` **creates no entry and exits 0**, so the matching `pop` restores
whatever was already on top of the stack -- which in a shared repository is some
other lane's work.

**What happened.** Lane A wanted to compare a script against its previous
version, so: `git stash -q`, check out the old copy, `git stash pop -q`. The
tree was clean, `stash -q` saved nothing and said nothing, and the `pop`
restored **lane B's stash from 2026-08-21** -- "stale local ctest fixture
rebuild" -- leaving **18 unmerged `DU` paths** under `services/ctest-*`.

**Why it is nastier than it looks.**

* Both commands **succeed**. There is no error to notice, and `-q` removes even
  the "No local changes to save" line that would have been the only hint.
* The stack is **shared across worktrees**. `git stash list` here shows entries
  from lane A, lane B and lane C, so a wrong `pop` reaches straight into another
  agent's uncommitted work.
* The damage looks like *your* merge conflict. Eighteen unmerged paths in a
  subsystem lane A never edits is confusing rather than obviously foreign.

**The rule.** Never pair `stash` with `pop` on trust. Either check that an entry
was created -- compare `git stash list | wc -l` before and after, or read the
output without `-q` -- or avoid the pair entirely: `git show <rev>:<path>` reads
an old version with no working-tree change at all, which is what was actually
wanted here and would have taken one command.

**Repair, for whoever hits it next.** It is recoverable if nothing of yours was
uncommitted: confirm every conflicted path belongs to the foreign entry
(`git status --porcelain`), confirm the entry was **kept** rather than dropped
(`git stash list`), confirm your own commit is still `HEAD`, then
`git reset --hard HEAD`. The pop keeps the entry when it conflicts, so the other
lane loses nothing -- but that is luck, not design: a *clean* pop would have
dropped their stash entirely.
