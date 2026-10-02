## `A-GROUPMGR-DELETE-HAS-NO-GUARD-AND-GROUPTYPE-SYSTEM-PROTECTS-NOTHING` (lane A, 2026-08-26) — ✅ **FIXED** 2026-08-30

**In short:** `groupmgr delete 0` destroyed the `root` group and printed
`Deleted group 0.` as a success. There is no confirmation, no privilege check,
and no protection for system groups — even though the code carries a
`GroupType::System` label that looks like exactly that protection. The label is
never read by anything.

**Where.** `kernel/src/fs/groupmgr.rs`.

- `init_defaults` seeds GID 0 as `root` and GID 1 as `wheel`, both
  `group_type: GroupType::System` (~124–134).
- `delete_group` is four lines: `state.groups.retain(|g| g.gid != gid)`, then
  `NotFound` if nothing was removed, else `Ok(())`. It does not look at
  `group_type`, at membership, or at the caller.
- A tree-wide grep for `GroupType::System` returns **three** hits: the two
  constructions above and one assertion inside the module's own self-test.
  Nothing anywhere *branches* on it. The variant is a display label
  (`group_type.label()` in the shell listing) wearing the name of a policy.

**Why this is worth an entry.** The danger is not that a destructive operation
exists — it is that the codebase reads as though the case were already handled.
A reviewer who greps for `System` finds the variant, sees it set on `root`, and
reasonably concludes that system groups are distinguished from user groups for
a reason. They are not. The same is true of the shell: `groupmgr` lists a
`[System]` tag beside `root`, which tells the operator the system knows this
group is special, at a prompt where deleting it is one word.

**The proper fix.** Decide what `GroupType::System` is *for* and then make it
load-bearing, rather than adding a special case for GID 0 specifically. The
narrow version is `delete_group` returning `KernelError::PermissionDenied` (or
`InvalidArgument`) for any `GroupType::System` group; the fuller version also
refuses to remove the last member of a system group and refuses `remove_member`
on `root`. Deleting by GID-0-literal is the wrong shape of guard — it protects
one id rather than the property that made that id worth protecting, and `wheel`
at GID 1 is just as load-bearing.

**Fixed (2026-08-30).** `GroupType::System` now *is* the policy, and it took two
rules rather than the one the paragraph above asked for.

- `delete_group` refuses any `System` group with `PermissionDenied`. It also
  had to change shape: the old body was `retain` first, length-comparison
  second, which cannot consult the entry it has already dropped. It now finds
  the group, decides, and only then removes — so there is no window in which
  `root` is deleted and put back.
- `create_group` refuses to *mint* a `System` group, likewise
  `PermissionDenied`.

**Why the second rule was not optional, and was not in the original writeup.**
The first rule alone converts one defect into another. If `System` groups are
undeletable and anything may create one, then `groupmgr create 1000 x system`
mints an immortal group, and 252 of them exhaust `MAX_GROUPS` permanently with
no way to recover short of a reboot. An undeletable object that untrusted input
may create is a resource leak with a security label on it. So the two rules are
one decision — *the system's own identity set is fixed at startup* — and the
entry's "narrow version" would have shipped a slot-exhaustion bug in exchange
for the deletion bug.

The general form is worth keeping: **whenever you make a class of object
undeletable, ask in the same breath who may create one.** Immutability granted
at the destroying end and withheld at the creating end is not a protection, it
is a lever.

**What did not change.** The entry's "fuller version" also proposed refusing to
remove the last member of a system group, and refusing `remove_member` on
`root`. Neither was implemented, and that is deliberate rather than deferred:
`init_defaults` seeds every group with an **empty** member list on purpose (it
refuses to fabricate memberships it has not observed — see the doc comment).
An empty `root` is therefore the normal state at boot, so "the last member" is a
rule about a condition this implementation does not have, and a guard on it
would fire on exactly nothing. It becomes a real question once membership is
wired to `useracct`, which is already recorded there as the deferred proper fix.

**Shell.** `cmd_groupmgr` renders the new `PermissionDenied` specifically rather
than as `Error: PermissionDenied` — `groupmgr: delete: group 0 is a system
group`, and `groupmgr: create: system groups are defined at startup and cannot
be created here`. `system` is still accepted by the type parser and still listed
in the usage line: refusing the *word* would leave the operator guessing which
of the three types they may use and why, where refusing the *request* tells them.

**Tests.** `groupmgr::self_test` grew to 10 cases — 8 covers deletion (both
`root` at GID 0 and `wheel` at GID 1, so a guard written against the literal `0`
fails it; plus a `User` group at GID 100 that *is* deletable, so the guard is
shown to read the type and not to have frozen the table), 9 covers creation
(refused as `System`, accepted as `Service` at the same GID and name, then
deleted). kshell rung 108 asserts the same four outcomes through the shell,
each as a paired assertion — the message, the exit status, and the absence of
the success line — because a refusal that prints an error and acts anyway
satisfies the first of those alone.

**Relationship to the D1 burn-down.** These are two different bugs that
compound, and fixing one does not fix the other. Before the twenty-first batch,
`cmd_groupmgr`'s `delete` arm guessed GID **0** for any word it could not parse
— so `groupmgr delete 1O` deleted the root group. That guess is now refused, so
reaching this operation required *typing* `0`; the operation itself was still
unguarded, and a correctly-typed `groupmgr delete 0` still destroyed `root`.
Both halves are now closed. The pairing is the point: the burn-down makes a
destructive operation *hard to reach by accident*, and says nothing whatever
about whether it should succeed when reached deliberately. Neither fix would
have been sufficient alone, and the burn-down's refusal message would have made
the remaining hole look handled. See
`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`.

**Not a regression.** True since `delete_group` was written.
