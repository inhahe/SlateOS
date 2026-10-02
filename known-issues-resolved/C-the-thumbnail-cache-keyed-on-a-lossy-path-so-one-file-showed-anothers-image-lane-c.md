## The thumbnail cache keyed on a lossy path, so one file showed another's image (lane C)

**Status: FIXED 2026-08-15** (lane C), commit below. Fourth instance of the
lossy-path class, and the first where the damage is not a lost name but a
**collision**.

`Thumbnail::source_path` was a `String` built with `to_string_lossy`, and it is
the disk cache's key: `simple_hash` FNV-hashes it with the mtime to produce the
cache filename. Every undecodable byte in a name became the same U+FFFD, so two
genuinely different files whose names differ only in such bytes hashed to one
cache entry — and the file explorer displayed one of them the other file's
thumbnail. Nothing errors; the wrong picture is simply shown. `source_path` is
now a `PathBuf` and `simple_hash` takes `&Path` and hashes
`as_os_str().as_encoded_bytes()`.

`purge_stale` had a matching problem in the other direction: it compared
directory entries by `to_string_lossy`, so a foreign file in the cache
directory whose name is not UTF-8 could be *rendered into* something matching
our `{hash:016x}.thumb` shape and deleted. Now compared as bytes.

**The lesson worth keeping is about the tests, not the bug.** The natural
regression test for this class needs a path the platform cannot decode, and on
the Windows test host `OsString` cannot hold arbitrary bytes at all — so the
obvious test has to be `#[cfg(unix)]` and never actually *runs* here. A
`cfg(unix)` test is compile-checked at best (and until this session, not even
that — see the note in the `apps/backup` entry above). The fix is to find the
host's *own* uncodable case: on Windows an unpaired surrogate is a legal
`OsString` that `to_string_lossy` maps to U+FFFD, which reproduces exactly the
same collision. Both tests now exist, and the Windows one was confirmed to fail
when the lossy hash is put back. Any future test in this class should carry a
runnable-on-the-host twin rather than a Unix-only assertion.
