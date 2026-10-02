## B-THE-DIFF-PATCH-PIPELINE-NOW-SURVIVES-A-BYTE-THAT-IS-NOT-UNICODE (lane B, 2026-09-14)

**Status:** VERIFIED 2026-09-14 — the joint result of the two fixes above

Neither entry above states the thing a user would actually care about, because
neither fix alone delivers it. Measured end to end, with our own two binaries:

    $ diff -u orig.txt new.txt > u.patch     # line 2 holds byte 0351
    $ cat -v u.patch
    --- orig.txt
    +++ new.txt
    @@ -1,3 +1,3 @@
     alpha
     cafM-i comment
    -charlie
    +CHARLIE
    $ patch -i u.patch target.txt
    $ cmp target.txt new.txt                 # IDENTICAL

**Before today this pipeline was broken twice over, independently**, and each
fault would have masked the other:

| stage | what it did | class |
|---|---|---|
| `diff` | wrote U+FFFD into the patch in place of `0351` | silent corruption |
| `patch` | refused to read the patch at all, exit 2 | loud refusal |

Fixing only `patch` would have left `patch` faithfully applying a corrupted
patch — a *worse* outcome than the refusal, because the corruption would then
reach the file. That is worth stating plainly: the two fixes were found
separately and are recorded separately, but shipping one without the other
would have made the pipeline quieter rather than better.

The round trip is the case to reach for when either file is touched again. It
exercises both halves through their real entry points and its assertion is
`cmp`, not a transcript, so it cannot pass on a right-looking message.
