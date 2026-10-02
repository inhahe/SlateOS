## TD-B-USER-SWITCHING-PROGRAMS-CANNOT-RESET-SUPPLEMENTARY-GROUPS (lane B, 2026-09-10)

**Status: HALF FIXED 2026-09-12, and the half that is left is a different
piece of work.** `authlib::identity::become_user` now DROPS the supplementary
groups before changing gid and uid, so the caller's groups no longer follow a
user switch into the new session. That covers `su`, `doas`, `sudo`, `login`
and `sshd` at once, which is why the call lives in `authlib`.

Found by `scripts/check-stale-blockers.py`, written the same day after this
shape cost three separate waits: the request this entry names has said
`LANDED` since 2026-09-07.

**Two things had to be got right, and the obvious implementation gets both
wrong.**

1. *The order is groups, then gid, then uid*, because each step sheds the
   privilege the next one needs. `setgroups` after `setuid` fails with EPERM.
2. *It cannot be a `pre_exec` closure added beside the existing `cmd.uid`/
   `cmd.gid` calls.* `std` applies those in the child and runs `pre_exec`
   closures **afterwards**, so a `setgroups` added that way runs with the
   privilege already gone, fails, and aborts the child — turning a silent leak
   into a program that cannot start a shell. All three calls therefore happen
   inside one closure, and `Command::uid`/`gid` are deliberately unused.

**The remaining half: there is no name-to-gid resolver.** The correct fix sets
the *target's* groups, not none. `userdb::Record::groups` returns
`Vec<String>` — names — and nothing in this tree maps a group name to a gid,
so the list the syscall takes cannot be built. Dropping is the safe direction
(too few groups means work refused that should have been allowed; too many
means authority the user never had), but a user who belongs to `wheel` will
not have it after `su - them` once memberships start being tracked. **The
trigger for finishing this is a group database**: `/etc/group` parsing, or
`userdb` growing a gid alongside each name.

**A note on how this was verified, because the host build cannot see it.**
The code is inside `#[cfg(unix)]`, and the dev host is Windows, so
`cargo build --target x86_64-pc-windows-gnu` compiles none of it and a green
host build proves nothing. Checked with
`cargo check -p authlib --target x86_64-unknown-linux-gnu`, and then checked
again by introducing a deliberate typo into the block and confirming the
compiler reported it — because "it compiled" and "it was skipped" otherwise
look identical. That hazard is its own entry:
`B-DEV-HOST-IS-WINDOWS-SO-CFG-UNIX-CODE-IS-NEVER-COMPILED`.

The description below is kept in the tense it was written in.

**In short:** when a program switches to another user, it can now change that
user's main identity for real, but it cannot clear the *extra* group
memberships the original user had. Right now nobody has any extra groups, so
nothing leaks. The moment the kernel starts tracking them, every user switch
carries the old user's extra groups into the new session.

**Where.** `userspace/su/src/main.rs`, `exec_as_user`. The same will apply to
`doas`, `sudo`, `login` and `sshd` as each gains its privilege drop.

**Why it is not simply done.** `posix::setgroups` returns `ENOSYS`
deliberately -- the kernel implements it only in the Linux-ABI table
(`kernel/src/syscall/linux.rs`) and `posix/src/syscall.rs` has no native number
for native libc to call. Filed as
`requests/b-a-no-syscall-sets-supplementary-groups-changes-root-or-changes-directory.md`.
`std`'s `CommandExt::groups` makes the child call `setgroups` between fork and
exec; on `ENOSYS` the child aborts and never execs, so requesting it today does
not leave `su` half-working, it leaves `su` not working.

**Why `ENOSYS` is the right answer and not a bug.** `posix::setgroups`'s own
doc comment argues it: a stub returning 0 would let privilege-dropping code
ship believing it had dropped something. That reasoning is why this entry
exists at all -- the failure is visible instead of silent.

**The shape of the leak.** A process that keeps the caller's supplementary
groups and lowers only its uid still holds every group the caller was in. That
is the textbook version, and it is what this code does. It is *empty today*:
`posix::getgroups` reports zero groups, so there is nothing to retain. That is
a fact about the current kernel, not a guarantee, and it is precisely the kind
of fact that stops being true without anyone revisiting the code that depends
on it.

**The fix, when the syscall lands.** Set the target's groups in the child
before `setgid`/`setuid` -- `CommandExt::groups` if it is stable by then,
otherwise a `pre_exec` closure calling `setgroups` directly. The ordering is
not optional: groups, then gid, then uid, because each step drops the privilege
the previous one needed.
