## `B-TIME-WAS-THE-SHELL-KEYWORD-WEARING-GNU-TIMES-NAME` (lane B, 2026-08-29) -- **FIXED 2026-08-29**

**In short:** `userspace/coreutils/src/bin/time_cmd.rs` used to print
`real 0m1.23s / user … / sys …` -- the shape bash's built-in `time` keyword
prints. But the program a user reaches by typing `/usr/bin/time` is a different
program entirely: GNU Time 1.9, which prints
`1.23user 0.45system 1:15.25elapsed 2%CPU (0avgtext+0avgdata 1152maxresident)k`
and takes options (`-f`, `-o`, `-p`, `-v`, `-a`, `-q`) the keyword has never
heard of. Nothing shared: not one option, not one format directive, not one
line of output. The stub answered to the name and to nothing else.

It also read argv as `String`, so an argument holding a byte that is not valid
UTF-8 -- a legal filename on this OS -- panicked before the child ever ran.

**Fixed by transcribing GNU Time 1.9's `src/time.c`** into 1063 lines, argv
carried as `OsString`/`&[u8]` end to end. `scripts/time-diff.sh` (174 cases,
committed first at `278b32426`) reports **169 passed, 0 differed, 5 differ on
purpose**, and `cargo test --bin time_cmd` runs 52 unit tests.

**The five `xfail`s, and why each is deliberate:**

| Case | Why it must differ |
|---|---|
| `--help` (and the `--hel` abbreviation) | upstream's last three lines refer the reader to `bug-time@gnu.org` and the GNU project's web pages. This is not the GNU project. Everything above those lines is byte-identical, typos included -- "Commonly usaged", the double space in `-h,  --help`, and the literal `real %%e`. |
| `--version` (and `--vers`) | names SlateOS coreutils, as every other bin here does. |
| `-f 'ab\'` | upstream walks **past the format's terminating NUL**: it prints `ab?\`, then the NUL itself, then whatever follows it in memory -- measured as `ab?\<NUL>true`, i.e. it read into `argv[2]`. Ours prints `ab?\` and stops. Reproducing an out-of-bounds read to match its output byte for byte is not a compatibility goal. |

**Three upstream quirks that ARE reproduced, because a script can depend on
them:**

1. **A format ending in a bare `%` loses the closing newline.** `case '\0'` in
   upstream's `%` switch is a `return`, not a `break`, so it skips both the
   trailing `putc ('\n')` *and* the `ferror` check that would have reported a
   write failure. `time -f 'abc%' true 2>/dev/full` therefore exits **0** where
   every other format exits 1.
2. **`-p` suppresses `Command exited with non-zero status N`; `-f` with the
   identical string does not.** `summarize` compares the format *pointer*
   against `posix_format`, not its contents. `Format::Posix` is a distinct enum
   variant here for exactly this reason, and two unit tests pin the split.
3. **`-h` is not an option.** The getopt string is `"+af:o:pqvV"`; only the long
   `--help` reaches `case 'h'`. `time -h true` is an "invalid option" error.

**The stderr/file exit-code asymmetry, which is the subtlest thing here.**
Measured on GNU: `time true 2>&-` → 1, `time /nope 2>&-` → 1 (*not* 127),
`time true 2>/dev/full` → 1, but `time -o /dev/full true` → **0**. The reason is
buffering: stderr is unbuffered, so `summarize`'s per-directive
`if (ferror (fp)) error (1, errno, "write error")` fires immediately; a `-o FILE`
stream is block-buffered, so nothing is written inside `summarize` at all and
the only flush is `main`'s **unchecked** `fflush (outfp)`. The port reproduces
this structurally rather than by special-casing the exit code: `Sink::Standard`
writes straight through, `Sink::File` buffers 4096 bytes.

**Note there is no gnulib `close_stdout` in GNU time** -- `time --help >&-`
exits 0 in silence and `time --help >/dev/full` exits 0. That is the opposite of
`nice`'s rule, so the sibling ports must not be copied here; `run` discards the
help/version stream's `finish()` on purpose, with a comment saying so.

**Where the numbers come from.** Upstream calls `wait3`, which hands back the
reaped child's rusage directly. Rust's `Child::wait` has no such variant, so the
port calls `getrusage(RUSAGE_CHILDREN)` after the single `wait` -- identical as
long as exactly one child is ever reaped, which is the case. On Linux
`RUSAGE_MEM_TO_KB` is the identity (`GETRUSAGE_RETURNS_KB`, confirmed from
`rusage-kb.h`), so no page-size conversion is needed. **On SlateOS itself,
`posix::resource::getrusage` leaves `RUSAGE_CHILDREN` all-zero**, so every
memory and fault directive reads 0 there until the kernel accounts children.
That is a posix/ gap, not a bug in this transcription.

**Gates cleared by the rewrite:** `time_cmd.rs:argv-as-string` out of
`scripts/argv-utf8-baseline.txt` (**11 findings remain across 10 files** --
`diff ed fetch grep logger more patch ps sh tar`, `sh` carrying two; this line
said "10 across 9" until 2026-08-29 -- it counted the *files* and forgot that
`sh` contributes two findings, the same off-by-one commit `631783b54` made.
`python scripts/argv-utf8.py --check` prints the number and is the authority.
It became 10 across 9 for real when `grep` left the baseline, below);
`time_cmd.rs:host-error-text` out of `scripts/host-errmsg-baseline.txt` (8 files
remain). `scripts/getopt-ambiguity-check.py` no longer lists `time_cmd` in
`NOT_GNU` -- GNU does ship the program, just not under our file's name, so the
script gained a `GNU_NAME` override table (`time_cmd` → `time`) and a
`Table.gnu` property. Full sweep: 59 tables checked, 0 disagreements.

**The name is still `time_cmd`, and nothing renames it on installation.** The
old module doc claimed "the binary is installed as `time`"; that was false --
`scripts/create-ext4-rootfs.sh` does not install the Rust coreutils bins at all.
`src/bin/time.rs` would collide with Rust's own `time` crate, which is the only
reason for the suffix. The diff harness symlinks both sides as `time` so that
`argv[0]`-derived text matches.
