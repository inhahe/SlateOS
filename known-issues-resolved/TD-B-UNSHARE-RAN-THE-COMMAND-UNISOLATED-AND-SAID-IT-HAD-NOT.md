## TD-B-UNSHARE-RAN-THE-COMMAND-UNISOLATED-AND-SAID-IT-HAD-NOT — 2026-09-15 — FIXED by refusing

**In short:** `unshare` never called `unshare(2)`. It printed *"unshare:
mapping current user to root in user namespace"* — **present tense, about
something it had not done** — and then ran the command with no namespaces
created at all.

**The sibling of `TD-B-NSENTER-RAN-THE-COMMAND-IN-THE-WRONG-NAMESPACE`, found
by grepping the shape rather than waiting for it.** `nsenter` was fixed first;
one `grep` for `real implementation|simulat|Command::new` over the neighbouring
crate found this within a minute. Two tools, one defect, and the second would
not have been looked at on its own — it was not on the advertised-but-unread
ranking, because its fields *are* read. They are read by the simulation.

**Worse than `nsenter` in one respect.** `nsenter` said nothing and ran the
command elsewhere; this one **asserted the action** before running the command
without it. A false statement in the present tense is not a stale comment — it
is output, and the user read it as confirmation.

**Running unisolated is the harm, not a lesser version of it.** `unshare` is
reached for exactly when an operation is risky enough to want containing:
`unshare -m -- <mount juggling>` expects private mounts, and without a new
mount namespace they are the host's. The command runs, succeeds, and changes
the wrong system.

**Refused, not wired — checked rather than assumed.** `posix::unshare`
validates its flag set and its `CAP_SYS_ADMIN` gate and then returns `ENOSYS`.
Its `unshare(0) -> 0` is **not** an exception and was examined before being
dismissed: Linux defines the zero-flag call as a successful no-op, and
util-linux uses it to probe for the syscall's existence.

The execution path is deleted rather than guarded — `Command::new` no longer
appears in the file. The module doc and `--help` say what happens now.

**Ends the day a namespace subsystem lands**, at which point both this and the
`nsenter` refusal become real `unshare(2)`/`setns(2)` calls.

**Where it lives:** `userspace/unshare/src/main.rs`.
