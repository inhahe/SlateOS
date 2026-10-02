## 1049. sbctl keeps what can work or is one planned change away; the four signing commands are deleted

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q17 with option 2, which Claude
recommended) for `sbctl`. Applying the same rule to the other commands the
question listed is Claude's (operator-approved scope). Relayed verbatim
through lane F's session.

**In short:** `sbctl` manages Secure Boot keys. It used to report creating
keys and signing kernels while writing nothing; since 2026-09-15 those
commands refuse instead. Now the four that need cryptography this project
does not have and has not planned -- `create-keys`, `sign`, `rotate-keys`,
`bundle` -- are deleted, per §1006. `enroll-keys` and `reset` stay, refusing,
because they wait only on a door into the kernel's key store that lane A has
been asked for and has scheduled (A-Q21, §978 on lane A's branch).

**The rule, as it applies beyond sbctl.** A command that does not work stays
only while what it waits for is planned; otherwise it is deleted and added
back when it is implemented (§1006). The other refusing commands the question
named (`unshare`, `nsenter`, `dbus-daemon`, `dbus-send`, `dbus-monitor`, `lp`,
`lprm`, `eject`) are judged by that rule one at a time, each against the
roadmap, in the commit that settles it.
