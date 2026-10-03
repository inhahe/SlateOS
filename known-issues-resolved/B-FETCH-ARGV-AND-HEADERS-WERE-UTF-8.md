### B-FETCH-ARGV-AND-HEADERS-WERE-UTF-8. Six more defects found while converting `fetch` to bytes — 2026-08-30 — FIXED 2026-08-30 (lane B)

**Where:** `userspace/coreutils/src/bin/fetch.rs`, throughout. This was the
`fetch.rs:argv-as-string` line of `scripts/argv-utf8-baseline.txt`; the
conversion is what surfaced the rest, including the resolver bug above.

1. **`env::args().skip(1).collect::<Vec<String>>()`** — the tracked finding.
   A URL or `-o` name holding a byte that is not valid UTF-8 (a legal filename
   on this OS) aborted the process before `main` ran. Now `args_os()` carried
   through as `OsString`/`Vec<u8>` to the socket and to the `open`.
2. **`String::from_utf8_lossy` on the response header block.** A `Location:`
   or `Content-Disposition:` byte that is not UTF-8 became U+FFFD, and with
   `-O` that replacement character went into a **file name**. Headers are now
   `Vec<u8>` end to end; only the host is required to be ASCII.
3. **A hand-written option loop.** No `--`, no bundling (`-qv`), no attached
   value (`-oout.html`, `--output=…`), no unambiguous long-prefix (`--verb`),
   and no `--version` at all. Replaced with `coreutils::getopt`, which gives
   all of those and POSIX's own error wording.
4. **`println!` for the `-I` header dump.** A `SIGPIPE`-less write failure
   panicked instead of being reported; `fetch -I url | head -1` was the way in.
   Now `stdfd::Stream::stdout` + `close_stdout`, as the rest of the crate does.
5. **`-d` could not send a binary body.** The data came in as `String`, so a
   POST of anything but text was impossible. Now `Vec<u8>`.
6. **`find_subsequence` could panic.** `haystack.windows(n)` panics when
   `n == 0`; an empty needle reached it from the header scan. Guarded, with a
   test.

Also fixed in passing: the `Saved to:` diagnostic printed the path unquoted,
so a name with a space or a control byte was ambiguous or invisible — it now
goes through `quotef_os`, and `-O` refuses a URL whose last segment is `.` or
`..` rather than writing to the parent directory.

`python scripts/argv-utf8.py --check` now reports **4 findings across 4 files**
(`diff logger patch ps`), down from 5.
