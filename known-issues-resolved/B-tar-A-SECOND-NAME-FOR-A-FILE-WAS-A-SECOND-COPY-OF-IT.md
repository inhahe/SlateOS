## B-tar-A-SECOND-NAME-FOR-A-FILE-WAS-A-SECOND-COPY-OF-IT (lane B, 2026-08-29) -- **FIXED 2026-08-29**

**In short:** when `tar` is asked to archive the same file twice in one run --
`tar -cf backup.tar dir dir/sub`, or just `tar -cf x f f` -- GNU stores the
bytes once and records the second name as a *hard link* to the first. Ours
stored the whole file again, so an archive of a tree named twice was twice the
size it should have been. Worse, ours applied the same idea to file types GNU
excludes, and applied it *before* writing the member rather than after: two
names for a **socket** produced an archive containing a hard link pointing at a
member that was never written, which no extractor can unpack.

Discovered while measuring `-cvv` for the verbosity work: GNU printed a
`Removing leading '/' from hard link targets` notice where ours printed none,
for a command line with no link in it at all. Pulling that thread
(`tar-hardnotice1.sh` .. `tar-hardnotice6.sh`) found the rule underneath it.

### The rule, measured

GNU keys a table on `(device, inode)`. Every member it *successfully writes*
that is a **regular file or a symlink** goes in; a later member with the same
key is written as typeflag `1` naming the first. Three parts of that, and ours
had all three wrong:

| | GNU | Ours (before) | Consequence |
|---|---|---|---|
| link count | irrelevant -- an `nlink=1` file named twice is deduplicated | required `nlink > 1` | `tar -cf b.tar dir dir` wrote the tree twice |
| member type | regular files and symlinks only | every non-directory | a fifo named twice became a link record; GNU writes two fifos |
| when | after the member is in the archive | before it was attempted | two names for a socket gave a link to a member that does not exist |

`--hard-dereference` turns the mechanism off entirely. Ours has no such option
yet, so there is nothing to gate on -- but if one is added, that is where it
goes.

A fourth, smaller divergence came out of the same probes: GNU strips a member's
name (and so prints the prefix notice) *before* it opens the file, so
`tar -cf x /abs/unreadable` says `Removing leading '/' from member names` and
then `Cannot open`. Ours built the whole header after the open and printed
nothing but the error. `add_regular` now calls `stored_name` before `File::open`
and passes the result to a split-out `header_for`.

### Still open: one notice that no model explains

`tar -cf x /abs/unreadable /abs/readable` -- GNU prints a
`Removing leading '/' from hard link targets` notice; ours prints none. It is
stderr only, on a command line where one member fails, and it does not affect
any archive. What makes it interesting is that no rule fits the whole table:

| argv (all absolute, all `nlink=1`) | GNU prints the link-target notice? |
|---|---|
| one readable member | no |
| one unreadable member | no |
| two readable members, different inodes | **yes** |
| the same readable member twice | **yes** |
| unreadable then readable | **yes** |
| readable then unreadable | **yes** |
| the same unreadable member twice | no |

"Fires once a name is entered in the link table" predicts a notice for a single
readable member. "Fires when a link record is written" predicts none for two
different inodes. "Fires on a lookup against a non-empty table" predicts none
for *unreadable then readable*, since the failed first member is not entered.
Every model contradicts one row. It is plausibly GNU's separate
`--check-links` accounting table rather than the dedup table, but that was not
run to ground.

Ours matches every row above except the two that mix a failure with a success.
Left as-is deliberately: guessing at a rule that does not fit the measurements
would be worse than a known, documented gap. The differential's link-table
cases use *relative* names so that nothing strips and this question cannot
contaminate them; the notice-ordering case uses a single unreadable member,
which is a row we do match.
