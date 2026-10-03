## C-A-BUILD-THAT-NEVER-COMPILES-BECAUSE-THE-TARGET-DIRECTORY-CANNOT-BE-LISTED — ENVIRONMENT, WORKED AROUND 2026-09-02 (lane C)

**Status:** worked around 2026-09-02 by deleting the affected tree. Not a code
defect; recorded because the symptom is badly misleading and lanes A and B keep
their own `target/` trees on the same volume.

**In short:** a `cargo test` stopped finishing. It printed `Compiling net80211`
and then sat there — twenty minutes, no error, no progress. It looks exactly
like a slow machine or a compiler stuck in a loop, and it is neither: the
compiler was blocked trying to *read the list of files* in its own build-output
directory, which had reached a state where that listing never returns.

### How to recognise it

- `cargo` prints `Compiling <crate>` and stalls indefinitely. No error is ever
  printed, and the timeout is the only thing that ends it.
- The giveaway is **CPU time, not wall time**: `wmic process where
  "name='rustc.exe'" get processid,kernelmodetime,usermodetime` showed ~1.2
  seconds of CPU consumed across 15 minutes of wall clock. A compiler that is
  merely slow burns CPU; one that is blocked does not. This one check separates
  "the machine is busy" from "something is stuck", and is worth reaching for
  before waiting any longer.
- `ls target/<triple>/debug/deps` and `cmd /c dir` on the same directory both
  hang forever, while every other operation on the same volume — `git status`,
  reading files, listing sibling directories — returns instantly. A volume that
  is slow is slow everywhere; this was one directory.

### What it actually was

A stack from `cdb -p <pid> -c "~*k12;qd"` put rustc's main thread in
`rustc_session::search_paths::SearchPath::new` → `std::fs::DirEntry::path`.
`SearchPath::new` enumerates each `-L dependency=…` directory at startup, so
every rustc invocation lists `deps/` before it compiles anything. With that
directory unreadable, no compile in the workspace could start.

Root cause of the directory's state is not established — a damaged NTFS index
or simply an enormous entry count both fit. What is established is that the
volume was **not** at fault (88 GB free, no Defender scan running, 30 GB of
physical memory free, and unrelated I/O on the same drive was instant).

### The remedy

Delete the affected tree; it is build output, so it is regenerable by
definition and gitignored. `rd /s /q` from a real `cmd` (not `rm -rf` through
MSYS), which is itself slow here — it reclaimed the large artifacts in the
first minutes and then spent much longer on the small ones, at ~14 seconds of
kernel CPU over 11 minutes, so let it finish rather than concluding it is stuck
too. Stop every `cargo`/`rustc` process first: deleting a cache out from under
a live build produces errors that look like new problems.

**Worth knowing:** a `cdb` stack is a cheap first move on any silent hang here,
not a last resort — it took one command to convert "the build is mysteriously
slow" into "rustc is blocked in `SearchPath::new`", which named both the cause
and the fix.
