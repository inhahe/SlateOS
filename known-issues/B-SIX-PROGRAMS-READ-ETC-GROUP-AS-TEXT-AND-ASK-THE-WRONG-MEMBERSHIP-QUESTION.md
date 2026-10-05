## B-SIX-PROGRAMS-READ-ETC-GROUP-AS-TEXT-AND-ASK-THE-WRONG-MEMBERSHIP-QUESTION (lane B, 2026-09-12) -- `doas` FIXED, five open

**In short:** six programs parse `/etc/group` by hand instead of using the
shared reader. Two things go wrong. First, they read it with `read_to_string`,
which fails on the **whole file** if any single byte in it is not valid text --
and on this OS a group name may contain any byte but `/` and NUL, so one odd
group name switches off group handling everywhere. Second, they ask only
whether the group's *member list* names you, and that list deliberately leaves
out everyone whose **main group** it already is. So the people most obviously
in a group are the ones reported as not in it.

**Why the second half is the dangerous one.** In `doas` -- the program that
decides whether you may run a command as root -- the rule language has both
`permit :wheel` and `deny :wheel`. Answering "not a member" when someone *is*
one makes `permit` merely unhelpful, but makes **`deny` fail open**: the rule
stops applying to exactly the accounts most likely to be in the group, and the
caller falls through to whatever `permit` comes next.

That is the same inversion `read_group_entries`' own doc comment was written to
warn about after lane A found it in `mkfs` and `fsck` -- *"for a check guarding
a privileged action, 'I do not know' and 'it is safe' must not be the same
value"*. The comment guarded the error path. The bug was in the success path,
eight lines below it.

**Measured, not assumed:** `id -nG` on this machine lists the primary group
(`inhahe adm cdrom sudo dip plugdev users docker`, with `inhahe` the login
group). `pwdb::Group::members`' own doc says the fourth field omits primary
members, and `pwdb::group_list` is built to put the primary gid back.

### Where

| Program | Reads | Status |
|---|---|---|
| `userspace/doas` | `read_to_string("/etc/group")`, members only | **FIXED 2026-09-12** |
| `userspace/getent` | `read_to_string` x7, and `parse().unwrap_or(0)` | **passwd/group FIXED 2026-09-12**; its five other databases still read as text |
| `userspace/install` | `read_to_string` on both files, **and tried the number before the name** | **FIXED 2026-09-12** |
| `userspace/loginctl` | `read_to_string(GROUP_FILE)` | **unreachable** -- its `userdbctl` personality |
| `userspace/mktemp` | `read_to_string` | **unreachable** -- its `id`/`groups` personalities |
| `userspace/newgrp` | `read_to_string(...).unwrap_or_default()` | **FIXED 2026-09-12** |

**Two of the six cannot be run at all, which the first version of this entry
did not say.** `scripts/multicall-aliases.py` reports `loginctl`'s `userdbctl`
and `mktemp`'s `id`/`groups` as unreachable personalities -- 168 of them across
67 crates, produced by NOTHING, which is the subject of the open question
"169 command names exist inside other programs and cannot be run. Which ones do
we keep?". Their group parsing is dead code whose fate the operator decides, so
fixing it would be work on code two of the three answers delete. Counting them
as "open" alongside three live programs overstated the remaining work by
two-thirds, and the count came from grepping for the defect without asking
whether the code runs.

So the class was **four live programs, all four now fixed**. `newgrp` was the
worst of them: `unwrap_or_default()` turned an unreadable *or* non-text group
file into an **empty group table**, the unreadable-means-empty conflation in
its purest form -- so one group whose name is not text reported that every
group does not exist.

### The fix, as applied to `doas`

`pwdb` already does this correctly and has no dependencies, so it is exempt
under §768 the way `quoting` and `utmpfile` are. The two files are read as
**bytes** by the caller rather than through `pwdb::Db::from_files`, because
that helper does `unwrap_or_default()` and would reintroduce the very
conflation being removed. Membership is then `group_list(name, primary_gid)`,
which is what `id -G` prints.

The lookup was also split into a pure `user_in_group_in(passwd, group, user,
group_name)` taking bytes, because none of this was testable before: the build
host has neither file, so every existing test either skipped itself or
exercised the "cannot read" arm. Six tests now cover supplementary membership,
**primary** membership, a non-member, an absent group, an absent caller, and a
group name that is not UTF-8 -- each asserting its own fixture really has the
property it is named for, so none can pass vacuously.

### `getent` had a second defect the others do not

`uid: fields[2].parse().unwrap_or(0)` -- and the same for gid. A
`/etc/passwd` line whose uid field is not a number was reported as **uid 0**,
which is root. glibc's `fgetpwent` rejects such a line and so does `pwdb`, so
moving onto it fixed this at the same time. There is a test.

Its non-UTF-8 case is also the most likely to be hit in practice: a person's
name in the GECOS field, written in Latin-1, is the ordinary way a byte that is
not valid UTF-8 gets into `/etc/passwd`. The arm was `Err(_) => Vec::new()`, so
one such account made `getent passwd alice` answer **"no such user"**, exit 2,
for every account on the system -- a wrong answer rather than an error.

**Five of `getent`'s seven databases are still read as text** -- `hosts`,
`services`, `protocols`, `networks`, `shadow`. `pwdb` does not cover those
formats, so they need their own byte parsing rather than a shared reader, and
that is a separate change.

### `install` had a third defect, and it is the one POSIX names

`-o`/`-g` did `name.parse::<u32>()` FIRST and returned on success. GNU's manual,
*Disambiguating names and IDs*:

> POSIX requires that these commands first attempt to resolve the specified
> string as a name, and only once that fails, then try to interpret it as an
> ID. ... Simply invoking `chown 42 F` will set F's owner ID to 1000 -- not
> what you intended.

So on a system with an account named `1000` -- "using a number as a user name
is common in some environments", says the same page -- `install -o 1000` and
`chown 1000` picked **different accounts**. The `+` escape exists precisely
because the name comes first; a number-first parser has no use for it, and
this one ignored `+` entirely.

Fixed by sharing `chown`'s and `id`'s resolver rather than repairing the
private one. That meant **extracting `coreutils::userspec` into its own crate**
(`userspace/userspec`), which was clean: the module imported exactly one thing,
`pwdb::Db`, and nothing from `crate::`. `coreutils` re-exports it, so
`coreutils::userspec::…` still names it and no bin changed -- the same move,
for the same reason, as `quote`.

The tests could not previously tell the two orders apart: the build host has no
`/etc/passwd`, so every existing case exercised the numeric fallback. The
resolvers now take the database as a parameter, and the fixture is a database
in which `1000` is an account *name* whose uid is 7.

### What is deliberately NOT changed

`None` still means "no answer" and is still distinct from "no members". The
three-way result is now: absent group is `Some(false)` (nobody is in a group
that does not exist -- an answer, not an absence), absent caller is `None`
(their primary gid is unknowable and primary membership counts), otherwise the
list is consulted.
