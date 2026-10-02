## B-stat-HAS-NO-OPTIONS-AND-CANNOT-READ-A-CLOCK (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** `stat` reports a file's size, owner, dates and so on. Almost
nobody runs it to read the output; they run it inside a script to pull out one
number, which is spelled `size=$(stat -c %s file)` — `-c` meaning "print just
this". The shipped `stat` parsed **no options at all**. It treated `-c` and
`%s` as two more file names, failed on both, and printed the whole
human-readable paragraph for `file` — so `$size` came out as a paragraph. It
also printed the string `0` instead of a date for any file dated 1970 or
earlier, and could be made to spin for minutes on a single file with a
far-future date.

### There was no argument parser

`userspace/coreutils/src/bin/stat.rs` was 343 lines with a `main` that read, in
full:

```rust
for (i, path_str) in args.iter().enumerate() { … show_stat(path_str) … }
```

Every argument was a path. So of GNU's options, **all** of them were broken in
the same way:

| Invocation | What it did |
|---|---|
| `stat -c %s f` | Two errors on stderr, the full block for `f` on stdout, exit 1. A script capturing stdout gets the block. |
| `stat -L link` | Reported a file named `-L` (error) and the *link*, never the target. |
| `stat -t f` | Same; no terse output exists. |
| `stat -f /` | Same; no filesystem mode exists. |
| `stat -- -weird` | No `--`, so a file whose name starts with `-` was unreachable… by luck, reachable, since nothing was an option. |

The standalone twin, `userspace/stat/src/main.rs` (2845 lines, also providing
`touch`/`ln`/`readlink`), has had `-c`, `-L`, `-t` and `-f` all along. This is
the same shape as the four finds before it: the shipped binary is a stripped
reimplementation, and what got stripped was the part scripts use.

### Time formatting was wrong at both ends and slow in the middle

`format_timestamp` began:

```rust
if epoch_secs <= 0 { return "0".to_string(); }
```

so **the epoch itself printed as `0`**, not as `1970-01-01 00:00:00` — and a
file restored from an archive with no recorded mtime gets exactly that stamp.
Every legitimate pre-1970 date printed `0` too.

It then counted forward one year at a time from 1970 to find the year. For a
timestamp far in the future — `touch -d` will set one, and so will a corrupt
inode — that is hundreds of billions of iterations, i.e. **`stat` hangs for
minutes on one file**, at a cost to the attacker of one `touch`. Replaced with
Howard Hinnant's `civil_from_days`, which is exact over the whole proleptic
Gregorian range and runs in constant time. There is a test that the far-future
case returns immediately; if it ever takes measurable time, the loop is back.

Nanoseconds were not printed at all, so two files written a millisecond apart
looked simultaneous — which defeats the most common reason to compare
timestamps in the first place.

### Six smaller defects, all in the same direction

1. **Symbolic links did not show their target.** `stat link` printed
   `File: link` and stopped. Where the link points is the single most useful
   thing about it; GNU prints `File: 'link' -> '/etc'`. Fixed.
2. **The name was never quoted**, although the file imported `quoteaf_os` and
   used it *only in the error path*. A filename containing a newline produced
   output that could not be parsed, which is the whole reason GNU quotes.
3. **Device major/minor used the pre-2.6 encoding** (`rdev >> 8`, `rdev & 0xff`)
   in the standalone's `%t`/`%T` — silently wrong for any minor above 255, which
   on a modern system is most of them. The rewrite uses the current encoding
   and tests minor 300 specifically.
4. **`%N` hand-rolled its quoting** as `'{name}'` in the standalone, so a
   filename containing an apostrophe emitted an unbalanced quote. Now routed
   through the shared quoter.
5. **`%F` had no `regular empty file`**, GNU's one special case and the one
   scripts actually match on.
6. **Nothing checked the write.** Output went through `println!`, which panics
   on a broken pipe rather than exiting — `stat * | head -1` on a large
   directory. Writes are now checked, `BrokenPipe` exits with the accumulated
   status, and the final flush is checked too, which is the same rule
   `B-tee-REPORTS-SUCCESS-AFTER-LOSING-THE-DATA` established.

### What the rewrite adds

`-c FORMAT`, `--format=`, `--printf=` (escapes interpreted, no trailing
newline), `-L`, `-f` (via the posix crate's `statvfs`, the same call the
standalone uses), `-t` with GNU's exact terse field orders, `--`, option
permutation, and rejection of unknown options. Roughly 30 `%` specifiers plus
printf-style widths.

Two structural choices worth recording:

* **The default human-readable block is itself a format string**, expanded by
  the same code as `-c`. Six `println!`s were how the old version came to quote
  names in its errors but not in its output; one code path cannot drift from
  itself. The block is asserted character-for-character in a test.
* **Everything below the syscalls is written over a plain `StatInfo`/`FsInfo`
  struct, not `std::fs::Metadata`.** `Metadata`'s unix accessors are
  `cfg(unix)`, the build host is Windows, and so anything written against them
  is invisible to `cargo test --workspace`. That is precisely how a `stat` with
  no argument parser survived this long. 35 tests now run on the host, covering
  the parser, the calendar, the formatter and the default block; clippy clean on
  `x86_64-pc-windows-gnu` and `x86_64-slateos`.

**Not verified end-to-end** — `stat` is `cfg(unix)`-gated and needs QEMU. Same
gap as `dd`, `tee`, `chown` and `chmod`; the fourth argument in three days for
a filesystem-level harness that runs the shipped binaries inside the OS.

**Still not implemented:** `%U`/`%G` print the numeric uid/gid rather than
looking up names, because name lookup needs `/etc/users.yaml` (§353). Printing
the number is right, just unfriendly; printing a guessed name would not be.

**Six for six.** Every shipped `coreutils` bin read against its bigger
standalone twin so far — `chmod`, `dd`, `tee`, `tar`, `chown`, `stat` — has had
a silent-wrong-behaviour bug.

### Update 2026-08-23 — rewritten again, against GNU 9.4's `src/stat.c` itself

The 2026-08-22 fix bolted an argument parser onto the existing renderer. That
closed the headline defect, but opening the file for the `Vec<String>` argv
conversion (`scripts/argv-utf8.py`) showed the renderer underneath it was still
an approximation of GNU's, in ways a script can see. It is now a port of
upstream's `print_it` / `print_stat` / `print_statfs` / `out_*`, read function
by function, with every expected string measured against real GNU 9.4 before
being written down. Twelve further defects, all fixed:

1. **`main` collected `Vec<String>`** — the backlog item itself. `stat "$f"`
   *panicked* on a name holding a non-UTF-8 byte, which is a legal name here.
   Now `args_os`, with every name and format carried as `&[u8]`/`Vec<u8>`.
2. **No option clustering and no attached values.** The hand-rolled parser
   matched whole words, so `stat -c%s f`, `stat -Lt f` and `stat -tc %s f` all
   failed — and `-c%s` is how the option is nearly always written. Now
   `coreutils::getopt` with GNU's own `"c:fLt"`.
3. **No `--help`, no `--version`, no `--cached=MODE`.**
4. **A format could not hold a non-UTF-8 byte** (it was a `&str`). GNU passes
   such a format through unchanged.
5. **`%U`/`%G` printed the number** — the `TD-B-ID-AND-STAT-…` entry below.
6. **An unknown specifier passed through as itself** (`%Q` printed `%Q`); GNU
   prints `?`, and a format with a typo in it should not silently look like a
   literal.
7. **The width layer implemented `-`, `0` and a width, and nothing else.** GNU
   has the whole `'-+ #0I` flag set, a width *and* a precision, and filters the
   flags per specifier — `%+d` prints no sign because `%d` is unsigned
   upstream, `%#o` on the I/O block size grows no leading zero, `%.3Y` is three
   digits of fraction, `%10.10A` is how the default block's mode column is
   built. All of it is now reproduced, including the awkward one: a **negative**
   epoch with a fraction borrows, so `-1.5s` prints `-1.500` and `-0.5s` prints
   `-0.500` with a sign on a zero.
8. **`--printf` knew five escapes.** It now has GNU's full table plus octal,
   `\xHH` (only when a hex digit follows, so `\xzz` is the *unrecognized escape*
   `\x`), and the two warnings — `unrecognized escape` and `backslash at end of
   format` — neither of which changes the exit status, as measured.
9. **`-` was a file called `-`.** It is standard input, `fstat`ed on fd 0; and
   in `-f` mode it is the specific error GNU gives for it.
10. **`%m`, `%r`, `%R`, `%Hd`, `%Ld`, `%Hr`, `%Lr` and `%C` did not exist.**
    `%m` is a parent-directory walk on `st_dev` (there is no mount table),
    checked against GNU's answers for `/tmp/x`, `/proc/1/cmdline` and
    `/dev/null`.
11. **The default block was the pre-9.x shape** (`Device: %Dh/%dd`,
    `Size: %-15s`). It is now 9.x's, byte for byte, including the wider variant
    with `Device type:` that a block or character device selects.
12. **Times were hard-coded to UTC** with a literal `+0000`, on the reasoning
    that no timezone database existed. `userspace/localtime` had existed for
    some time and `ls -l` was already using it; `stat` now renders through the
    same `%Y-%m-%d %H:%M:%S.%N %z` template upstream hands `nstrftime`.

Two deliberate divergences, both recorded in `design-decisions.md` §371–§372:
`%N` is **always** quoted (upstream only consults `QUOTING_STYLE` when the user
wrote the literal two characters `%N`, which is why GNU's own `File:` line
prints a raw newline for a name containing one — and why GNU's `%.3N` is
*unquoted*, measured); and `%C` prints a silent `?` rather than reporting a
failed security-context lookup on a system that has no such concept.

40 host tests, clippy clean on `x86_64-pc-windows-gnu` and
`x86_64-unknown-linux-gnu`. Still not verified end-to-end — same QEMU gap as
above.
