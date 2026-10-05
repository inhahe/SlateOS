## 1045. Names that live inside another program: case by case, and a kept name is installed as that program

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q11 with "Claude's recommendation":
option D, with a default of B for any name kept). Relayed verbatim through
lane F's session.

**In short:** some programs answer to several names -- one file installed as
both `useradd` and `userdel` does two jobs. 169 such extra names existed when
this was asked (148 today) and nothing installed any of them, so their code
was finished, tested and unrunnable. Each name is now decided on its own: a
name for a subsystem SlateOS does not have is deleted (§1006); a name that is
kept is installed as the same file under the extra name, as busybox does, and
gets a program of its own only where separate permissions for it matter.

**Why B is the default and A the exception.** SlateOS grants permissions per
binary, so one file under six names holds the union of six jobs' permissions.
For most sibling sets -- the `useradd` family all edit the same two files --
that union is what each would be granted anyway. Where the jobs genuinely
differ (`systemctl`'s fourteen), a name earns its own crate.

**What follows:** the triage, name by name, in `known-issues.md` ->
`TD-B-ONE-HUNDRED-AND-SEVENTY-TWO-COMMAND-NAMES-NOBODY-CAN-RUN`; the ledger
`scripts/multicall-aliases-baseline.txt` shrinks as each is settled.
Installing a file under extra names is the rootfs recipe's job (lane D), and
is requested from them.
