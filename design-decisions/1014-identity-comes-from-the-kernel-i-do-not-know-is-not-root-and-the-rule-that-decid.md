## 1014. Identity comes from the kernel, "I do not know" is not root, and the rule that decides is a function

**Date:** 2026-09-10
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `passwd` decided whether you were allowed to change someone
else's password by reading an environment variable -- a setting the person
running the command chooses. When the variable was absent it assumed root, and
it is normally absent, so the permission check never once ran. The answer now
comes from the operating system, and the rule that uses it is a separate
function with tests.

**The three decisions here, in the order they matter.**

**1. Ask the kernel, not the environment.** `getuid(2)` reports the credential
recorded at spawn; a process's parent cannot set it. The environment is the
parent's to write. There is no version of "read `$UID`" that is safe in a
program that grants privilege on the answer, so the variable is not consulted
at all -- not as a primary source, and not as a fallback either, which is where
`doas` and `polkit` still have it.

**2. `Option<u32>`, and `None` is not root.** The defect was not really the
environment lookup; it was `.unwrap_or(0)` on the end of it. That collapses "I
could not find out" into "the most privileged answer available". The type now
keeps them apart and callers must handle absence explicitly -- in `passwd`, by
refusing to run.

*The alternative considered:* return `u32` and use `u32::MAX` for unknown, as
`doas` does. Rejected because a sentinel is safe exactly as long as nothing
does arithmetic or comparison on it, and the entire purpose of this value is to
be compared. §1013 has the matching case in `su`, where a `u32::MAX` that was
harmless for months became a shell owned by nobody the moment `setuid` was
given it.

**3. The permission rule is a function that returns, not lines in `main` that
exit.** This is the part with a real tradeoff, because the three lines in
`main` were perfectly readable. They were also **unreachable for the entire
life of the program**, and nothing found that out, because a decision made in
`main` from process state cannot be reached from a test. Lifting it into
`permission(caller_uid, caller_name, target, named_explicitly, action)` cost an
argument list and bought the first six tests this program has ever had of who
may do what. One of them -- `a_user_may_not_change_another_users_password` --
is the bug.

**The failure mode worth naming.** A check that is always skipped looks exactly
like a check that always passes. Both produce a program that works, in the
sense that the intended user gets the intended result. The only way to tell
them apart is to reach the check with a caller who should be refused, and that
is a test, not a read.

**And the one that stings.** `userspace/oils` had already worked this out. Its
`reported_identity` deliberately ignores an inherited `UID=`, citing "precisely
the spoofing bash refuses", and it is in this same tree, written by the same
process, before the six programs that did the opposite. **A correct answer
already in the tree does not propagate by existing** -- nothing connects the
shell's reasoning about `$UID` to `passwd`'s use of `$UID` except somebody
grepping for the string. That is an argument for the shared function in
`authlib` over six correct copies: the copies would have been correct on the
day they were written too.

**Scope.** `passwd` is converted here. `chage`, `newgrp`, `polkit`, `crontab`
and `doas` are logged as
TD-B-FIVE-PROGRAMS-STILL-TAKE-THE-CALLERS-IDENTITY-FROM-THE-ENVIRONMENT and
follow.
