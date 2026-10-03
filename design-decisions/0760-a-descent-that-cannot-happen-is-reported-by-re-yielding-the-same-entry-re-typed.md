## 760. A descent that cannot happen is reported by re-yielding the same entry, re-typed — because the module has exactly one entry record and stepping past a mark erases it

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** When `find` or `rm -r` walks a tree and hits a directory it
cannot open — no permission, say — it should print "Permission denied" and
carry on. Ours printed nothing: the walk noted the failure and then
immediately overwrote the note with the next file it found, so nobody ever saw
it. The same was true of a directory too deep for our fixed traversal stack:
everything below the eighth level simply did not exist as far as the caller
was concerned. Both now hand the caller back the *same* directory a second
time, relabelled "could not read this", with a reason attached.

### Why the shape is forced, not chosen

`fts` allocates nothing. There is one `FtsEnt` per open traversal, living in
the instance, and `fts_path` points into the one path buffer that is mutated in
place as the walk descends and ascends. That is a deliberate design (it is why
`fts_children` returns `ENOSYS` — see its doc comment), and it has a
consequence that was not noticed until it bit: **a failure cannot be recorded
and reported later, because there is nowhere to record it.** The old code did
exactly that —

```rust
inst.current.fts_info = FTS_DNR;   // mark it
inst.current.fts_errno = e;
// ... falls through ...
step(inst)                          // writes the next child into `current`
```

— and the mark was gone one function call later. The code that set it is
locally correct and reads as if it works, which is why it survived review: the
bug is not in the statement, it is in what the next statement does.

So the only way to report is to `return` the entry. That means the caller of
`descend_into_current` must be the one that reports, and
`descend_into_current` must not touch `current` at all — it returns
`Result<(), i32>` and says nothing about presentation. This is a general rule
worth carrying: **in a module with a single shared output slot, the function
that discovers a fault cannot also be the function that reports it, unless it
is also the one that returns.**

### The alternatives

| Option | Why not |
|---|---|
| Yield a *separate* `FTS_ERR` entry after the `FTS_D` | Needs a second entry record — or the same path buffer at two levels at once, which is precisely what this module does not have. |
| Set a sticky error on the stream, checked by `fts_close` | Callers learn at the end which files they missed, which is useless for `find` (wrong place in the output) and dangerous for `rm -r` (the exit code is right but the damage is done). |
| Leave the depth case silent, as it was | This is the one the old comment claimed was BSD behaviour. It is not: glibc's `fts` has no depth limit. The claim was invented to justify what the code happened to do. |

Re-yielding the same entry re-typed is also **what glibc does** — `fts_read`
returns `p` again after `fts_build` fails, with `p->fts_info` now `FTS_DNR` —
so callers written against glibc already handle it. The cost is that a caller
which ignores `FTS_DNR` sees the same path twice, once as `FTS_D` and once as
`FTS_DNR`. That is the price of the single-record design, it is what portable
callers expect, and it is strictly better than the path being silently absent.

### Why `ENOMEM` for the depth limit

Extracted into `descent_refusal(depth) -> Option<i32>` rather than left inline,
because the errno *is* the decision and it is the part a reader will want to
argue with. Running out of traversal stack says nothing about the directory, so
`EACCES` would send someone to check permissions that are fine, and `ELOOP`
would claim a symlink cycle we did not detect. `ENOMEM` is what BSD's
`fts_build` reports when it cannot get what it needs, which is the same shape
of answer: the walker ran out of something, not the filesystem.

The depth cap itself (8) remains, and remains too shallow — tracked as
`TD-B-FTS-DEPTH-IS-CAPPED-AT-EIGHT` in known-issues.md, along with the
`telldir`/`seekdir` route that would remove the coupling to `dirent`'s pool.
Making the limit honest and raising the limit are two changes, and only the
first is unambiguously right.

See known-issues.md `B-FTS-SWALLOWS-EVERY-DESCENT-IT-CANNOT-MAKE`.
