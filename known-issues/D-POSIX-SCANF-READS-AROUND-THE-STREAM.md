## D-POSIX-SCANF-READS-AROUND-THE-STREAM — `scanf` and `fscanf` read a whole line from the descriptor and parse only that (lane D, 2026-09-27) — **Status: FIXED 2026-09-27**

**In short:** `scanf("%d", &n)` does not read through the stream's buffer:
it reads one line straight from the file descriptor, a byte per system call,
and parses that line alone.  So `12 34` typed on one line gives the first
`scanf` 12 and throws 34 away; a format whose input spans two lines fails;
and whatever `fgets` or `getc` had already buffered is skipped.  All three
lose data without an error.

**Where:** `posix/src/scanf.rs` -- `vfscanf` takes `fileno(stream)` and calls
`scan_fd_source`, whose `read_line_from_fd` reads to the newline.

**The proper fix:** scan through the `FILE`, one character of lookahead
pushed back with `ungetc` when the conversion stops -- glibc's `inchar` /
`ungetc` in `vfscanf-internal.c`.  The scanning engine reads a
NUL-terminated string today; it needs a source it can pull from and push one
character back to, over a string for `sscanf` and over a stream for
`fscanf`.

**Fixed 2026-09-27** by porting glibc's engine (design-decisions.md §1122):
one engine, reading one character at a time and giving back the one it
looked at too far, over the string for `sscanf` and through the stream for
`scanf` and `fscanf`.  The port also fixed what the old engine got wrong
beside it:

| What | Was | Now |
|---|---|---|
| `%hd`, `%hhd`, `%hn` | stored four bytes, over what followed a `short` or `char` | two, one |
| an integer out of range | wrapped | saturates with `ERANGE`, as `strtol` |
| `0xZ` with `%x`, `1ex` with `%f`, `infinx` | a look-ahead left `xZ`, `ex`, `infinx` | glibc's consumption: `Z` left, `x` left, a matching failure |
| `nan(...)` | payload read | not read -- glibc's scanf reads `nan` only |
| `%[z-a]`, `%[abc` | swapped to `a-z`; accepted | three members; a conversion error |
| NUL in a stream | ended the input | a character |
| new | -- | `%m`, `%p` (and `(nil)`), `%b`, `%lc`, `%ls`, `%l[`, `%C`, `%S`, `%N$`, C23's `w` modifiers |
