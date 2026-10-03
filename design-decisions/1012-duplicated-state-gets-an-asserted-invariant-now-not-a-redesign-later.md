## 1012. Duplicated state gets an asserted invariant now, not a redesign later

**Date:** 2026-09-10
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** which groups a user belongs to is written down in three separate
files, and nothing but careful code keeps the three copies saying the same
thing. The right long-term answer is to stop storing it three times -- but that
is a change to how accounts are stored on disk, which is the operator's call and
has been sitting unanswered. So instead of leaving the copies unguarded until
that question is answered, the code now carries a test that reads all three
after every operation and fails if they disagree.

**The state.** `/etc/group` lists each group's members. `/etc/gshadow` lists
them again. Each account record carries its own list of the groups it is in.
`Database` in `userspace/useradd/src/main.rs` -- which is `useradd`, `usermod`,
`groupadd`, `groupdel`, `groupmod` and the rest in one binary -- has five
methods that mutate membership, and each has to touch all three.

**The alternatives.**

*Do the redesign.* `design-decisions.md` §353 already made `/etc/passwd` and
`/etc/shadow` generated from a single `/etc/users.yaml`; the matching move is an
`/etc/groups.yaml` with the two group files generated from it, and one of the
two membership lists dropped. That is the proper fix and it stays the proper
fix. It is not mine to make: §353 did not decide it for groups, dropping one of
the two lists changes what is on a user's disk, and it is already raised in
`open-questions.md`. Doing it unasked would be deciding a storage format on the
operator's behalf.

*Leave it.* The four-day-old `todo.txt` entry that named this said plainly that
`Database` "now changes both together ... so nothing in *this* binary can update
one and forget the other. **Nothing enforces that for a future writer.**" That
sentence is a description of a gap, and re-reading it each tick is not closing
it. The trigger the entry set for the redesign -- a second writer, or observed
drift -- has not fired and, checked rather than assumed, *cannot* fire
observably: the account files are created at runtime, so there is no drift to
observe in the tree.

*Assert the invariant.* Chosen. `assert_membership_agrees` states what has to be
true of the three stores together, and one test runs every mutating operation
with that assertion after each.

**Why this is worth doing even though the redesign supersedes it.** The
invariant is not scaffolding for the redesign; it is the specification the
redesign has to satisfy. If `/etc/groups.yaml` lands tomorrow, this test is what
says the generated files still agree with the accounts. And it is cheap in the
way that matters: it makes no claim about how the data *should* be stored, so it
does not prejudice the decision it is standing in for.

**It found a defect in the code it was written to protect, on the first run.**
`rename_group_everywhere` renamed the group in `/etc/gshadow` and in every
account and not in `/etc/group`. Not a live bug -- its only caller, `cmd_groupmod`,
assigned `db.groups[idx].name` itself before calling it, so the pair of them did
the whole job. But the method's name promised all three, and a second caller
written from that name would have produced an `/etc/gshadow` line for a group
`/etc/group` did not have. The rename now lives entirely in the method.

**Two ways the existing test was blind, both worth carrying past this file.**
There *was* a test named `renaming_a_group_renames_it_in_the_accounts_that_are_in_it`.
It called `rename_group_everywhere` standalone -- exactly the unsafe usage --
and passed, because (1) it asserted only the account half of "everywhere", and
(2) its fixture pushed a `/etc/group` entry with no `/etc/gshadow` line, so the
one file the method silently skipped was never populated for it to skip. The
second is the subtler one: **a fixture that omits a store cannot detect an
operation that omits the same store**, and it reads as thorough either way. When
I added the missing assertion, it failed against the fixture rather than the
code, and the fix was to give the fixture its third file.

**The self-test.** An invariant nothing exercises is a decoration, and one whose
checker has quietly stopped checking is worse -- it reports the absence of a
problem it can no longer see, which is the failure mode §1011's gate hit when
its classifier stopped matching. So a companion test builds each of the three
shapes of drift deliberately and requires the checker to panic on each. If a
future edit to `assert_membership_agrees` weakens it, that test goes red rather
than the suite going quietly green.
