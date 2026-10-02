## D-POSIX-STDIO-WAS-SIXTEEN-UNLOCKED-SLOTS — the C library's streams were a fixed pool with no locks, and a dozen calls answered what glibc does not (lane D, 2026-09-27) — **Status: FIXED 2026-09-27**

**In short:** a C program's streams -- how it reads and writes files
through `fopen`, `fgets` and `printf` -- came from a pool of sixteen, so its
seventeenth open file failed; nothing locked them, so two threads writing one
stream could corrupt it; `fread` from a pipe could return early saying
neither "end of file" nor "error"; and `fopen` ignored the letters that ask
for "fail if it exists" (`x`) and "close on exec" (`e`), so `"wx"` truncated
a file it should have refused.  The streams are rewritten on musl's design
and behave as glibc's do (design-decisions.md §1121).

| What | Was | Now |
|---|---|---|
| open streams | 16 slots; the 17th `fopen` failed `EMFILE` | as many as memory allows |
| threads | no lock (`flockfile` a no-op) | a recursive lock per stream; `flockfile`, `ftrylockfile`, `funlockfile`, `__fsetlocking` |
| `fread` from a pipe | returned after one short `read`, flags unset | waits for all it asked for, end of file or an error |
| buffering | 1 KiB; `stdout` always line buffered | 4096 bytes; line buffered only on a terminal |
| `fopen` modes | `+` seen 2nd or 3rd only; `x`, `e` ignored | glibc's letters: `x` `O_EXCL`, `e` `O_CLOEXEC`, `+` anywhere in six, `,` ends |
| `fopen` NULL path or mode | `EINVAL` | `EFAULT` (§1115) |
| `fdopen` | mode ignored, any descriptor taken; 0-2 returned `stdin`/`stdout`/`stderr` | glibc's: `EBADF` for a closed descriptor, `EINVAL` for a mode it cannot give, `a` sets `O_APPEND`; always a new stream |
| writing a read-only stream | buffered and reported success; failed at the flush | `EOF` and `EBADF` at once |
| a write after a read on `r+` | landed after the read-ahead | lands at the stream's position |
| `fclose(stdout)` | flushed; never closed descriptor 1 | closes it |
| `freopen(NULL, mode, f)` | did nothing | reopens the file (glibc); changes the flags in place for a pipe |
| `fflush` on an input stream | nothing | gives back the read-ahead |
| `exit` | flushed output | flushes output and gives back the unread input of streams used |
| `popen` | no `e`; the child inherited earlier `popen` pipes; `pclose` of another stream `EINVAL` | glibc's: `e`, `sh -c --`, earlier pipes closed in the child, `pclose` of any stream closes it |
| wide streams | no orientation; `ungetwc` ASCII only; over-long UTF-8 accepted | `fwide` orientation as glibc keeps it; `ungetwc` of any character; over-long forms and surrogates `EILSEQ` and a stream error |
| `remove` of a directory | failed | `rmdir` after `EISDIR` |
| `tmpnam` | `/tmp/tmp_NNNNNN` from a counter, never checked | `/tmp/fileXXXXXX`, random, checked with `lstat` |
| new calls | -- | `fopencookie`, `fmemopen`, `open_memstream`, `open_wmemstream`, `fcloseall`, `fgetln`, `getw`, `putw`, `tempnam`, `fileno_unlocked`, `fgetc_unlocked`, `fgets_unlocked`, `__getdelim`, `_flushlbf`, `__freading`, `__fwriting`, `__freadable`, `__fwritable`, `__flbf`, `__fbufsize`, `__fsetlocking`, `fpurge`, and the wide `_unlocked` forms |
| removed | exported data symbols `BUFSIZ`, `FILENAME_MAX`, `_IOFBF`, `_IOLBF`, `_IONBF` (C macros, never symbols) and `stdio_rename` | -- |

**How it was found:** following the `fopen` mode bug into the rest of
`stdio.rs`.  Found by reading, not by a failure.

**Where:** `posix/src/stdio.rs` (rewritten); `wchar.rs` (the wide calls go
through `stdio::WideStream`); `printf.rs` (a stream is held for a whole
`printf`); `crt.rs` (`exit`); `process.rs` (`fork` holds the stream list);
`stdlib.rs` (`tmpfile` closes its descriptor if `fdopen` fails); and the
memory streams, new, in `stdio_mem.rs`.

**Still open:** `fscanf` reads around the stream (next entry), and the
wide `scanf` family does not exist yet.  The ring-3 fixture `services/ctest-stdio` waits on
a rung from lane A (`requests/d-a-run-the-ctest-stdio-fixture.md`).
