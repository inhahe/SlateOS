## TD-C-CREATING-A-FILE-THAT-ALREADY-EXISTS-DESTROYS-IT-SILENTLY

**Date:** 2026-09-14. **Lane:** C. **Repaired the same hour; kept as a trap.**

**In short:** writing a "new" file whose name is already taken deletes the old
one without a word. No tool involved says anything: `cat > f` truncates, the
Write tool overwrites, and git records it as an ordinary edit. On 2026-09-14
this destroyed `scripts/rustscan.py` -- 418 lines, eleven days old, imported by
seven scripts and wired into the boot test -- and left an unrelated module of
the same name in its place, on `origin/lane-c`, for about an hour.

**Three tells were present and all three were missed:**

1. **`git status` said `M`, not `??`.** A file you have just created is
   untracked. If it shows as *modified*, it existed before you wrote it. This
   is the cheapest check there is and it is one character wide.
2. **The commit's own diffstat.** Two genuinely new files showed pure
   additions; the third read
   `scripts/rustscan.py | 582 ++++++++-----------------`. **A file you just
   created cannot have deletions in it.** Any `-` in the bar is proof the name
   was taken. Mechanically: `git show --numstat` gives deleted-line counts, and
   a clobber deletes roughly the whole previous file.
3. **A gate refused the push and named the file.**
   `check-gate-call-sites.py` said `rustscan.py` is invoked with `--self-test`,
   "which appears nowhere in the script ... this gate reports OK having done
   something other than what the call site asked". That is an exact description
   of a module somebody has replaced. It was read as "my new library needs a
   self-test", because the reader assumed the file it named was his own.

The third is the one worth dwelling on: **the instrumentation worked.** The
tree caught this within minutes, in precise language, and the catch was
filtered through the very assumption that caused the damage. A correct alarm is
only as good as the reading, and the reading is done by whoever is least able
to doubt themselves at that moment.

**The rule, stated so it can be followed without judgement:** before creating a
file, test that the path does not exist. After creating it, confirm
`git status --short` shows `??` for it. Both are one command; neither requires
suspecting anything.

**Why the obvious defence does not work.** "Look before you write" only helps
someone who already suspects the name is taken, and the whole failure is not
suspecting. There are 222 scripts in this directory and no index of what they
do; the module destroyed here solves the exact problem its destroyer spent that
day rediscovering, and its docstring opens by naming the two traps -- "a comment
that mentions X", "a test that exercises X" -- that cost two lanes a day of
false positives and blind self-tests. **This is a discovery failure before it
is a discipline failure**, and the durable fix is an index of what the scripts
directory already contains, not more care.

**Sibling trap, same family** -- see lane A's entry on `git stash` creating no
entry on a clean tree, so a later `pop` restores *another lane's* stash: 18
unmerged paths from lane B's August work. Both are commands that succeed while
doing something other than what they look like, and both destroy work that was
never yours to lose.
