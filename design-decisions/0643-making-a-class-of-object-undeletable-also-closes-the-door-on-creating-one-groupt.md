## 643. Making a class of object undeletable also closes the door on creating one: `GroupType::System` is reserved at both ends

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane A
**Lane:** A

**In short:** The group table has a `System` label on `root` and `wheel` that
nothing read — you could delete the root group and the shell would say
`Deleted group 0.` Making the label mean something required deciding *what* it
means. The choice made here is "the identities the system is defined in terms
of, fixed at startup": `System` groups cannot be deleted, **and** cannot be
created. The second half is not an extra precaution; without it the first half
is a resource-exhaustion bug.

### The decision

`kernel/src/fs/groupmgr.rs`:

- `delete_group(gid)` returns `PermissionDenied` for a `GroupType::System`
  group.
- `create_group(.., GroupType::System, ..)` returns `PermissionDenied`. The
  `System` set is exactly what `init_defaults` seeds — `root` and `wheel` — and
  is closed for the life of the boot.

### Alternatives, and why they lose

**(a) Refuse deletion of GID 0.** The obvious fix, and the wrong shape. It
protects one identifier rather than the property that made that identifier worth
protecting. `wheel` at GID 1 is just as load-bearing and would be unprotected,
and any future system group would arrive unprotected by default — the guard does
not generalise because it never described anything.

**(b) Refuse deletion of `System` groups, leave creation open.** This is what
the original writeup proposed, and it trades one defect for another.
`MAX_GROUPS` is 256; if anything may create an undeletable group, then 252
`groupmgr create <n> <name> system` invocations exhaust the table permanently,
with no recovery short of a reboot. *An undeletable object that untrusted input
may create is a resource leak wearing a security label.* The general rule worth
carrying: whenever you make a class of object undeletable, decide in the same
breath who may create one. Immutability granted at the destroying end and
withheld at the creating end is not a protection, it is a lever.

**(c) Keep `System` a free label and guard on a separate `builtin` flag set only
by `init_defaults`.** This works, and it is arguably more honest about the
distinction — `GroupType` is a *classification* (what kind of group this is),
while deletability is *provenance* (who defined it). It was rejected because it
leaves the original complaint standing: `GroupType::System` would still be a
display label that reads like a policy and is not one, and a reviewer grepping
for `System` would still be misled. Two fields, one of which is decorative, is
worse than one field that means what its name says. If a legitimate need ever
arises to classify a group as system-ish *without* making it immortal, that is
the moment to split the concepts — and the split will then be motivated by a
real case rather than by symmetry.

**(d) A privilege check on the caller instead of a blanket rule.** The right
answer eventually, and not available: `groupmgr` has no notion of who is
calling. There is no credential threaded through these entry points to check.
When one exists, this rule becomes its default policy rather than its
replacement.

### The cost, stated plainly

`groupmgr create <gid> <name> system` no longer works from the kernel shell. A
future service package that wants its own system group cannot get one through
the public API and will need a privileged path — which is the same path (d)
requires, so the cost is paid once. The word `system` is still accepted by the
shell's type parser and still appears in the usage line: refusing the *word*
would leave the operator guessing which of the three types they may use and why,
where refusing the *request* tells them.

### How to reverse

Delete the two `PermissionDenied` guards. Case 8 of `groupmgr::self_test`, case
9, and kshell rung 108 pin both halves and would need to go with them. Reversing
only the creation guard reinstates alternative (b) and its slot-exhaustion bug,
so if the creation guard is ever lifted, `MAX_GROUPS` needs a separate answer
first.
