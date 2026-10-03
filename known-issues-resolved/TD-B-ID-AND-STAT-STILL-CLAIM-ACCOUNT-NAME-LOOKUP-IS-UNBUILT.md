### TD-B-ID-AND-STAT-STILL-CLAIM-ACCOUNT-NAME-LOOKUP-IS-UNBUILT -- ✅ FIXED 2026-08-23 (`id` and `stat` both done), user-visible (lane B, 2026-08-23)

**What.** `id -n` and `stat`'s `%U`/`%G` print numbers where every other system
prints names, and each says in a comment that the lookup is not available yet:

    userspace/coreutils/src/bin/id.rs:7    -n  print name instead of number (not yet supported -- prints number)
    userspace/coreutils/src/bin/stat.rs:319 %U/%G are the *names*, which need /etc/users.yaml (§353). Until ...

**Why it is wrong.** The lookup has existed for some time. `userspace/pwdb`
reads `/etc/passwd` and `/etc/group`, is already a declared dependency of the
coreutils crate, and `ls -l` has been resolving both the owner and the group
column through it. The comments are describing a world that stopped being true
and were never revisited, so the capability is present and simply not called.

**Found by.** Converting `chown` off `Vec<String>` argv. `chown` carried the
same stale claim -- "name lookup not yet supported (see design-decisions.md
§353 on /etc/users.yaml)" -- and returned `invalid user: 'alice'` on a system
that knew alice, which is to say it rejected the spelling that essentially
every script and every manual page uses. Fixed there in the same commit, via
gnulib's `parse_user_spec`. Two bins were left holding the same belief.

**Why it matters more than it looks.** `id -un` is the ordinary way a script
asks who it is running as, and a numeric answer is not merely uglier -- it
compares unequal against the name the script is testing for. Same for a `stat
-c %U` in a permission check.

**Proper fix.** Call `pwdb::Db` the way `ls` and now `chown` do: one `Db::load()`
per run, `user_by_uid` / `group_by_gid`, and fall back to the digits when the
database does not know the id (that fallback is what `chown-core.c`'s
`uid_to_name` does, and it is why a `-v` line always has something to print).
Delete the comments rather than editing them; there is nothing left to qualify.

**When.** Both bins are already in the `scripts/argv-utf8.py` backlog
(`id.rs:65`, `stat.rs:657`), so both will be opened for the argv conversion
anyway. Fix it then -- the conversions keep turning up exactly this shape of
defect, and that is the argument for doing them rather than the cost of them.

**Update 2026-08-23 -- the `id` half is fixed; `stat` is still open.** `id` was
rewritten as a port of GNU's `src/id.c` + `src/group-list.c` + gnulib's
`lib/mgetgroups.c` during its argv conversion, and `-n` now resolves through
`pwdb::Db` exactly as this entry prescribed. Opening the file turned up three
further defects beyond the one recorded here, all user-visible, and all now
fixed in the same rewrite:

* **A USER operand was silently discarded.** `parse_args` inspected only
  arguments beginning with `-`, so `id alice` printed the *caller's* ids and
  exited 0. A wrong answer with a success status is worse than a failure.
* **The default line had its fields in the wrong order** -- `uid= euid= gid=
  egid=` where every other `id` emits `uid= gid= euid= egid=`. Two unit tests
  asserted the wrong order, which is how it survived.
* **There was no `groups=` field at all**, which is most of what the default
  line is for. It now comes from a new `pwdb::Db::group_list` (glibc's
  `getgrouplist`, measured against glibc 2.39 and unit-tested against that
  transcript).

One honest limitation ships with it, documented in the module docs rather than
papered over: `id` with **no operand** prints only the effective gid in
`groups=`, because that list is the kernel's (`getgroups(2)`), and
`posix/src/unistd.rs`'s `getgroups` still returns 0 supplementary groups.
`id $USER` reads `/etc/group` and prints everything. Making the no-operand case
read the file too would have `id` vouch for privileges the kernel has not
granted; gnulib takes the same view, synthesising a list only when `getgroups`
fails with `ENOSYS`. The real fix is in `getgroups`, not in `id`.

`stat`'s `%U`/`%G` are untouched and this entry stays open for them.

**Update 2026-08-23 (later) -- the `stat` half is fixed; the entry is closed.**
`stat` was rewritten against GNU 9.4's `src/stat.c` during its own argv
conversion, and `%U`/`%G` now resolve through `pwdb::Db` — loaded **lazily**,
only for a format that actually contains one of the two, so `stat -c %s` over a
thousand files still never opens `/etc/passwd`. An id the database does not
know prints `UNKNOWN`, which is upstream's word and the one a script greps for,
rather than the digits: `%u` and `%g` are right there for the number, so
falling back to it in the *name* field would make the two columns
indistinguishable. The stale `/etc/users.yaml` (§353) comment is gone rather
than edited, as this entry prescribed. Eleven further defects came out of that
file with it — see `B-stat-HAS-NO-OPTIONS-AND-CANNOT-READ-A-CLOCK` above.
