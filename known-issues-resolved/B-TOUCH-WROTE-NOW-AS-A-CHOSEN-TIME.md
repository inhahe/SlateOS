## B-TOUCH-WROTE-NOW-AS-A-CHOSEN-TIME (lane B, 2026-09-25) — FIXED 2026-09-25

**In short:** `touch f` on a file its user may write but does not own --
`/dev/null`, a group-writable file in a shared directory -- failed with
`setting times of 'f': Operation not permitted`. GNU succeeds. Ours read the
clock and wrote that instant, and the kernel lets only a file's owner write a
chosen time; asking for *now*, which GNU does by passing no times at all, needs
only write permission (`utimensat(2)`).

**Measured** in WSL against GNU 9.4 as an ordinary user: `touch /dev/null`
exits 0 there and 1 here; `touch -a /dev/null` and `touch -m /dev/null` fail on
both, because *now* on one half and `UTIME_OMIT` on the other needs the owner
again.

**How it was closed.** `coreutils::fsattr` gained `When::Now`, the kernel's
`UTIME_NOW`, and `Times::now()`; `touch` asks for it whenever no `-r` or `-t`
was given. The descriptor path (`touch -`) calls `futimens` directly, because
`std`'s `File::set_times` has no way to say *now*; the Windows arm reads the
clock, which loses only a permission rule that host does not have. Pinned by
`touch-diff.sh`'s `/dev/null` rows and `fsattr`'s `now_is_the_other_sentinel`.
