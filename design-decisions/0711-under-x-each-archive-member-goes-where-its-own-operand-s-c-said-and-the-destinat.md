## 711. Under `-x`, each archive member goes where *its own* operand's `-C` said — and the destinations are held open as descriptors rather than re-entered by path

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane B
**Lane:** B

**In short:** `tar -C somewhere ...` means "do the rest of this command from
inside `somewhere`". You may write several, and they stack: `-C a -C b` means
`a/b`. When you also name which members to unpack, the two lists interleave —
`tar -xf a.tar -C d1 one.txt -C d2 two.txt` puts `one.txt` in `d1` and
`two.txt` in `d1/d2`. Our tar used to fold all the `-C`s into a single
destination and put everything there, which silently unpacked files into the
wrong directory. The fix has to be able to go *back up* — the archive may hold
`two.txt` before `one.txt`, so tar can be inside `d1/d2` when it meets a member
that belongs in `d1`. This entry records how "back up" is done, and what happens
when two operands both claim the same member.

**Date measured against:** GNU tar 1.35, x86_64 Debian, `LC_ALL=C.UTF-8`.

### The decision, part one: held descriptors, not a second chdir

A process has exactly one working directory. Having walked `d1` → `d1/d2`, the
obvious way back to `d1` is another `chdir` — either `chdir("..")` or
`chdir(<the path we were given>)`. Both were rejected.

*What is wrong with `chdir("..")`:* it is a path resolution, and the thing it
resolves is a directory an untrusted archive is at that moment writing into. If
`d1/d2` is renamed or replaced between the two members, `..` is whatever the new
parent is. `tar` already refuses to walk through a symlink it did not create
(`Dir::locate`); re-resolving the destination itself would put the hole back one
level up.

*What is wrong with re-`chdir`ing the original path:* the same, plus it makes
the cost of a destination proportional to how often members alternate between
them.

*What was chosen:* `struct Roots` keeps one open `Dir` per `-C` level. The
working directory only ever walks **forward**, one `-C` at a time, and every
level it passes through is captured on the way. Going "back up" is then just
indexing a `Vec`, and every destination keeps the property the single root
already had: a directory renamed mid-extraction carries the extraction with it
instead of spilling into whatever took the old name.

*What it costs:* one open descriptor per `-C` on the command line, held for the
length of the run. That is bounded by argv, not by the archive — an archive with
a million members still costs one descriptor per `-C` — so it cannot be made
large by a hostile archive, only by a hostile command line, which the user wrote.
The alternative's cost is a re-resolution per member, which *is* archive-driven.

### The decision, part two: entering is lazy, and a `-C` nobody follows is dead

Measured, and copied: a `-C` is entered when a member belonging to it is met, not
up front.

| Command | GNU |
|---|---|
| `-C d1 a.txt -C d2 z.txt`, `d1/d2` absent | extracts `a.txt` into `d1`, *then* `d2: Cannot open`, exit 2 |
| `-C d1 a.txt -C nosuchdir not-in-archive` | `not-in-archive: Not found in archive` — `nosuchdir` never mentioned, never entered |
| `-C d1 a.txt -C nosuchdir` (nothing after) | exit **0**; the trailing `-C` is never reached |

The last row is the one that surprises: under `-c` a trailing `-C` is executed
*and* draws GNU's positional-options refusal (§ the trailing-`-C` block in
`do_create`), while under `-x`/`-t` it is silently dead. `-C` is a *step in a
sequence* when creating and a *destination for what follows* when extracting,
and the two readings genuinely differ. Both are reproduced as measured rather
than unified, because unifying them would mean changing observable behaviour in
whichever direction was picked.

One consequence is that a failure to enter is fatal *where it is met*: GNU prints
`Error is not recoverable: exiting now` and exits on the spot, so nothing that
would normally round off a run happens — not the delayed symlinks, not the
deferred directory stamps, and not the `Not found in archive` notices for
operands the archive had not reached yet. `do_extract` and `do_list_main` both
return early on a handler-initiated stop for exactly this reason.

Listing is lazy in the same way, which is visible in the *ordering*:
`tar -tf a.tar -C d1 one -C nosuchdir two` prints `one` before it reports
`nosuchdir`. A listing writes nothing to the filesystem, so it would have been
easy to argue the chdir does not matter and skip it — that argument is what left
`tar -tf a.tar -C nosuchdir` exiting 0 until 2026-08-30. The buffered stdout is
flushed before the diagnostic so the two streams interleave as GNU's do.

### The decision, part three: overlapping operands take the *first* one's level

`-C d1 t -C d2 t/a.txt` names one member twice, at two levels. There is no
correct answer to copy, because GNU's is incoherent: it walks its operand list
with a cursor rather than testing every operand, and measured on 1.35 that line
extracts everything into `d1` while simultaneously complaining
`t/sub/b.bin: Cannot open: File exists` and `t/a.txt: Not found in archive` —
it reports a member as absent from an archive it has just unpacked.

*Alternatives:* (a) reproduce the cursor, (b) extract the member once per
matching operand, (c) first match wins.

(a) was rejected because it means giving up the operand model this tar uses
everywhere else — every other name-matching path here tests all operands — to
gain bug-compatibility with a message that is self-contradictory. (b) was
rejected because it writes the same bytes twice and the second write has to
decide what to do about the first, which is the overwrite question with no user
input to answer it. (c) is what shipped: `Selector::wants` marks *every* matching
operand (so `Not found in archive` stays right) but returns the **first**
match's level, so the member is extracted once, into the destination the user
wrote first.

*What it costs:* one measured GNU line we deliberately do not reproduce, and it
is not in the differential harness as an xfail because the case is a command
line no one writes on purpose. If it ever needs to be, it belongs in
`tar-diff.sh` § 9 with this section as its reason.

**How to reverse.** `Roots` is ~60 lines in `userspace/coreutils/src/bin/tar.rs`
and the level is threaded through `Selector::wants`, `pending_dirs`, and
`DelayedLink::dir`. Reverting to one destination means dropping the level and
calling `enter_chdirs(chdirs, &mut n, chdirs.len())` once — and re-breaking the
eight `-C` cases in `scripts/tar-diff.sh` § 8 that this entry exists to explain.
