## 1017. Two defects that cancelled: `pgrep`'s pattern and its data were both re-encoded

**Date:** 2026-09-10
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `pgrep` searches for processes by name. It was mangling the text
it searched *through* and the text it searched *for* in exactly the same way,
so the two mistakes cancelled out and the answers looked right. Fixing the
obvious half on its own would have broken every search containing a non-English
character -- and did, in the tests, before the second half was found.

**The two halves.**

`read_cmdline` built a `String` with `bytes.iter().map(|&b| b as char)`. That
is a **Latin-1** re-encoding, not a UTF-8 decode: byte `0xE9` became `U+00E9`,
which is two bytes of UTF-8.

`split_alternatives`, in the pattern compiler, rebuilt the pattern the same way
-- `current.push(b as char)` -- before the engine turned it into
`Token::Literal(u8)`s.

Both sides therefore arrived at the matcher having been through the same
bijection, and byte `n` on one side compared equal to byte `n` on the other.
**Byte-exact matching, by accident.**

**How it was found**, which is the part worth keeping. Not by reading the
pattern compiler. The data side was converted to bytes for an unrelated reason
-- a process whose name is not UTF-8 was being dropped from `pgrep` entirely,
see below -- and two tests written to pin the *new* behaviour failed:

* a command line holding a real UTF-8 `é` did not match a pattern `é`;
* and it should have started matching, because that was the whole point.

The failure said the pattern side was still transcoding. Without those two
tests the change would have shipped as a regression that only shows up for
non-ASCII patterns, which nothing else in the suite uses.

**The rule this is an instance of.** *A pair of compensating defects reads as
correct until someone corrects one of them* -- and the person who corrects one
is, by construction, someone who has just decided that half is wrong. They will
be looking at a newly-failing test with a fresh explanation ready ("my change
broke it") that is true but not the reason. Both halves have to move together,
and the way to find the second half is to test the property you claim to have
fixed rather than the code you touched.

**The defect that started it.** `/proc/<pid>/stat` was read with
`read_to_string`, which *fails* on a non-UTF-8 `comm`; the `.ok()` turned that
into `None` and the process vanished. `pgrep` and `pkill` are one binary, so a
process could decline to be signalled by name by having a name that is not
text. It also took `pid`, `ppid`, `pgrp` and `tty` with it -- all plain digits
further along the same line, none of which has anything to do with the name.
`procinfo::ProcessStat` already reads it as bytes and its doc already named
this bug in `htop`; three more programs had it and two (`top`, `pstree`) still
do.

**Also here:** `procinfo::ProcessStatus` gained `euid`. Its own doc had said "a
caller wanting the effective UID wants another field here, not a different
index at the call site" -- `pgrep` was the first caller to want it, and the
four columns of `Uid:` are now named rather than counted.
