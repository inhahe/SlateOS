### [A] Correction: `capsettings` is not a path-keyed table, it is a path-PATTERN policy -- 2026-09-21
**Status:** CORRECTION to my own entries above. `capsettings` must NOT be converted to `FileId` keying; doing so would have been a wrong fix I was one step from making.

**In short:** I listed five kernel tables that remember a file by name and
should remember it by identity instead. One of them does not belong on that
list. It does not store files at all -- it stores rules about *where* files
are, and a rule like *everything under /etc* has no file to identify.

**What it actually does.** `PathRequirement.path` is a pattern, and
`matching_requirements` has three branches:

| branch | match |
|---|---|
| `r.path.ends_with('*')` | prefix match on the stem |
| `r.recursive` | prefix match, whole subtree |
| otherwise | exact string compare |

The first two cannot be inode-keyed even in principle: a prefix describes a
set that does not exist yet. Converting this table to `FileId` would have
required either dropping the wildcard and recursive forms, or a hybrid where
some rows key on an inode and some on a string -- which is worse than either,
because a reader could no longer tell which a given row does.

**This is a design difference, not a defect.** Path-pattern access control is
a real family -- it is what AppArmor does, and it is path-based on purpose.
Inode-and-label schemes like SELinux are the other family. This module is the
first kind, and my entries treated it as a broken instance of the second.

**What remains true, narrowly.** The third branch, exact compare, does have
the hard-link gap: a rule naming one path is not consulted for a second name
for the same file. That is worth knowing and is NOT worth fixing by keying
the table, because the fix would break the two branches that matter more. If
it is ever worth closing, the shape is a separate exact-match-by-identity
list, not a change to this one.

**How I nearly got it wrong.** I classified five tables by grepping for a
`path: String` field and a comparison against it. `capsettings` has both. It
took reading fifteen lines of `matching_requirements` to see that the
comparison is one of three branches and the other two are prefix matches.
The grep was accurate and the conclusion it supported was not -- the seventh
time today a textual signal stood in for reading the code.
