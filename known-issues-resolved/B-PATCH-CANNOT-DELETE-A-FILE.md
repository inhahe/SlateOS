## B-PATCH-CANNOT-DELETE-A-FILE (lane B, 2026-09-12) — FIXED

A patch whose destination is `/dev/null` deletes the file. This build wrote a
**one-byte** file instead — the empty join plus the trailing newline the
original had — and left it in the tree.

Measured: `diff -u --label x/a/keep.txt --label /dev/null keep.txt /dev/null`
applied by GNU leaves no `keep.txt` at all, **with or without `-E`**.

**I had implemented the weaker rule and assumed it covered both.** `-E` removes
a file the patch merely *emptied*, however the patch was spelled. A `/dev/null`
destination says outright that the file is gone, and needs no flag. Having just
added `-E` an hour earlier, I read the `delete.patch` cases as belonging to it
— and the case that disproved that passes no flag at all.

The reverse direction is handled: reversed, a deletion is a creation, so the
rule is off under `-R`. And it is suppressed under `-o`, for the same reason `-E`
is: the destination is not the file the patch was read from.

`patch-diff.sh`: 44 passed / 21 differed to **47 / 18**. Three cases, all in the
`delete.patch` family, one of which was the plain no-flag invocation.
