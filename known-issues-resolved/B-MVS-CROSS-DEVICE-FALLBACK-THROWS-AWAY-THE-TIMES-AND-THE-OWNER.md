## B-MVS-CROSS-DEVICE-FALLBACK-THROWS-AWAY-THE-TIMES-AND-THE-OWNER — FIXED 2026-09-01

**In short:** when `mv` moves a file between two filesystems it cannot rename it,
so it copies the bytes and deletes the original. Ours copies the bytes and the
permission bits and *nothing else*: the file arrives with today's date instead of
the date it was written, and (if run as root) owned by root rather than by
whoever owned it. GNU's `mv` carries both over, and a user who moves a photo
directory off one disk onto another does not expect every photo to be re-dated to
the moment of the move. Nothing user-visible is broken on a single-filesystem
machine, which is why this has gone unnoticed: no case in `scripts/mv-diff.sh`
crosses a filesystem boundary, because mounting a second one needs a password the
harness must not ask for.

**Correction, 2026-09-01 (same day, later).** That last sentence was wrong, and
it was load-bearing. Nothing had to be *mounted*: Linux already has a second
filesystem mounted and world-writable, at `$XDG_RUNTIME_DIR` or `/dev/shm`, and
either is a different `st_dev` from the `/tmp` the harness works in. §22 of
`scripts/mv-diff.sh` now crosses the boundary on every run, with no password and
no namespace, and its first run found two further defects that the reasoning
above had been quietly shielding — the two entries below. The claim was never
checked against `stat -c %d`; it was inferred from "mounting needs root", which
is true and irrelevant, because the mount was already there.

**Where.** `userspace/coreutils/src/bin/mv.rs:1601`, in `copy_across_devices`:

```rust
    fs::copy(src, target)?;
    fs::remove_file(src)
```

`std::fs::copy` is documented to copy the permission bits and is documented not
to copy anything else. It is the whole of the plain-file arm; the symlink arm
above it (`:1584`) recreates the link's text correctly and is not affected, and
the directory arm (`:1594`) refuses outright and is its own entry.

**What GNU does.** `copy.c`'s `copy_reg` finishes with `set_owner`, then
`set_authorized_context`, then `copy_acl`/`set_acl`, then `utimens` from the
source's `st_atim`/`st_mtim` — in that order, and the order matters, since
`chown` clears set-user-ID and so must precede the mode. For `mv` the flags that
select all of this are on unconditionally: `cp_option_init` (`mv.c:119`) sets
`preserve_timestamps` (137), `preserve_ownership` (134), `preserve_mode` (136)
and `preserve_links` (135), because a move is supposed to be indistinguishable
from a rename.

**The proper fix.** Replace the two lines with an explicit sequence in the same
order upstream uses: create the destination, copy the bytes, `fchown` (ignoring
`EPERM`, as upstream does when not preserving is merely unprivileged rather than
an error), `fchmod` from the source's mode including the set-ID bits, then
`futimens` with the source's `st_atim`/`st_mtim` at full nanosecond resolution —
and only unlink the source once all of that has succeeded. Nanoseconds rather
than seconds because the next entry depends on them.

**Two things that fall out of it.** First, `mv -u`'s comparison is currently a
plain `(mtime)` comparison; upstream passes `UTIMECMP_TRUNCATE_SOURCE` exactly
and only when the move is cross-device (`copy.c:2379`), which rounds the source's
stamp down to the destination filesystem's resolution before comparing. That flag
is unimplemented here — see `destination_is_older`'s doc at `mv.rs:1300` — and it
is *doubly* unreachable, because a fallback that does not preserve the timestamp
at all has no preserved timestamp for the truncation to be about. Whoever does
the fix above should do the truncation in the same change, since that is the
moment it stops being unreachable. Second, hard links across the fallback: `mv`
sets `preserve_links`, so a group moved together should arrive as a group, and
`fs::copy` gives one independent file per name. That is the same shape as the
directory refusal and belongs with a recursive fallback rather than with this.

**How it would be caught.** ~~It would not be, by anything we have. A regression
test needs two filesystems.~~ **Superseded by the correction above:** it *is*
caught, on every run, by §22 of `scripts/mv-diff.sh`, where six cases are
`xfail_case`s naming this entry. They turn into XPASS the moment the fix lands,
which is what will force them to be promoted to real cases. The original
paragraph's other half still stands and is still worth doing: the unit tests in
`mv.rs` drive `copy_across_devices` directly, so a test that stamps a source,
calls the function, and asserts the destination's mtime is the source's pins the
timestamp half at the function boundary where the differential harness can only
see it through the whole program. Write it with the fix.

**Fixed** by `copy_across_devices` doing the open, the create, the copy and the
four preservation steps itself instead of calling `fs::copy`, in GNU's order:
bytes, times, owner, mode. Three pieces are worth naming because none of them is
obvious from the sentence above.

* **The mode is written last, and a refused `chown` costs the set-ID bits.**
  GNU's own comment is the argument — *"chown turns off set[ug]id bits for
  non-root, so do the chmod last"* (`copy.c:3245`) — and `copy_reg`'s `case 0:
  src_mode &= ~(S_ISUID | S_ISGID | S_ISVTX)` is why being *refused* the
  ownership also drops them: a set-user-ID bit on a file that could not be given
  to its source's owner is a privilege granted to whoever holds it now.
* **The destination is created with the group and other bits held back**
  (`create_destination`), which is GNU's `omitted_permissions`. Without it a
  file whose source is world-readable is briefly world-readable *while holding
  the source's contents and before the `chown`*.
* **The ownership decision is shared with `cp`, not copied.**
  `fsattr::owner_differs` and `fsattr::take_ownership` were split out of `cp`'s
  `chown_to_source` in the commit before this one, so the group-only retry, the
  root check and the set-ID consequence have exactly one implementation.

Three unit tests pin it at the function boundary as this entry asked —
`the_cross_device_fallback_carries_the_times`, `..._carries_the_set_user_id_bit`
and `..._carries_a_links_own_time`, the last of which also asserts the link's
target was *not* stamped through the link. Six `xfail_case`s in
`scripts/mv-diff.sh` §22 became XPASS on the first run after the fix and are now
plain `run_case`s.

**Two things that did not fall out of it.** The `UTIMECMP_TRUNCATE_SOURCE` half
of `-u`'s comparison became *reachable* with this fix — there is a preserved
timestamp for the truncation to be about — and was still not implemented when
this paragraph was first written. **It was implemented the same day**, in
`userspace/coreutils/src/utimecmp.rs`: a port of gnulib's `lib/utimecmp.c` that
deduces the destination filesystem's resolution from the trailing zeros of its
three stamps and, when that is not decisive, measures it by writing a probe
timestamp and reading back which digits survived. `mv.rs`'s
`destination_is_older` became `destination_is_up_to_date` and passes the flag as
`!fileid::same_device(src, dst)`, which is what upstream's expression reduces to
for `mv`. Five `-u` cases were added to §22 — with the caveat, stated there,
that they cannot *measure* the truncation: both filesystems the harness can
reach keep nanoseconds, so the resolution deduces to one nanosecond, the
truncation is the identity, and upstream's own `SYSCALL_RESOLUTION < res` guard
skips the interesting code entirely. Breaking that code leaves all five passing;
only the unit tests catch it. And `preserve_links` across the fallback is
untouched; that half has been split out into its own entry below, because
leaving it under a heading marked FIXED would make a reader who found the §22
case believe it was already dealt with.
