## 756. A shared `Stat` gained an `mtime` field for one caller, because the alternative was that caller re-declaring `struct stat` to read a field the shared call had already fetched

**Date:** 2026-09-03
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `coreutils` has one small type that says what a file is — its
kind, its identity, its size — filled in by one system call that a directory
walk makes anyway. `tar` needs one more thing out of that same call: when the
file was last changed, so that `--keep-newer-files` can decline to overwrite
something newer than the archive's copy. The choice was to add that field to
the shared type, or to let `tar` keep its own hand-written copy of the operating
system's file-information structure purely to read a field the shared call had
already fetched and thrown away. The field was added.

### What was actually at stake

`dirfd::Stat` is built from a `struct stat` that `fstatat` has just filled in.
The buffer already contains `st_mtim`; the constructor simply did not copy it
out. So "tar keeps its own" did not mean tar making an extra call — it meant tar
declaring a **second** `#[repr(C)] struct CStat`, a second `extern fn fstatat`,
and a second `const _: () = assert!(size_of::<CStat>() == 144)`, to make the
same call against the same descriptor with the same flags, two lines away from
the one that had the answer.

That is precisely the duplication the extraction was undertaken to end. Keeping
it for one field would have meant finishing a refactor that deleted eleven
`extern` declarations by leaving the twelfth, and leaving with it the two
declarations most likely to rot — a `struct stat` layout and its size assertion
are correct until a libc changes underneath them, and then they are a stack
overwrite rather than a compile error.

### Why this is not the widening that was refused earlier

`dirfd.rs` already carries a written refusal to add `mode` and `mtime`, from
when `rm`'s tests wanted to read them: *"Widening it so that a test could read
them would be letting the test drive the shared API; when `tar`'s
archive-writing side needs those fields it can ask for them then, on its own
evidence."*

The distinction is the one that comment drew in advance, and it held:

| | `mode`/`mtime` for the tests | `mtime` for `tar` |
|---|---|---|
| Who wants it | a `#[cfg(test)]` helper | production code on the main extraction path |
| What it costs to refuse | the test reads it through `std` — three lines | a duplicate `struct stat`, `extern`, and size assertion |
| What refusing protects | the type stays the size of its purpose | nothing; the field is already in the buffer |

A test that wants a field can go and get it; that is what a test is for. A
caller on the hot path cannot, without rebuilding the very thing the module
exists to be the only copy of. `mode` is still absent for exactly this reason —
nothing but a test has ever asked.

### The alternative that was rejected, and why

**Keep a narrow tar-local `fstatat` for this one question.** Considered
seriously, because it confines the widening to zero. Rejected because "one
question" is not a stable quantity: the same call answers `is_dir` for
`--keep-newer-files`, and the identity for the delayed-symlink placeholder, and
would then be answering three questions through a private copy while the shared
one answered the other forty. The end state is two `struct stat`s again with a
line drawn between them that nobody can remember, which is the state this whole
piece of work was undoing.

### What was given up

`Stat` is 8 bytes larger and its doc comment is one clause longer, on every
caller including the ones that never read the field. That is the entire cost;
it is copied by value in a walk that is already making a syscall per entry, so
it is not measurable. The type's stated purpose — "the whole of what a walk
reads from a lookup" — is unchanged in kind: an mtime is something a walk reads
from a lookup.

### One detail worth recording

The field is **seconds, signed**, and a pre-epoch time stays negative rather
than clamping to zero. The direction is not symmetric: a 1960 file reported as
1970 compares *newer* than it truly is, so `--keep-newer-files` would skip a
member it was supposed to extract — a silent data loss, where the opposite error
is a harmless redundant write. The non-unix constructor has to recover this from
`SystemTime::duration_since`'s *error* payload, which carries the interval back
to the epoch, and there is a test at the boundary in each direction.
