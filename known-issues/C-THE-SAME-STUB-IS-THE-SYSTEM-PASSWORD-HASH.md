## C-THE-SAME-STUB-IS-THE-SYSTEM-PASSWORD-HASH (found by lane C, owned by lane B, 2026-08-17)

**In short.** The made-up mixing function that `apps/diskimager` was passing
off as three checksums is also, byte for byte, the function `userspace/login`
and `userspace/chpasswd` use to hash passwords into `/etc/shadow`. Two
consequences, both reproduced: a password set with `passwd` cannot be used to
log in at all, and the entries `chpasswd` writes are labelled `$5$` — the
standard crypt(3) identifier for SHA-crypt — while containing something that
is not SHA-crypt, not SHA-256, and not stretched by any number of rounds.

**Not mine to fix.** `userspace/**` is lane B's. Filed in full, with the
reproduction and the mechanical check that found it, as
`requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`. Logged here so
it is not lost if that request is actioned and removed.

**The short form of the fix**, which is lane B's call: `posix/src/crypt.rs` is
already a correct and complete SHA-crypt — `$5$`/`$6$`/`$1$`, the `rounds=`
field, the crypt base-64 alphabet, Drepper's published vectors, 29 tests —
delegating its core to `posix/src/sha2.rs`. Three tools that read and write
one file should be calling it instead of each hashing differently. The part
with a genuine tradeoff is what to do with the entries already written in two
wrong formats.

**Why this is in lane C's tracker at all.** Because the two bugs are the same
bug, and finding the second one cost nothing once the first was understood.
The check is mechanical: extract every 8-hex-digit literal from a file that
claims to implement a published algorithm, and look for a contiguous run
matching the algorithm's published constant table. `chpasswd` and `login`
carry the genuine SHA-256 IV and no K table at all — exactly the disk imager's
shape. It is worth running over any transcribed algorithm in this tree,
because it costs nothing and, unlike a test that checks the digest's shape, it
cannot be satisfied by a plausible-looking function.
