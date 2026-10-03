## B-RUSAGE-AND-STATVFS-WERE-SHORT-OF-MUSLS-SIZE (lane B, 2026-09-09) -- FIXED the same day

**In short:** two structures the system fills in for a caller were smaller than
the C library's, so calls that are supposed to fill the caller's object filled
the front of it and left the rest untouched.

| | ours, before | musl |
|---|---|---|
| `struct rusage` | 144 | 272 |
| `struct statvfs` | 88 | 112 |

**Where.** `posix/src/resource.rs` and `posix/src/statvfs.rs`.

**Why neither was noticed.** Every *named* field was already at the correct
offset in both; only the trailing reserved arrays were missing. `getrusage`,
`wait4` and `statvfs` therefore returned correct values for everything anyone
reads, and left the tail as the caller's allocator had it.

**Fixed** by carrying musl's `__reserved[16]` and `__f_spare[6]`. Found by
`scripts/check-libc-abi.py`.

**`statvfs`'s size test could not have caught it**: it asserted
`size_of::<Statvfs>() == 11 * 8`, restating the declaration it was checking. A
test whose expected value comes from the thing under test passes for any
declaration -- and occupies the place a real check would go.
