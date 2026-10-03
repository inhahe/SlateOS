## `md5sum` and `sha256sum` accepted no options at all — including `-c`, which is why anyone runs them (lane B, 2026-08-24)

**In short:** both programs took a list of file names and printed a hash for
each, and that was the whole of it. There was no `--check`, so neither could
*verify* a checksum file — which is the reason a person types `md5sum` in the
first place; the printing half is only useful because the checking half exists.
There was no `--tag`, `-z`, `--status`, `--quiet`, `--strict`,
`--ignore-missing`, `-w`, `--help` or `--version` either: every one of those
was treated as a file name, so `md5sum --check SUMS` did not check anything —
it reported `md5sum: '--check': No such file or directory` and then hashed
`SUMS` itself, printing a line that looks plausible and answers a different
question. Both also read each file entirely into memory before hashing it, and
both panicked outright on a file name holding a byte that is not valid UTF-8.
Rewritten from GNU coreutils 9.4's `src/digest.c` and checked against the real
GNU binaries, 113 cases each.

### Why this one is worse than the size of the diff suggests

The other conversions in this section (`cmp`, `tee`, `echo`) were programs that
did their job and were missing options around the edges. These two were missing
*the* option. A checksum utility that can only print is a checksum utility with
no consumer: the output exists to be fed back in later, on another machine, by
`-c`. Without it the only way to verify anything was to re-run the program and
compare the two outputs by eye — which is not a workflow anybody has, and is
why nothing in the tree called these binaries at all.

The specific failure mode is worth stating because it is silent. `md5sum -c
SUMS` in the old version does not fail. `-c` is not an option it knows, so `-c`
becomes an operand, and an operand that does not exist is a diagnostic on
stderr — one line, easily lost in a script — after which `SUMS` is hashed as
data and a normal-looking checksum line goes to stdout. Exit status 1, from the
missing `-c` file rather than from any verification. A script that tested only
for output would conclude the check ran.

### What was missing

1. **`-c`/`--check` — the entire verifying half of the program.** With it, the
   five options that only make sense alongside it: `--ignore-missing`,
   `--quiet`, `--status`, `--strict` and `-w`/`--warn`. Upstream refuses each
   of those *without* `-c`, in a fixed order, and the order is observable when
   two are wrong at once.
2. **`--tag`** — BSD-style `MD5 (name) = <hex>` output, and the corresponding
   ability to read those lines back.
3. **`-z`/`--zero`** — NUL-terminated records, which is what makes the output
   safe to pipe into anything when a file name may contain a newline.
4. **`-b`/`--binary` and `-t`/`--text`.** Genuine no-ops for the byte content
   on a POSIX system — gnulib's `O_BINARY` is `0` — but they still choose the
   `*`/` ` indicator byte in the output, and `--tag` implies `-b`, so
   `--tag --text` is an error while `--text --tag` is fine.
5. **`--help` and `--version`** — neither existed.
6. **Name escaping.** A file called `we<newline>ird` was printed raw, producing
   two lines that no reader can put back together and that `-c` would then
   parse as two separate records. Upstream escapes `\n`, `\r` and `\` and
   marks the record with a single leading backslash; `-z` disables escaping
   because a NUL-terminated record cannot be confused by a newline anyway.
7. **argv was `Vec<String>`.** `env::args()` panics on the first byte that is
   not valid UTF-8, so `md5sum $'\xff'` aborted before printing anything —
   in a program whose entire input is file names. Both removed from
   `scripts/argv-utf8-baseline.txt`; 27 findings remain.
8. **The whole file was read into memory.** `read_to_end` then hash. `md5sum`
   on a disk image is an ordinary thing to do and that shape answers it with
   an allocation the size of the input, which past a certain size is not a slow
   answer but no answer. Both hashes are now incremental, fed in 64 KiB reads.

### The three rules that would not have survived being guessed

Each of these was measured against the GNU binary before it was written, not
recalled:

* **A checksum file may use `<hex>  NAME` or BSD-reversed `<hex> NAME`, but not
  both in one file.** This reads as tidiness and is a security rule. In the
  reversed layout the name begins immediately after one space; so a reversed
  line naming a file whose name starts with a space or `*` is also a
  well-formed *standard* line, naming a different file. Upstream latches the
  layout on the first line that settles it and rejects the other kind
  thereafter. A parser that simply tried both would verify the wrong file and
  report `OK`.
* **The escaping is announced by a leading backslash on the record, not
  detected.** `filename_unescape` refuses a trailing lone backslash, a `\`
  before anything but `n`, `r` or `\`, and an embedded NUL — so a name that
  merely *contains* a backslash round-trips as `\\` and a name that was never
  escaped keeps its backslashes literally. Getting this backwards turns every
  Windows-style path in a checksum file into a different name.
* **Four failure kinds have four different exit rules.** Mismatched, missing,
  unreadable and improperly-formatted are counted separately; `--strict`
  promotes only the last of them to a non-zero exit, `--ignore-missing`
  demotes only the second, and `--ignore-missing` with *nothing* verified is
  its own diagnostic (`no file was verified`) rather than a silent success.

### What the differential harness found

`scripts/digest-diff.sh` is one harness that runs its whole case list once per
program, because upstream is one file compiled eight times and ours is one
module parameterised by an `Algorithm` constant. Nothing in it hard-codes a
digest width; the two cases that would have (a deliberately wrong checksum, and
a digest one hex digit short) derive theirs from the reference binary's own
output at startup, so a future `sha1sum` is one word in `PROGS` away.

It covers *both* programs by default rather than needing `PROG=` to reach the
second, which is not a convenience: `scripts/all-diff.sh` finds harnesses
through a glob and cannot pass one an argument, so a harness that named two
programs and silently examined one would have reported green for a binary it
never ran. It also reads only `tail -1` and calls a harness green by matching
`" 0 differed"`, so the per-program lines are indented and the totals are
printed once at the end — leaving the last program's own line as the tail would
have hidden a difference found in the first.

**113 passed, 0 differed, 2 differ on purpose** for each of `md5sum` and
`sha256sum`; `226 passed, 0 differed, 4 differ on purpose` in total. The two
are `--help`, which omits the GNU project's ancillary block (bug-report
address, home page, `info` reference) — the preceding 28 lines are
byte-identical — and `--version`, which names SlateOS. Everything else agrees:
all three output formats, the escaping, every option error and prefix
ambiguity, and the whole of `--check` including the layout latch, the
`WARNING:` lines with their singular/plural forms, and the exit statuses.

Run `OURS=/usr/bin ./scripts/digest-diff.sh` to confirm the harness still
discriminates; it reports all four xfails as XPASS and nothing else, which is
what says the remaining 226 are comparisons and not coincidences. (`OURS` names
a *directory*, as in every family harness here, so one run covers both
programs. It named a single binary until 2026-08-25, which meant it also needed
a `PROG=` beside it to say which — and so the discrimination check could only
ever cover one of the two programs at a time.)

### What our own tests caught that the harness could not have

The streaming rewrite introduced a bug the differential harness would have hung
on rather than reported: `Md5::absorb` assigned `used` from the tail remainder
unconditionally, so topping up a partial block *without filling it* threw the
partial block away and left `used` at 0. `squeeze` pads with "one zero byte at
a time until `used == 56`", which under that bug never advances — an infinite
loop reachable from `md5sum` with no arguments. It was caught by
`byte_at_a_time_agrees_with_one_shot` and by
`every_split_of_a_multi_block_message_agrees`, which hash one message at all
201 possible two-way splits; the padding path is now commented with why the
early return is correctness and not an optimisation.

That is the argument for those tests existing at all. A one-shot hash cannot
get chunk-boundary handling wrong, so converting to a streaming one adds a
whole failure class that the RFC 1321 vectors — every one of which is a single
`update` — cannot see.

### The two gates that stopped seeing these programs when the code moved

Both were caught by the pre-push hook on the first attempt to push this, and
both are the same shape: a check that reads *bins* went blind the moment the
thing it reads moved into a module two bins share. Neither was a false alarm to
be silenced.

`scripts/getopt-ambiguity-check.py` scanned `src/bin/**.rs` for a
`LONG_OPTIONS` table and, finding none in either bin, printed `md5sum has no
LONG_OPTIONS table … (not yet converted to coreutils::getopt?)` and reported
`0 table(s) checked`. That note is *reassuring* and was wrong: the table exists,
it had simply moved to `src/digest.rs`. The fix follows the delegation — a bin
whose `main` is `coreutils::<module>::main(…)` is read from `src/<module>.rs`
instead — and keeps the finding named for the **bin**, so the shared table is
compared against `md5sum`'s GNU binary *and* `sha256sum`'s. That is not
redundant: it is the only thing that would catch a table right for one
algorithm and wrong for the other. Both now check clean against real GNU:
`2 table(s) checked; 0 disagreement(s)`.

`scripts/host-errmsg.py` flagged `eprintln!("{}: {e}", algo.program)`. Its
exempt shape is a literal that is exactly `<name>: {e}` — the "the error is the
whole message" print, which in every bin carries a `getopt::Error` and never an
`io::Error`. Ours is semantically that, but with the name as *data*, so the
literal is `"{}: {e}"` and the gate cannot tell it from a real site printing
the host's error text with one word of context. Widening the exemption to
`{something}: {e}` would have admitted exactly the sites the gate exists to
find, so the literal was removed instead: `getopt::Program::report(&e)` prints
`name: <message>`, which is byte-identical and is now the one named place for
that operation. Adding a line to the baseline or to `IGNORE` would have been
the cheaper answer and the wrong one — the gate's complaint was about a shape
it genuinely cannot check, not about this file.

### Where it lives

`userspace/coreutils/src/digest.rs` is the sixteenth shared module, and it is
the strongest form of that crate's argument: the two sides that must agree are
not two utilities but one utility and itself. `--check` parses output this same
program wrote, so a checksum file is a format with two independent
implementations inside one binary, and any drift between them shows up as a
file that verifies as `FAILED` on the machine that produced it.

The bins keep only their hash. `sha256sum` delegates to `userspace/sha2`, now
through the incremental `Sha256` rather than the one-shot `sha256`, and its
FIPS vectors were repointed at the path the program actually takes. `md5sum`
keeps MD5 locally because it has exactly one consumer; it moves out when it has
two, for the same reason SHA-256 already did.
