### TD-OILS-RW-OFFSET. `osh`'s `<>` read/write descriptor does not share one OS file offset — 2026-07-19 — ✅ RESOLVED 2026-07-28

**What:** The `<>` (open-for-read-write) redirect is now implemented
(`RedirectOp::ReadWrite`), covering `<> file` (fd 0 → stdin), `1<>`/`2<>`
(no-truncate write), and `exec {fd}<> file` (persistent rw descriptor).
The *write* side is fully faithful — writes land at offset 0 and overwrite
in place, matching bash (verified via `od`). The read and write halves are,
however, **two independent handles**: `open_fds` holds the read side and
`open_write_fds` the write side, and since 2026-07-28 the read side is a real
open `File` (`InputSrc::File`, see TD-OILS-PIPE-HEAD-CURSOR-DRAIN) rather than
a byte snapshot — but it is a *separate* open of the path, with its own
offset. A real `O_RDWR` descriptor shares ONE OS file offset between reads and
writes, so in bash a `read` after a `>&N` write on the same `<>` fd continues
from the post-write position; in osh the read handle's position is independent
of the write handle's.

(The 2026-07-28 shared-descriptor work fixed the *other* half of this — reads
through a `<>` fd now share one offset with children, subshells and pipeline
stages, exactly as `< file` does. What remains is only the read↔write split.)

**Where:** `userspace/oils/src/interp.rs` — `ExtraFdOp::ReadWriteFile`
(install in `install_extra_fds`, exec loop, and `apply_persistent_redirect`),
`open_rw` helper, and the `RedirectOp::ReadWrite` arm of `resolve_redirects`.

**Repro:** `exec 3<>f; echo AB >&3; read -u 3 x; echo "[$x]"` — bash reads
from just past "AB\n" (EOF → empty); osh's read handle still starts at 0. This
is unusual in real scripts (interleaved read+write on one `<>` fd), so it is
low priority.

**Proper fix:** unify osh's fd model so a single descriptor is backed by one
`File` opened `O_RDWR` and usable for both directions, rather than the split
two-handle representation. `InputSrc::File` already carries a live `File` with
faithful seek/read-ahead accounting (`FileInput::sync`), so the remaining work
is to let one entry appear in both `open_fds` and `open_write_fds` backed by
the same handle, and to make the write path sync the read buffer first — a
narrower change than it was, but still touching every builtin that reads or
writes user-space fds. Deferred until there is a concrete need beyond `<>`.

**Fix (2026-07-28) — ✅ the shared offset now works.** The proper fix, and it
turned out to be small once the read side was a live `File`:

1. **One open, two duplicates.** `open_rw_pair` opens the path `O_RDWR|O_CREAT`
   *once* and `try_clone`s it; the clone is a duplicate of the descriptor (dup
   on unix, `DuplicateHandle` on Windows), so the two halves name one open file
   description and share one offset. `ExtraFdOp::ReadWriteFile(String)` becomes
   `ExtraFdOp::ReadWrite(InputFd, Arc<File>)` — the pair is opened at
   *resolution* time and carried, rather than the path being re-opened (twice!)
   at install time, so the binding and every duplicate of it share a position.
   All four `<>` sites go through it: both `install_extra_fds` loops, the
   `RedirectOp::ReadWrite` arm of `resolve_one_redirect`, and
   `apply_persistent_redirect`.
2. **Read-ahead is squared up after every read.** A buffer over a descriptor
   whose position the *write* half can observe is a hazard with no escape point
   to unwind at — the write never passes through `FileInput`. So `FileInput`
   gains an `observable` flag (set by `rw_input`), and `lock_input` now returns
   an `InputGuard` that calls `sync()` on drop when it is set. This is bash's
   rule (`zsyncfd` after the `read` builtin), and it is strictly necessary
   rather than merely tidy: a record reader must peek one byte *past* its
   delimiter, so `read -N 3` would otherwise leave the offset at 4 — an error
   that not reading ahead at all would not have prevented.

Verified byte-identical to bash 5.2 across writes-then-reads, reads-then-writes,
alternation, `exec 4<&3` and `exec 4>&3` dups, an external child, a subshell, a
partial `read -N`, `cat <&3`, fd 0, and create-without-truncate:
`tests/corpus/redirect-rw-shared-offset.sh` (new) plus four `interp.rs` unit
tests (`read_write_fd_*`, `read_write_redirect_creates_without_truncating`).

**Residual — writing to fd 0 is still not modelled.** `>&0` resolves to stdout
whatever fd 0 is, so `{ echo X >&0; } <> f` prints to the terminal instead of
patching `f`, and `{ echo X >&0; } < f` succeeds instead of failing with
bash's `echo: write error: Bad file descriptor` (status 1). That is a
*write-side* gap — fd 0 has no `WriteFd` slot at all — and is tracked
separately as TD-OILS-FD0-WRITE. **(Closed 2026-07-28: fd 0 now carries its
write half and its access mode; see TD-OILS-FD0-WRITE, resolved. The fd-0 lines
excluded from `tests/corpus/redirect-rw-shared-offset.sh` for this reason are
now covered by `tests/corpus/redirect-fd0-write.sh`.)**
