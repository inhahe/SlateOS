## 843. The shell's *binary* starts programs; the shell's *library* still only asks

**Date:** 2026-09-13. **Lane:** C. **Decided by:** Claude (autonomous).

**In short:** the desktop can now start a program from the start menu, and the
`Command::spawn` that does it lives in `gui/desktop`'s binary rather than
anywhere in the library. `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH` says the
intent should stay a value rather than become a spawned `Command` in the window
manager, and that rule is kept: nothing in `DesktopShell` or `ShellSession`
starts anything. What changed is that the outermost layer -- the process that
*is* the shell, as opposed to the library that draws it -- now acts on the
queue instead of printing it.

**The tension, stated plainly, because it is real.** That entry's reason is
sound: policy about *how* a program starts -- namespace, capabilities, cgroup,
environment -- belongs to the service that owns process creation. A window
manager that spawns is a window manager making those decisions by omission.
Against that: until 2026-09-13 nothing executed a launch at all, and the cost
of that was not theoretical. `design-decisions.md` 818 -- an operator decision
that a passwordless account is never locked -- could not be implemented,
because there was no lock path to put a condition on. A queue nobody drains
makes every feature downstream of it untestable and unfinishable.

**Why the binary is the right place for it rather than a violation of the
rule.** The boundary the entry is protecting is *library/policy*, and the
binary is already outside it. On SlateOS the same function becomes a channel
send to the process server; the swap is local to `drain()` in `main.rs`, and
every caller above it -- the start menu, the run box, the hotkeys, the session
-- is unchanged because each still only records an intent. Putting the spawn in
`ShellSession` would have made that swap a refactor of the library instead.

**What it does not claim.** `Command::spawn` inherits this process's
environment and privileges, which is exactly the policy-by-omission the entry
warns about. That is acceptable on a development host and is **not** acceptable
on SlateOS, where it must become the channel send. The entry stays open for
that reason, with its "proper fix" unchanged.

**A failed spawn is reported, not swallowed.** The start menu names programs by
absolute path and on a development host most of those paths do not exist.
"cannot start /usr/bin/settings: not found" is actionable; silence is a desktop
that appears to do nothing when clicked.

**The alternative that was rejected.** Leave launches inert until a process
server exists. It keeps the boundary perfectly clean and it is what the tree
did for weeks; the cost is that the shell remains a program that cannot start
programs, 818 stays unimplementable, and the first real test of the whole
launch path waits on another lane's service.
