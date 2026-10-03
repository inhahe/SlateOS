## TD-COREUTILS-LONG-OPTIONS-DO-NOT-ABBREVIATE (lane B, 2026-08-16) — **open: one program left, `kill`, frozen by B-Q22**

**Status:** OPEN -- re-measured 2026-10-03, and down to one program. Every
other program in `userspace/coreutils/src/bin/` parses through the shared
`getopt` (directly, or through its library module: the digests, `basenc`,
`ls`, `pgrep`), and the sixteen whose GNU originals use gnulib's
`parse_long_options` or `parse_gnu_standard_options_only` instead were
compared with GNU 9.4 on abbreviated `--help`/`--version` (208 cases). One
differed: `expr`, which matched its two options whole where gnulib accepts any
unambiguous prefix of a lone argument -- fixed the same day, with cases in
`scripts/expr-diff.sh`. The one program left is **`kill`**, which still
matches `--signal`, `--list` and `--table` whole; it is not converted because
open question B-Q22 is deciding which `kill` the system keeps, and the answer
decides what there is to convert.

**In short:** GNU lets you shorten a long option to any unambiguous prefix —
`cat --squeeze` means `--squeeze-blank`, `ls --col` means `--color`. Ours accepts
only the full spelling, so ordinary commands that work everywhere else are
refused here. `sort` was fixed (see above); the other 84 utilities were not,
because each parses argv by hand.

Measured, our `cat` against glibc's:

| command | glibc | ours |
|---|---|---|
| `cat --squeeze` | squeezes blank lines | `unrecognized option '--squeeze'` |
| `cat --show-a` | shows all | `unrecognized option '--show-a'` |
| `cat --num` | `option '--num' is ambiguous; possibilities: '--number-nonblank' '--number'` | `unrecognized option '--num'` |

Note the third row: getting abbreviation right is not only about accepting more,
it is about *refusing* the ambiguous ones with the right sentence. A utility that
accepted the shortest unique prefix per its own table would still be wrong
whenever its table differs from GNU's, which is why the table's contents and
order both matter (see the entry above).

**What the fix looks like.** Not 84 hand-written parsers. `sort` now contains a
correct, measured `getopt_long`: the declaration-ordered option table, exact
match beating prefix match, the five diagnostics, and `argmatch` for option
arguments. That belongs in `userspace/coreutils/src/` beside `quote.rs` as a
shared module, with `sort` as its first caller, and then each utility converted
to it. The module is the deliverable; converting all 84 is mechanical after it.

**Not urgent, but it gets worse with time.** Every utility that gains a long
option without the shared module is another hand-written parser to convert. The
current behaviour is safe — it refuses valid commands rather than
mis-interpreting them — so nothing is at risk except compatibility.

### Progress (appended 2026-08-16)

The module landed as `userspace/coreutils/src/getopt.rs`
(`ebb72b9a3`, `8492d4d78`), and `sort` and `cat` (`8956816e4`) call it. Every
row of the table above now behaves as glibc does, verified by `cat-diff.sh`'s
new `run_getopt` section — 15 option cases compared to glibc for stdout,
stderr *and* status, 95 passed / 0 differed.

Two things measured during the conversion changed the module's shape, and both
are traps for the remaining 83:

- **The usage exit status is per-utility, not a constant.** Measured across 28
  utilities it is **1** for almost all of them and **2** only for `ls`, `sort`
  and `grep` — the three that already gave 1 a meaning (`sort -c` found the
  input unsorted; `grep` matched nothing). The draft module hardcoded sort's 2,
  which would have silently changed `cat`'s exit status. It is now
  `Program::new(name, usage_status)`, and `Program::new` has no default because
  the value that would be wrong is not the rare one. **Measure it per utility:**
  `<util> --zzz-bogus; echo $?`.
- **`argmatch` overrides that status to 1 for everybody.** In the same program,
  `ls --zzz` is 2 but `ls --sort=zzz` is 1. The module encodes this; a caller
  cannot get it wrong.

Also measured: a utility's *own* usage errors (`sort -k0`) take its usage status
but carry **no** `Try '… --help'` referral — only getopt's and argmatch's do.
That is `Program::usage()`, separate from the getopt sentences.

**Converting the next utility** is two measurements and a mechanical edit:

1. `<util> --=x` under glibc — an empty prefix matches everything, so the
   ambiguity list prints the whole `struct option[]` **in declaration order**,
   which is the order the `LONG_OPTIONS` table must copy (the order is
   observable output, not an implementation detail).
2. `<util> --zzz-bogus; echo $?` — the usage status.
3. Replace the hand parser with `Program::resolve_long` / `argmatch`, and check
   for the `other as char` bug while there: both `sort` and `cat` had it, and it
   reports an option nobody typed (0xC3 rendered as `Ã`, then re-encoded as two
   bytes). Iterate short-option bytes, not chars.

### Progress (appended 2026-08-17)

`wc` is the third (`scripts/wc-diff.sh`: 113 passed, 0 differed, 3 differ on
purpose). It was not a table swap — the whole front end was rewritten — and it
added three traps to the list above:

- **Some usage errors *do* keep the referral.** `wc --files0-from=- FILE` prints
  `Try 'wc --help' for more information.` where `sort -k0` prints no such line.
  The difference is in the upstream call: `error (0, …)` followed by
  `usage (EXIT_FAILURE)` prints it, `error (EXIT_FAILURE, …)` does not. That is
  now `Program::usage_referring()` beside `Program::usage()`, and **which one a
  diagnostic uses is per-message, not per-utility** — read the call site.
- **Measurement alone is not always enough; read the upstream source.** `wc`'s
  column width looks like a fixed 7 and is not: it is the digit count of the sum
  of the operands' *sizes*, except that a lone count of a lone input is exempt
  and prints bare, and any input that stats but has no size (a pipe, a terminal,
  a directory) forces 7. Four rules inferred from the harness each fit every case
  measured to that point and each broke on the next; `coreutils-9.4/src/wc.c`
  (`get_input_fstatus`, `compute_number_width`) settled it in one read. When a
  utility's output depends on something the output does not show — here `S_ISREG`
  of every operand, and whether the `--files0-from` list was small enough
  (10 MiB) to slurp, which is the *only* reason `--files0-from=-` on a pipe pads
  differently from `--files0-from=FILE` — fetch the source rather than infer.
- **The harness runs a Windows build, and `Metadata::is_file()` lies there.** On
  Windows it means "not a directory and not a symlink", so an MSYS pipe answers
  *yes* and reports a length of whatever is buffered in it. Every `S_ISREG`
  question in a converted utility needs the host analogue — `GetFileType`, where
  only `FILE_TYPE_DISK` is a regular file — or the harness will certify a rule
  that is wrong on the target. `wc.rs` → `Stat` is the shape to copy: the three
  answers are `Failed`, `Regular(len)` and `Other`, and conflating the first two
  with the third is exactly the bug this caught.

One divergence found here is not `wc`'s and affects all 85: under a UTF-8
locale GNU quotes option arguments with `‘…’` rather than `'…'`. Confined to
`quote()`/`argmatch` — `quotef`, `quoteaf` and getopt's own sentences are ASCII
in every locale. Left as-is and queued for the operator: `open-questions.md` →
**B-Q2**.

`head` is the fourth (`scripts/head-diff.sh`). It is the first conversion where
the getopt swap was the *smaller* half: the parser it replaced knew `-n` and
nothing else — no `-c`, no `-q`/`-v`, no `-z`, no negative count, no multiplier
suffix — and it silently substituted 10 for any count it could not parse, so
`head -n 5O f` (letter O) printed ten lines rather than saying so. It also read
input as UTF-8 `String` lines, which reported a non-UTF-8 line as an I/O error
and truncated the file there, and re-emitted `\r\n` as `\n` because
`BufRead::lines` strips the carriage return. Four more traps for the remaining
81:

- **A utility may have a second option syntax that getopt never sees.**
  `head -3 f` is the pre-POSIX form, and upstream parses it by hand off `argv[1]`
  alone, *before* `getopt_long` runs. The position rule is observable and nobody
  would guess it: `head -3 -q f` works, `head -q -3 f` answers
  `invalid trailing option -- 3`. It falls out of the digits `0123456789` being
  listed in the short-option string, so a digit reaching getopt at all proves it
  was not first. `tail` has the same form; check for one before assuming argv is
  getopt's alone.
- **The obsolete form's letters are not flags.** In `head -2k`, `k` is a
  *multiplier suffix* appended to the digits before parsing, so it means 2048
  lines. `c` selects bytes and clears the multiplier; `l` selects lines and does
  **not** clear it, so `-2kl` is 2048 lines. Reproduced rather than tidied.
- **Numeric arguments go through gnulib's `xstrtoumax`, which is a specification
  in itself.** Leading whitespace and `+` accepted, trailing whitespace not; a
  bare suffix means one of it (`head -n K` is 1024) but only if it is the very
  first byte, so `head -n ' K'` is an error; a second suffix switches the base
  (`1K`=1024, `1kB`=1000, `1KiB`=1024, a lone `1Ki` invalid); and **a bad suffix
  outranks an overflow**, so `head -n 99999999999999999999X` reports an invalid
  number rather than a value too large. The caller's own suffix list narrows
  gnulib's, which is why `head -n 1w` is refused though `w` exists in the same
  function. `head.rs` → `parse_count` is the shape to copy; `tail`, `split`,
  `fold`, `cut` and `nl` all reach the same code upstream.
- **Streaming is a correctness requirement, not an optimisation.** `wc` reads
  each input whole; `head` must not, because `yes | head -n1` has to terminate.
  The two eliding forms (`-n -N`, `-c -N`) do have to see the end, and buffer
  only the tail they might still drop. Note the rule the streaming loop cannot
  apply until EOF: an unterminated final line *counts* as a line for `-n -N`
  (so `printf 'a\nb' | head -n -1` prints `a\n`) but is never itself printed.

`tail` is the fifth (`scripts/tail-diff.sh`: 217 passed, 0 differed, 3 differ on
purpose), and the largest so far: the parser
it replaced knew `-n` and nothing else, and the feature it did not have *at all*
was `-f`. Three of its lessons generalise, and one of them is a bug in shipped
code that the previous entry described but did not fix.

- **The Windows pipe lie is now a shared module, and it corrupts bytes, not
  just widths.** The `wc` entry above recorded that `Metadata::is_file()` means
  "not a directory and not a symlink" on Windows, so an MSYS pipe answers *yes*.
  `tail` showed what that costs when the answer selects an *algorithm* rather
  than a column width: the pipe also reports the bytes currently buffered in it
  as a length and returns success from a seek that moves nothing, so
  `printf 'a\nb\nc\n' | tail -n3` took the backwards block scan, read the pipe
  dry hunting for a fourth line, and printed **nothing**; `tail -c3` printed the
  **whole file**. Both were measured against the shipped binary. The question is
  now `coreutils::filekind` — `regular()` (three-valued: yes / no / could not
  tell), `is_regular()`, and `is_seekable()`, which additionally performs the
  seek and reads the position back, because a handle can accept a seek and
  ignore it. `wc` was moved onto it in the same change. **Every remaining
  utility that takes a shortcut for regular files must call it rather than
  `Metadata::is_file()`** — `cat`, `cp`, `split`, `od` and `truncate` all ask
  the same question.
- **A harness that runs a following utility needs `timeout -s KILL`, and must
  normalise the status.** MSYS `timeout` sends SIGTERM, and Cygwin can only
  deliver a signal to a *native* Windows child by `TerminateProcess`, which it
  does for SIGKILL alone — so `timeout 3 ./tail.exe -f f` does not expire, it
  hangs forever. It was measured hanging for five minutes before the run was
  killed by hand. `timeout -s KILL 3` works, but then the two shells disagree
  about how to report it: MSYS bash gives **137**, `wsl.exe` gives bare **9**,
  and neither is `timeout`'s usual 124 — so the harness folds all three
  together (`norm_rc`) or every `-f` case shows a spurious status difference.
- **`quote()` and `quoteaf()` are not interchangeable, and picking the wrong one
  is invisible until the input contains a quote.** gnulib has three styles:
  `quote()` escapes the way C does, `quotef()` and `quoteaf()` the way a shell
  would. They agree on every string holding neither a quote nor a backslash,
  which is why shipped `head` echoed a bad `-n` argument with the wrong one and
  no harness case noticed — `head -n "a'b"` must answer `'a\'b'` and said
  `"a'b"`. Fixed here, with six cases added to `head-diff.sh` that can tell the
  styles apart. **Which style a message uses is a property of the upstream call
  site**: `xdectoumax`, `xstrtod` and `argmatch` use `quote()`; a file name in
  an I/O error uses `quoteaf()`.

`tail`'s own shape, for whoever converts the next utility with a pre-POSIX
option form: the obsolete word is stricter than `head`'s. `head -3 f` needs
only to be first, but `tail -3 …` requires the *whole* command line to be one
of three shapes (the word alone; the word and one non-option; the word, `--`,
and at most one more), and a digit reaching getopt at all produces
`option used in invalid context -- 3` — where `head` says
`invalid trailing option -- 3`. The two utilities do not share the sentence.
Within the word, `b` is both a unit and a ×512 multiplier, applied in two
different places: `tail -b` is 5120 *bytes* (the default scaled), `tail -2b` is
1024 (the digits handed to `xstrtoumax` with `"b"` as the suffix list).

`cut` is the sixth (`scripts/cut-diff.sh`: 238 passed, 0 differed, 3 differ on
purpose). Its parser was not merely missing options — its *data structure* could
not express the answer, which is the trap worth carrying forward:

- **A hand-written parser can be wrong in the shape of what it stores, not just
  in what it accepts.** Shipped `cut` kept the selection as a `Vec` of expanded
  indices, so `cut -f2-` — every field from the second on — parsed to the single
  index 2, and `cut -b1-` printed one byte. No amount of adding options fixes
  that; the list has to be a list of *ranges* with an open-ended marker
  (`u64::MAX` here, as upstream uses `SIZE_MAX`). Before converting a utility,
  check that the representation can hold what the grammar can say.
- **Ranges that *overlap* merge; ranges that merely *touch* do not.** `-b1-2,3-4`
  and `-b1-2,2-4` select the same four bytes and are not the same selection:
  with `--output-delimiter=.` the first prints `ab.cd` and the second `abcd`.
  The delimiter goes between *ranges*, not between bytes, so the merge rule is
  observable. Nothing in `cut --help` hints at this and no black-box probe
  suggests looking for it; it came out of reading `set-fields.c`. The
  complement (`--complement`) has the mirror rule — it must not emit an empty
  gap between two touching ranges.
- **`-c` selects bytes.** GNU 9.4 falls `--characters` straight through to
  `--bytes`, so `cut -c1` on a two-byte character prints half of it. The only
  thing `-c` changes is the wording of the diagnostics (`invalid byte/character
  position` rather than `invalid field value`), which is per-mode and has three
  distinct spellings. `-n` — documented as "do not split multi-byte characters"
  — is accepted and does nothing at all, including where it would matter.
- **Post-loop cross-checks have an order, and the order is observable.**
  `cut -b1 -d: -s` reports the delimiter, not the suppression, because upstream
  tests them in that sequence; `cut -d: -s` with no list at all reports the
  missing list first. A conversion that validates as it parses gets a different
  sentence from the same command line. Port the checks where upstream put them —
  after the option loop, in upstream's order.

`cut`'s reading loop is a port of C's `getc`/`ungetc`/`getndelim2` rather than
anything `BufRead` offers, because the one-byte lookahead is load-bearing in two
places: `-d $'\n'` (where the delimiter *is* the terminator, and a trailing
newline must end the input rather than open another field) and the
`buffer_first_field` fork, where whether field 1 is selected decides which of
two code paths runs. `Input` in `cut.rs` is that shape.

`uniq` is the seventh (`scripts/uniq-diff.sh`: 273 passed, 0 differed, 3 differ
on purpose). Its lessons are all about the parser's *inputs* — what argv is
parsed against, and what argv is allowed to be:

- **The environment is part of the grammar, and a harness that varies only argv
  certifies half the parser.** `uniq` reads two variables by hand.
  `POSIXLY_CORRECT` makes the first operand end option parsing, so
  `POSIXLY_CORRECT=1 uniq f -c` treats `-c` as the output file — and note that
  getopt is *not* doing this: `uniq`'s optstring starts with `-`
  (RETURN_IN_ORDER, which is how operands come back interleaved with options),
  and that mode disables getopt's own `POSIXLY_CORRECT` handling, so the utility
  re-implements it. Separately, `_POSIX2_VERSION` in `[200112, 200809)` disables
  the `+N` form below. `uniq-diff.sh` grew an `ENVV` array applied to *both*
  sides for this; copy it for any utility that reads the environment at parse
  time (`ls`, `sort`, `df`, `du` and `tail` all do).
- **An obsolete form can be an *operand*, not an option.** `head`'s and `tail`'s
  pre-POSIX words at least look like options; `uniq +5` is intercepted at the
  point where a *file name* would be taken, so it never reaches getopt at all,
  and the same word is a perfectly good file name when any of its three
  disqualifiers apply (strict POSIX2, a non-numeric tail, or overflow). The `-N`
  half has a trap the other two do not: digits **accumulate across arguments**,
  so `uniq -1 -2` skips *twelve* fields, and `-f` resets the accumulator —
  meaning `-1 -f3` is 3 and `-f3 -1` is 31. Order-dependent state in the option
  loop is the thing to look for; `-D` resetting `--all-repeated`'s delimiting
  method is a second instance in the same utility.
- **The second operand is an OUTPUT file, and it is truncated.** This is the
  first utility here that writes to a name on its own command line, and it cost
  a fixture during measurement — a probe of `uniq +1 c.txt` emptied `c.txt`
  before anything read it. A harness must give each side its own scratch output
  name and compare the two *files*, distinguishing "no file was created" from
  "an empty file was created": `run_outfile` in `uniq-diff.sh`. Check for this
  before writing cases, not after — `tee`, `split` and `csplit` are next.
- **Reproduce upstream's dead code; do not tidy it.** `uniq.c` has two branches
  that cannot execute — the `too many repeated lines` error (guarded by
  `count_occurrences`, which at that point is enum value 0) and the
  `grouping && countmode` cross-check (unreachable because `-c` sets
  `output_option_used`, so the earlier check always fires first). Both are ported
  with comments saying why they are dead. The reason is not fidelity for its own
  sake: "fixing" one of them changes which sentence a *real* command line
  produces, and the next reader cannot tell a deliberate divergence from a
  mistake.

`uniq`'s three invalid-number diagnostics (`-f`, `-s`, `-w`) quote nothing at
all *and* carry no `Try '… --help'` referral — a third combination beyond
`Program::usage()` and `Program::usage_referring()`, and one more reason to read
the upstream call site per message rather than look for a per-utility rule.

`nl` is the eighth (`scripts/nl-diff.sh`: 222 passed, 0 differed, 5 differ on
purpose — three regex ones and `--help`/`--version`). It had the largest
semantic gap of any conversion so far: the shipped parser knew `-b` and `-w`,
silently *ignored* every other flag, defaulted `-b` to `a` where GNU defaults to
`t`, and had no section machinery at all — so `nl` on a file with `\:\:\:`
delimiter lines numbered them as text. Four more traps:

- **A diagnostic is a sentence plus, separately, a referral — and `nl` is the
  utility that makes the difference visible.** Upstream `getopt_long` prints only
  the sentence and returns `'?'`; the `Try '… --help'` line comes later from the
  caller's own `usage (EXIT_FAILURE)`. Nearly every utility calls `usage` on the
  spot, so the two always appear together and read as one message — which is how
  `getopt::Error` modelled them, as a single `message` string. `nl`'s option loop
  sets an `ok` flag and keeps going, so `nl -Z -bX` prints **two** sentences and
  **one** referral, in argv order. `Error` is now `{ sentence, referral, status }`
  with `message()` joining them; five converted utilities had been splitting the
  referral back off by hand (`e.message.split_once("\nTry '")`), which is the
  usual sign that one field was two things. **A getopt error is not necessarily
  fatal to parsing** — check whether the utility's `default:` case exits or only
  clears a flag.
- **Out-of-range splits by *direction*, not by which limit was hit.** gnulib's
  `xdectoint` sets `errno = min <= tnum ? EOVERFLOW : ERANGE`, so `nl -w 0` says
  `Numerical result out of range` while `nl -w 2147483648` — over the caller's
  own `INT_MAX`, nowhere near `intmax_t` — says `Value too large for defined data
  type`, the same as a genuine `intmax_t` overflow. The natural implementation
  (over/under the caller's range → ERANGE, past `intmax_t` → EOVERFLOW) is wrong
  in exactly one quadrant, and only a case that exceeds a *small* ceiling can
  catch it.
- **An option's argument can be copied over the front of the old value rather
  than replacing it.** `nl -d abc -d x` leaves the delimiter `xbc`, because
  upstream writes one or two bytes through a `char *` that may still point into
  the previous `argv` string. No amount of black-box probing suggests looking for
  this; `coreutils-9.4/src/nl.c` did. The same read settled the blank-line
  counter being `static` and so surviving a section change.
- **Pin every `-w` in the harness.** `nl -w 2147483647` really does emit two
  gigabytes of spaces per line; one unpinned probe produced a 2 GB transcript.
  The width bounds are tested through their diagnostics instead.

`nl -bp` needs backreferences, which `userspace/ere` does not have — three
harness cases are `xfail` for it. The empty BRE is a second `ere` divergence and
is handled in `nl` instead: `ere` refuses an empty pattern on purpose (bash's
`[[ x =~ "" ]]` is status 2) while glibc's `re_compile_pattern` accepts one and
matches everywhere, so `Style::Matching` carries an `Option<Regex>` whose `None`
is the empty expression.

`expand` is the ninth (`scripts/expand-diff.sh`: 205 passed, 0 differed, 3
differ on purpose — a directory operand, `--help`, `--version`). It brought the
first *shared* module between two utilities rather than all of them:
`userspace/coreutils/src/tabstops.rs`, a port of `expand-common.c`, which
`unexpand` will call too. The old parser recognised only `-t N`, read the file
through `lines()` as UTF-8 (corrupting every non-UTF-8 byte), used
`unwrap_or(8)` so `-t bogus` silently became eight, and exited 0 no matter what
went wrong. Four traps:

- **A utility that emits padding will emit as much as you ask for, and a
  harness case is not safe merely because it is valid.** `expand -t
  18446744073709551615` is accepted by GNU and by us, and turns one tab into
  2**64-1 spaces at ~11 MB/s. Two orphaned processes leaked 109 GB of temp
  files before this was noticed. The fix is general, not per-case: **every**
  invocation in `expand-diff.sh` runs under `timeout -k 2 30`, both sides. This
  is the same class as `nl -w 2147483647` one conversion earlier, so treat it
  as a standing rule when converting any utility that pads — `fold`, `pr`,
  `printf`, `seq`, `yes`.
- **The obsolete digit form is a different mechanism in `expand` and in
  `unexpand`, though it looks like the same feature.** `expand`'s short string
  is `"0::"`…`"9::"` — ten options with *optional* arguments — and it recovers
  the whole cluster with `parse_tab_stops (optarg - 1)`, pointer arithmetic
  back onto the digit itself, so `-4,8` is one list. `unexpand`'s is
  `",0123456789at:"`: eleven **no-argument** options that accumulate a number
  one digit at a time, with `,` flushing it, so `-1,3` is two stops and `-12`
  is one at twelve. Porting the first to the second would be wrong in a way no
  black-box probe of `-t` would ever reveal.
- **A tab-stop diagnostic preempts every option after it**, because upstream's
  `parse_tab_stops` calls `error` where it is found, inside the option loop —
  so `expand -t x -Z` reports the bad tab stop and never mentions `-Z`. Ours
  had to defer `finalize()` to after the loop while reporting *parse* errors in
  place, since the ascending/zero/`+`-vs-`/` checks are the ones upstream also
  defers to `finalize_tab_stops`.
- **`-i` is evaluated against the byte as rewritten, not as read**, and
  backspace is not blank. Upstream sets `convert &= entire_line || c == ' ' ||
  c == '\t'` *after* a tab has already become a space, and `\b` falls through
  that test to end the leading run while also rewinding both the column and the
  index into the stop list — so `printf 'abc\b\tx\n' | expand -t 2,4` yields
  two spaces, not one. A unit test asserting the intuitive answer failed; GNU
  was right and the test was wrong.

`unexpand` is the tenth (`scripts/unexpand-diff.sh`: 234 passed, 0 differed, 3
differ on purpose — the same directory operand, `--help`, `--version`). It is
the first utility converted that wrote *no* new parsing of its own: the whole
tab-stop half came from `tabstops.rs`, which `expand` had landed one conversion
earlier, and the option half from `getopt.rs`. The old parser recognised `-a`,
`-t N` and nothing else — no `--first-only`, no obsolete digits, no `-t` list,
and it read the file as UTF-8 lines. Three traps, all of which needed the
upstream C source rather than a black-box probe:

- **An allocation can be observable through the *order* of two error
  messages.** Upstream sizes its pending-blank buffer with `xmalloc
  (max_column_width)`, but only *after* the first operand has been opened
  (`if (!fp) return;` precedes it). So `unexpand -t 18446744073709551615
  nosuch` reports the missing file and `unexpand -t 18446744073709551615
  empty` — a readable, *empty* file — reports `memory exhausted` with status
  1. Ours had no `memory exhausted` at all until this was measured; the fix
  was `Vec::try_reserve_exact` plus moving construction of the converter
  inside the `if input.advance()` arm, which also makes the unit test instant
  because the reservation fails without ever touching the allocator.
- **A fixture can agree with the wrong parser.** The obsolete digit form
  accumulates across the *whole* command line, so `-1 -2` is one stop every
  **twelve** while `-1,2` is stops at 1 and 2. On the eight-column data every
  other case used, those two readings produce identical output — a harness of
  380 cases would have certified a parser that read the digits as `expand`
  does. `twelve.txt` exists solely to reach columns 12 and 24 and separate
  them. Generalise: when two candidate readings of an option differ only at
  certain column/size values, a fixture must be built to *hit* those values,
  or the harness proves nothing about that option.
- **Four unit tests failed and GNU was right in all four.** `-a` on eight
  blanks after `a` gives `a\t b`, not `a\t\tb` (the eighth blank starts a run
  toward 16 that never arrives, and a lone blank sitting on a stop stays a
  blank). `\b` is not blank, so by default it ends the leading region and the
  line is emitted unchanged. `-a -t 2,4` on seven blanks emits two tabs — one
  flushed from the pending buffer, one for the arriving blank — then leaves
  everything past column 4 alone. Writing the expectation from intuition and
  the code from the C source, then reconciling, is what caught these; writing
  both from intuition would have produced a self-consistent wrong utility.

`fold` is the eleventh (`scripts/fold-diff.sh`: 272 passed, 0 differed, 5 differ
on purpose — the directory operand, `--help`, `--version`, and the two
*abbreviations* of the latter pair, which are xfails rather than plain cases so
that what they certify is "`--he` resolves at all", not the help text we already
know differs). The old implementation was wrong in six ways a one-line command
would show — a column was always a byte, input was decoded as UTF-8 through
`lines()` (so a non-UTF-8 file stopped at the first bad byte, CRs vanished, and
an unterminated last line *gained* a newline), `-w bogus` silently became
`-w 80` via `parse().unwrap_or(80)`, `-w 0` meant "don't wrap" where GNU refuses
it, every exit was 0, and `-b`/`--width=N`/`-40`/`--` were all read as
filenames. Four things worth carrying forward:

- **Measuring the *next* utility found a bug in the *last* one.** Probing
  `fold --width 5` to establish its grammar is what exposed that `expand
  --tabs 4` and `unexpand --tabs 4` — shipped an hour earlier — printed
  `option '--tabs' requires an argument`. A long option with a *required*
  argument takes the next word when there is no `=`; both files had been
  written from the opposite belief, and *both harnesses lacked the row that
  would have caught it*, so the harnesses were part of the fix (commit
  `2a70349c7`). Every other converted utility already got this right, which is
  the tell: when one conversion disagrees with seven, suspect the one.
  Generalise: a utility with a required-argument long option needs the
  separated spelling in its harness, not only the `=` one.
- **A sixth shared module, `xnum.rs`,** rather than a third hand-rolled copy of
  gnulib's `xstrtoumax`/`xdectoint`. `nl` and `head` had each already written a
  partial one, disagreeing exactly where two partial copies would, and `fold
  -w` would have been the third. The grammar is much larger than "a decimal
  integer": leading whitespace, an accepted `+`, a suffix table with two
  possible bases, a bare suffix meaning *one* of it (`head -c K` is 1024), and
  — the part no one reinvents correctly — a choice between two different
  `strerror` sentences for out-of-range decided by a heuristic on the *value*
  (`INT_MAX / 2`) rather than on the bound that was violated. The suffix table
  was certified against GNU `head -c` rather than against my reading of it.
  `fold` passes `Some(b"")` (no suffixes at all), which is why `fold -w 1K` is
  an error though `head -c 1K` is not: one function, different caller lists.
- **The over-wide character does not reset the column.** When a character alone
  exceeds the width, upstream emits it on its own line via `if (offset_out ==
  0) { line_out[offset_out++] = c; continue; }` — and that `continue` skips the
  `column = offset_out = 0` that every *other* break performs. So `printf
  'a\tb\n' | fold -w 4` is `a\n\t\nb\n`, not the intuitive `a\n\tb\n`: the tab
  lands alone at column 8, and `b` is therefore column 9 and breaks again. Both
  halves of the unit test asserting the intuitive answer were wrong and GNU was
  right, at width 8 as well as at width 4.
- **`fold` is per-file where `expand` is one stream.** `column` and
  `offset_out` are locals of `fold_file`, so a file not ending in a newline has
  its partial line flushed *without* one and the next operand starts at column
  zero. `expand`'s operands are a single continuous stream. The two utilities
  are neighbours, share a harness shape, and differ here — which is precisely
  why `half1.txt`/`half2.txt` exist in both harnesses with opposite
  expectations.

### Progress (appended 2026-08-17, `paste`)

`paste` is the twelfth (`scripts/paste-diff.sh`: 200 passed, 0 differed, 6
differ on purpose; 37 differ when pointed at MSYS2's `paste`, which is the
harness proving it still discriminates). The shipped version recognised only
`-d` and `-s`, did not cycle the delimiter list, did not collapse its escapes,
had no `-z`, treated `-` as a file called `-`, split input as UTF-8 text, and
exited 0 whatever happened. Five things worth carrying forward:

- **A whole gnulib quoting style had to be built first, and it was built by
  reading the C, not by guessing.** `paste`'s trailing-backslash diagnostic uses
  `quotearg_n_style_colon (0, c_maybe_quoting_style, …)` — deliberately *not*
  the usual `quote()`, because that "would double the number of displayed
  backslashes, making the diagnostic look bogus." Nothing else in our tree had
  `c_maybe`. It is now `quote_c_maybe`/`quote_c_maybe_colon` in `quote.rs`
  (`a465d3b29`), settled by reading `lib/quotearg.c` and then *measured* against
  two independent GNU oracles — `ls --quoting-style=c-maybe` for the plain form
  and `paste -d 'ARG\'` for the colon one — over every byte in three positions
  (`scripts/c-maybe-probe.py` → `tests/c-maybe-gnu.txt`, 1590 rows,
  `tests/c_maybe.rs`). **All 1590 matched on the first run**, which is the value
  of doing both: source-reading and black-box measurement agreeing is evidence,
  either one alone is a belief. Three inputs no oracle can express (NUL, the
  empty string, and `/` for the `ls` oracle) are unit tests marked *unmeasured*
  rather than quietly folded in with the rest.
- **The two modes are not two spellings of one algorithm, and the difference is
  in the *error* rules, not the merge.** Parallel opens every operand up front
  with `error (EXIT_FAILURE, …)` — fatal, before a byte of stdout, and only the
  first bad operand is ever named. Serial opens as it goes with `error (0, …)` —
  names every one and continues. So `paste A nosuch B` and `paste -s A nosuch B`
  differ in stdout, stderr *and* their interleaving. Every missing-file row in
  the harness therefore runs both ways; a harness that tested one mode's failure
  path would have certified the other's as identical.
- **A read error is reported one output line *after* the read that failed.**
  Upstream leaves it in the stream's `ferror` flag and only looks when it closes
  the file, which is the next time round the loop — and in the meantime the
  failed stream keeps answering "end of file", so the line it was part of still
  finishes and still gets its delimiter. `Source::failed` exists to reproduce
  that delay rather than to store an error, and the unit test drives a reader
  that yields one byte and then fails, because nothing else exhibits it.
- **`\0` and "no delimiter" are the same value, which is why `-d ''` is not the
  empty list.** `#define EMPTY_DELIM '\0'`, so a NUL cannot be *used* as a
  delimiter; `main` rewrites an empty `-d` argument to the two characters `\0`
  so the list has one position that emits nothing. The position is still spent:
  `-d 'x\0y'` puts `x` between columns 1|2, nothing between 2|3, and `y`
  between 3|4 — a list that merely dropped the position would put `y` at 2|3.
  This is exactly the kind of rule a plausible implementation gets wrong and a
  whitespace-tolerant harness never catches.
- **The delimiter diagnostic is raised between the option loop and the opens**,
  which pins it in a two-sided way no single test states: a later `-d` rescues
  an earlier bad one and any getopt error preempts it (both are raised inside
  the loop), yet it preempts a missing file (the opens come after). Three
  harness rows and one unit test hold that position from both sides.

One asymmetry is deliberately *not* reproduced and is recorded here instead.
With standard input closed, `paste -s - A 0<&-` prints `paste: -: Bad file
descriptor` **twice**: once from the per-file read and once from `main`'s
closing `if (have_read_stdin && fclose (stdin) == EOF)`. Rust has no
`fclose (stdin)` and surfaces a read error where the read happens, so the second
line has no analogue. The parallel-mode companion — `opened_stdin &&
have_read_stdin` → `paste: standard input is closed` — *is* reproduced, guarded
by a `#[cfg(unix)]` file-descriptor check that can never fire on the Windows
host this harness runs on.

### Progress (appended 2026-08-17, `comm`)

`comm` is the thirteenth (`scripts/comm-diff.sh`: 195 passed, 0 differed, 7
differ on purpose — **zero differences on the harness's first run**; 61 differ
when pointed at MSYS2's `comm`, which is the harness proving it still
discriminates). The shipped version had no `--total`, no `--output-delimiter`,
no `-z`, no order checking at all, took `--check-order` and `--` as filenames,
and read its input as UTF-8 text. Five things worth carrying forward:

- **The locale decides what the program *computes*, not just how it words a
  complaint — so this harness runs under `C` end to end.** Its twelve siblings
  run under `C.UTF-8` and drop to `C` only for the gnulib-quoted diagnostics
  (B-Q2). `comm` cannot: `order = hard_LC_COLLATE ? xmemcoll (…) : memcmp2 (…)`,
  and `hard_locale` is false for exactly `C` and `POSIX`. Under any other locale
  GNU pairs lines by `strcoll`, where case can be secondary and two different
  byte strings can compare *equal*. Ours compares bytes always. Under `C` that
  is GNU's rule and the agreement is by construction; under `C.UTF-8` it would
  have been a coincidence of codepoint order matching byte order, and a harness
  that ran there would have been certifying the coincidence. A closing section
  re-runs five ASCII comparison cases under `C.UTF-8` to *bound* the claim
  rather than hide it; the divergence itself is filed against the same
  collation entry as the `oils` funmap listing.
- **The default order check is not "warn on descent" — it is armed by something
  else entirely.** `check_order` fires only when
  `check != DISABLED && (check == ENABLED || seen_unpairable)`. So `comm D D`
  over a file that is thoroughly out of order is *silent and exits 0*: every
  line pairs, nothing ever sets `seen_unpairable`, and the descent is never
  looked at. Add `--check-order` and the same command line becomes a fatal error
  after one line of output. That pair of rows is in the harness precisely
  because an implementation written from the option's *name* passes every other
  order-checking test and fails these two.
- **The three columns are made of separators, not of padding.** `writeline`
  writes zero separators before a column-1 line, one before column 2 *if
  column 1 is shown*, and two before column 3 *counting only the shown columns*.
  So `-1` does not blank a column, it removes a tab from every remaining line —
  which is why stdout is compared through `od -An -c` and why all ten
  suppression spellings are separate rows.
- **`--output-delimiter=` is a NUL, not nothing.** `col_sep` keeps pointing at
  `""` while `col_sep_len = *optarg ? strlen (optarg) : 1` forces the length to
  1, so each boundary emits one zero byte. Repeating the option is allowed only
  with an *identical* argument, and the comparison is against the argument **as
  typed** — which is why `Settings` stores the raw bytes and derives the
  effective separator, rather than storing the separator and losing the
  distinction between `=` and `=\0`.
- **Upstream's four-buffer-per-file rotation collapses to two owned lines, and
  the one behavioural difference is provably invisible.** `lba[2][4]` +
  `alt[2][3]` exist so the EOF path can re-check the last two lines; ours keeps
  `prev` and `prev2`. After exactly one line has been read upstream's "two back"
  *aliases* that same line, so its re-check compares a line with itself and
  stays silent — where ours skips the re-check because `prev2` is `None`. Same
  silence, reached two ways. The argument is written out on `Column::prev2` so
  the next reader does not have to reconstruct it from the C.

One asymmetry is deliberately *not* reproduced. `comm - -` merges one stream
against itself — well defined, and both sides get the output right — but GNU
then closes both files, and both are `stdin`, so the second `fclose` runs on an
already-closed stream. That is undefined behaviour; on glibc it sets the error
indicator and `close_stdout` reports `comm: -: Bad file descriptor` and exits 1.
Reproducing the diagnostic would mean reproducing a double free, so the case is
an xfail in the harness with the reasoning attached, alongside the two host
xfails (a directory operand, which a Windows `File::open` refuses outright) and
the four `--help`/`--version` ones.

### Progress (appended 2026-08-17, `join`)

`join` is the fourteenth (`scripts/join-diff.sh`: 303 passed, 0 differed, 5
differ on purpose — again **zero differences on the harness's first run**; 107
differ when pointed at MSYS2's `join`, which is the harness proving it still
discriminates). Like
`comm` it runs under `C` end to end, and for the same reason
(`hard_LC_COLLATE ? xmemcoll : memcmp`); unlike `comm` the collation decides not
only the order but *which lines pair*, so a UTF-8 run would have been certifying
a coincidence twice over. The shipped version knew six options and only as
separate words (`-a 1` yes, `-a1` no, `-t:` no, `-12` no), reported everything
else as `join: unknown option: -i`, and was wrong in four ways that changed
answers rather than diagnostics: `-o auto` was parsed and ignored, `-e`'s filler
was substituted *before* the comparison so it decided which lines paired, an
unpairable line was reprinted as its fields rejoined (moving the join field out
of first position), and input was read with `BufRead::lines` so `\r\n` lost its
`\r` and one non-UTF-8 byte ended the run. Five things worth carrying forward:

- **An operand can stop being an operand.** `join`'s optstring begins with `-`,
  which is `getopt_long`'s RETURN_IN_ORDER mode: operands are *not* permuted to
  the end. `join` uses that to support the obsolescent `-j1 N` / `-j2 N` / `-o
  LIST LIST` spellings by *retroactively reinterpreting* an earlier operand once
  a third one arrives — two slots plus a four-valued status each, and a shift.
  The visible consequence is that `join -o 1.1 A B C` says
  `invalid file number in field spec: 'A'`: the name it blames was never an
  operand. An implementation that counted operands at the end instead would
  blame `C`, agree with GNU on every ordinary command line, and be wrong here.
  `Parse::add_file_name` is that machine transcribed verbatim, including the
  `prev_optc_status` carry that makes `-o X Y` extend the list.
- **The default order check is armed by an unpairable line, not by a descent** —
  the same trap as `comm`, and worth restating because the two utilities are
  usually converted apart. `join dis.txt dis.txt` over thoroughly unsorted input
  is silent and exits 0.
- **`%.*s` stops at a NUL, and that is observable.** The disorder warning prints
  the offending line with `printf ("%s:%ju: is not sorted: %.*s", …)`. The
  length is computed by stripping one trailing `'\n'` — the literal newline, not
  `-z`'s delimiter — and capping at `INT_MAX`, but `%.*s` then truncates again
  at the first NUL. Under `-z` every record ends in one and most contain
  newlines, so the warning shows a fragment. Reproducing the length arithmetic
  alone passes every ASCII test and fails `-z --check-order`.
- **Two upstream statements are dead, and transcribing them faithfully would
  have been the bug.** Both tail blocks contain `if (seq_other.count) seen_
  unpairable = true;`, and the merge loop above them only exits when one of the
  two counts is zero — so in each block the other count is provably zero. They
  are documented in `join()` as not transcribed. Copying C without checking
  reachability is how a transcription acquires behaviour the original never had.
- **A field that is absent and a field that is empty are the same field.**
  `keycmp` gives an out-of-range join field length 0 and sorts length 0 before
  everything, so `-1 99999999999999999999 A B` (clamped to `PTRDIFF_MAX`, not
  refused) makes every line of `A` unpairable and prints nothing, status 0. The
  clamp itself needed `xstrtoimax`'s three-state result reimplemented: no digits
  and digits-then-junk are both `INVALID`, clean overflow is `OVERFLOW` and is
  *accepted*, so `-1 -9223372036854775808` is an error while
  `-1 -9223372036854775809` is not.

### Progress (appended 2026-08-17, `tsort`)

`tsort` is the fifteenth (`scripts/tsort-diff.sh`: 86 passed, 0 differed, 9
differ on purpose — **zero differences on the harness's first run** for the
third conversion running; 37 differ when pointed at MSYS2's `tsort`, which is
the harness proving it still discriminates). It also survived 1100 randomly
generated graphs compared against GNU on stdout, stderr and status separately:
600 over plain names, and 500 over an alphabet chosen to be hostile — embedded
NULs, `\r`/`\v`/`\f`, high bytes, names that are prefixes of others, tabs and
runs of blanks as separators, and a trailing orphan token 15% of the time.
`scripts/tsort-probe.py` re-derives the rows quoted in `tsort.rs`'s
documentation. The shipped version had no option parser at all —
`--help`, `--version` and `--` were all read as file names, and a second operand
was silently ignored — and four things past the command line changed *output*
rather than diagnostics. Five things worth carrying forward:

- **A balanced tree whose only use is an in-order walk is not a data structure,
  it is a sort.** Upstream keeps items in Knuth's Algorithm A tree, rotations
  and all, keyed by `strcmp`; every pass over the items is `walk_tree`, which is
  an in-order traversal. An in-order traversal of a search tree yields sorted
  keys whatever the balancing did, so none of the rotation code is reachable
  through the output and a vector sorted by name is exactly equivalent. Roughly
  200 lines of upstream have no behaviour in them. Checking *reachability*
  before transcribing is the same discipline that caught `join`'s two dead
  statements, applied to a whole subsystem instead of two lines.
- **Insertion order into a linked list is output.** `record_relation` *prepends*
  to the predecessor's successor list, so the list runs newest-relation-first,
  and that order is the order ready items enter the queue — which is the order
  standard output prints. `printf 'a c\na b\na d\n' | tsort` answers `a d b c`,
  not `a b c d`. Any "is this a valid topological order?" test passes both. Only
  a byte-for-byte comparison against GNU catches it. Stored back-to-front here
  and walked in reverse, so the prepend stays O(1).
- **A cycle is a diagnostic, not a stopping condition.** GNU names the file,
  prints the cycle's members **on standard error**, deletes one relation to
  break the cycle, and resumes — so standard output still lists every item
  exactly once, several cycles can be reported in one run, and the status is 1.
  The shipped version printed the members on standard *out*. The backward walk
  (`detect_loop`) reuses the queue link field for its chain and needs several
  tree passes to close one cycle, which is why the caller repeats the walk until
  the chain empties rather than once.
- **Three delimiters, not "whitespace".** `DELIM` is `" \t\n"`. Carriage return,
  vertical tab and form feed are ordinary bytes *inside* a token, so
  `printf 'a\rb x\n'` is two tokens to GNU and three to anything built on
  `str::split_whitespace` or a line reader — and three tokens is an odd count,
  which is fatal. A `\r\n` file therefore fails outright rather than sorting
  slightly wrong.
- **gnulib's `parse_gnu_standard_options_only` calls `getopt_long` exactly
  once**, with a table of just `--help`/`--version` and an optstring of `""`.
  Three visible consequences: there are no short options at all, so `-h` is
  `invalid option -- 'h'`; options still permute, so `tsort FILE --version`
  prints the version; and because the single call either exits or reports
  nothing, every argument reaching the operand check is an operand. Worth
  recording because ~15 other utilities use the same helper, and this is their
  whole parser.

### Progress (appended 2026-08-17, `seq`)

`seq` is the sixteenth (`scripts/seq-diff.sh`: 2303 passed, 0 differed, 3 differ
on purpose, comparing stdout, stderr *and* exit status separately against
glibc's `seq` in WSL; `--flip` reports 307 differences on a deliberately
misaligned reference, which is the harness proving it discriminates). It is the
first utility whose *output* was wrong rather than its command line — it
accumulated `val += increment` in `f64` and stopped at `val <= last + EPSILON`,
so it drifted, printed Rust's `Display` instead of a precision taken from the
operand's spelling, and had no `-w` at all. It is also the first caller of
`coreutils::extfloat` (the x87 80-bit float certified the day before). Six
things worth carrying forward:

- **A leading `+` in the optstring is a whole parsing mode, and it is
  observable.** `seq`'s optstring is `"+f:s:w"`; the `+` makes `getopt_long`
  stop at the first non-option, so `seq 1 --version` prints
  `invalid floating point argument: '--version'` rather than the version, and
  `seq 1 -w 3` is an operand error. Utilities that permute and utilities that
  stop are not distinguishable from their `--help` text; check the optstring.
  `seq` additionally re-checks argv *before each* `getopt_long` call and breaks
  out if the next argument is `-` followed by `.` or a digit, which is what
  makes `seq -3` a bare operand instead of three unknown options.
- **Which diagnostics carry `Try 'seq --help'` is per-message and splits along
  a line the output does not explain.** `seq`'s four *format* complaints (no `%`
  directive, ends in `%`, unknown `%X` directive, too many `%` directives) print
  one line and stop; every other refusal — bad operand, missing operand, extra
  operand, zero increment, `-w` with `-f` — prints the sentence *and* the
  referral. Upstream that is `error (EXIT_FAILURE, …)` versus
  `error (0, …)` + `usage (EXIT_FAILURE)`; from the outside it is unguessable.
- **Width and precision come from how the operand was *written*, not from its
  value.** `seq 1 1 1.0` prints `1.0`, and `seq -w -1 1 3` prints
  `-1 00 01 02 03` — the `-` is counted in the first operand's width but the
  padding is computed from the widest, so the widths are deliberately unequal.
  Upstream's `scan_arg` does this with `size_t` arithmetic that *wraps*
  (`width--` on a bare `1.`), and the wrapped value is then compared with `max`,
  so it is load-bearing rather than a latent bug: transcribed with
  `wrapping_sub`/`wrapping_add`, not `saturating_*`.
- **`seq` prints one number past the end, sometimes.** When `first + i*step`
  overshoots `last`, upstream formats it anyway, strips the format's suffix,
  re-parses from past its prefix, and prints it if the value read back equals
  `last` and its text differs from the previous line's. That is the only reason
  `seq 0 0.000001 0.000003` reaches `0.000003`. A reimplementation that stops at
  the first overshoot is wrong on a whole family of ordinary inputs.
- **Two integer fast paths, both counting in decimal digit strings.** One runs
  on the operands as typed, one after conversion (so `seq 1e3 1 1005` and an
  `inf` endpoint also qualify). Both are exact past 2^64 — `seq
  18446744073709551615 18446744073709551625` counts correctly — and both are
  gated on no `-w`, no `-f`, a one-byte separator, and a step in `(0, 200]`.
  The 200 is quoted in the GNU manual, so `seq 1 200 2001` and `seq 1 201 2001`
  taking different code paths is observable and is a test case.
- **The harness passes argv through a file, not a command line.** Ours is a
  native Windows binary and the reference runs in WSL, so anything quoted into
  `wsl -e bash -c '…'` crosses two shells and a Win32 command-line encoder —
  which an argument like `%\303\251`, or a separator that is one newline, does
  not survive. `scripts/seq-cases.py` writes one case per line with US (0x1f)
  between arguments (a tab would not do: `read -a` collapses runs of
  *whitespace* separators and would lose `-s ''`), and `scripts/seq-probe.sh`
  is run unchanged on both sides. Any future harness for an argv-only utility
  with awkward arguments should copy this shape rather than `expr-diff.sh`'s
  inline `run_case`.

The three deliberate differences are one policy: for an unknown directive whose
byte is not printable, GNU writes the raw byte — measured, `seq -f $'%\n' 1 3`
puts a real newline inside `seq:` own diagnostic, which lets a format string
forge a second line of output — and we escape it as `\ooo`, the same choice
`coreutils::getopt` already makes for an unknown short option.

### Progress (appended 2026-08-17, `printf`)

`printf` is rewritten and certified (`scripts/printf-diff.sh`: 1178 passed, 0
differed, 7 differ on purpose, comparing stdout, stderr *and* exit status
separately against glibc's `printf` in WSL; `--flip` reports 817 differences on
a deliberately misaligned reference). **It does not move the 16-of-85 counter,
and that is not an oversight:** `printf` is the one utility that must *not* use
`coreutils::getopt`. Upstream parses its two options by hand, with a comment
saying why — `getopt_long` would let `--v` abbreviate `--version`, and a format
string is an operand, so `printf --v` has to print the four characters `--v`
rather than a version banner. Converting it to the shared parser would be a
regression dressed as consistency.

It was the worst-shipped utility we had. The old 329-line version had no
floating point at all (`%f` printed the argument unchanged), no field widths,
no format reuse, no `%b` or `%q`, and read numbers with Rust's parser, so a bad
one silently became zero. It is the second caller of `coreutils::extfloat` and
the first of `coreutils::cfmt`, the new eighth shared module. Seven things
worth carrying forward:

- **The reference is neither `printf` nor `/usr/bin/printf`, and both wrong
  answers look plausible.** In WSL a bare `printf` is *bash's builtin*, a
  different program with different diagnostics (`printf: 0o17: invalid number`
  where GNU says `'0o17': value not completely converted`); the first
  measurement batch of this task was silently taken against it. But spelling it
  `/usr/bin/printf` breaks the comparison the other way, because every GNU
  diagnostic is prefixed with `argv[0]` — each one would read
  `/usr/bin/printf: …` against our `printf: …`. `env printf` is the spelling
  that satisfies both: PATH holds no builtins, and `exec` keeps `argv[0]` bare.
  Any utility whose name collides with a shell builtin (`echo`, `test`, `true`,
  `false`, `kill`, `pwd`) has this trap waiting.
- **A field too wide to render is a `write error` at exit, not a complaint at
  the directive.** C counts a width in `int`, so `%2147483648d`,
  `%.2147483648d`, and a `*` width of exactly `INT_MIN` (whose magnitude is one
  past `INT_MAX`, and which `printf`'s own `INT_MIN <= w <= INT_MAX` check
  therefore lets through) all fail inside the C library: the conversion writes
  nothing, the rest of the format still prints, and only the *stream* remembers
  — so `printf '%*d|%s|\n' -2147483648 5 tail` prints `|tail|`, then reports
  `printf: write error` with no `strerror` clause, and exits 1. Ours reaches
  the same place with a `stream_failed` flag standing in for the stream's error
  indicator. It also outranks `\c`, because upstream's `exit (EXIT_SUCCESS)`
  still runs the `atexit` handler.
- **The same escape means different things in a format and in a `%b`
  argument.** In the format, `\101` is `A` and `\0101` is a backspace followed
  by `1`; in a `%b` argument the leading `0` is optional, so `\0101` is `A`.
  One flag (`octal_0`) selects between them, and the two spellings are visible
  on the same input — `printf '\0101'` and `printf %b '\0101'` disagree.
- **`\u` falls back to its own spelling, in the opposite case to the
  diagnostic.** In the C locale gnulib's conversion succeeds below U+0080 (so
  `\u0041` is `A` and `\u0000` is a NUL byte) and fails above, printing the
  escape back as `\uXXXX`/`\UXXXXXXXX` in **upper** case — while the surrogate
  refusal, `invalid universal character name \ud800`, is **lower** case. Two
  adjacent sentences in one file disagreeing on hex case is exactly what a
  transcription smooths over and a differential test catches.
- **Partial output survives a fatal format error.** `printf 'a%z'` prints `a`
  and *then* complains, because upstream flushes through
  `atexit (close_stdout)`. A reimplementation that reports before flushing
  loses the `a`, and nothing in the diagnostic hints that it should not.
- **The flag/conversion table is a validity check, not just formatting.**
  `%#d`, `%0s`, `%#s`, `%.1c`, `%'e`, `%5b` and `%-q` are all *invalid
  conversion specifications* — fatal, not ignored — because each flag removes
  entries from upstream's 256-entry `ok[]` table. `%b` and `%q` are matched
  before the flag loop, which is why they accept none.
- **A harness that builds only when the binary is missing measures the wrong
  binary.** `cargo test` and `cargo clippy` do not refresh `printf.exe`, so a
  fix verified by a unit test and then measured here was measured against the
  *previous* build, and reported as a genuine remaining difference in an
  otherwise clean run. Both `printf-diff.sh` and `seq-diff.sh` now build every
  run unless `OURS` was set by the caller.

The seven deliberate differences are one policy, the same one `seq` and
`coreutils::getopt` already apply: where a diagnostic echoes bytes the caller
chose — an unknown conversion (`%\n`, `%\x1b[31m`) or the tail of a character
constant (`%d "'aé"`) — GNU writes the raw byte and we escape it as `\ooo`.
Raw is a forged line, or a terminal escape sequence, inside `printf`'s own
error stream.

One case the harness cannot carry, recorded so it is not "fixed" later: an
argument that is not valid UTF-8. Our side is a native Windows binary, so argv
arrives as UTF-16 and MSYS transcodes on the way in — `\xff\xfe` comes back as
`\xc3\xbf\xc3\xbe`. The mangling is outside our code (an MSYS-native program
given the same argument sees the original bytes) and cannot happen on the
target, where argv is bytes; such a case would measure the Windows command
line rather than `printf`. Non-UTF-8 bytes are still tested where they can be:
in the *format*, where they arrive as escapes the program decodes itself, and
in `cfmt`'s unit tests, which take a byte slice directly.

### Progress (appended 2026-08-18, `tr`)

`tr` is the seventeenth (`scripts/tr-diff.sh`: 289 passed, 0 differed, 2 differ
on purpose — the `--help` referral tail and the `--version` banner, both of
which name an upstream project this is not; 70 differ when pointed at MSYS2's
`tr` with `OURS=/usr/bin/tr`, which is the harness proving it still
discriminates rather than passing everything put in front of it).

The shipped version was a 265-line skeleton with `-d` and ranges and nothing
else: no `-s`, no `-c`, no `-t`, no character classes, no equivalence classes,
no `[c*n]` repeat, escape decoding that handled a handful of letters, and a
`--help` of one line where GNU's is 54. Six things worth carrying forward,
five of which the harness found and the sixth of which it structurally cannot:

- **`tr` does not quote with `quote()`. It has two private renderers, and
  which one runs depends on the message.** `make_printable_char` — printable
  byte as itself, anything else as `\NNN` octal, *never* a named escape —
  renders the reverse-range message. `make_printable_str` — named escapes for
  `\a\b\t\n\v\f\r`, printable-or-octal otherwise — renders the `[=c=]` operand.
  And `invalid character class` and `invalid repeat count` run
  `make_printable_str` *then* `quote()`, so a backslash the first one produced
  comes back out doubled: `tr -d '[:no\377such:]'` says
  `invalid character class 'no\\377such'`, with two backslashes. Neither
  renderer escapes `'` or `\`. **None of this is visible on printable input**,
  which is why the first 250-case harness passed these cases while the code was
  wrong — every diagnostic case in it had been written with printable text. The
  general lesson for the next utility: a case that echoes caller bytes back
  must use *unprintable* bytes, or it is not testing the renderer at all.
- **I suspected the shared module before the caller, and was wrong.** Range
  endpoints were printing as raw high bytes, and `coreutils::quote` was the
  obvious suspect. It was read in full and is correct — measured against an
  8333-row GNU fixture. The bug was `byte as char` in `tr`'s own `parse_set`.
  A module with a fixture behind it is evidence; reach for the caller first.
- **The repeat count is `strtoumax`'s grammar, not a string of digits.**
  Leading whitespace is skipped and a leading `+` accepted (`[x* +1]` is one),
  but `-` is not and `+` before whitespace is not. The trap is that the base is
  chosen from the field's **raw first byte** — `*digit_str == '0' ? 8 : 10` —
  *before* the whitespace is skipped, so `[x*010]` is eight and `[x* 010]` is
  ten. Discriminating those two needs a SET1 of at least 11 bytes; with a short
  SET1 the padding hides the difference, and the harness's original repeat
  cases all used `a-f`.
- **`find_bracketed_repeat` aborts at the first escaped byte of any kind**,
  rather than scanning past it for the `]`. So `[x*\]]` is six literal bytes,
  and so is `[x*\062]` — even though its `]` is unescaped and would otherwise
  close the construct. `find_closing_delim`, which serves `[:…:]` and `[=…=]`,
  has no such rule and reads straight through. Two scanners, two behaviours,
  in one file.
- **The mode decides which operand is "extra".** `tr -d a b c` names `'b'`;
  `tr -s a b c` names `'c'`. Deleting *without* squeezing is the one mode that
  takes a single set, so it is the one mode whose first-too-many operand is the
  second. Ours had one rule for all modes and named the third every time. The
  explanatory second line appears only at *exactly* one excess operand.
- **One defect a byte-for-byte harness cannot see, so it is a unit test.**
  Upstream reaches SET2 through a lazy generator that stops once SET1 is
  exhausted; ours expanded it eagerly, so `tr a '[x*4294967296]'` was a 4 GiB
  allocation and a 50-second wait against GNU's 0.29s. The *output* was
  correct, so every comparison the harness makes passed. It is pinned by
  `set2_is_not_materialised_past_set1` instead. SET1 is deliberately left
  uncapped, because GNU grinds on a huge SET1 too and parity is the goal.

35 unit tests, clippy clean.

### Progress (appended 2026-08-18, `od`)

`od` is the eighteenth (`scripts/od-diff.sh`: 266 passed, 0 differed, 4 differ
on purpose — the `--help` referral tail, the `--version` banner, and `-w0` and
`-w-4`, on which GNU 9.4 `abort()`s; see below. 38 differ when pointed at
MSYS2's `od` with `OURS=/usr/bin/od`, which is the harness proving it still
discriminates rather than passing everything put in front of it).

The shipped version was a 251-line toy: `-t` accepted a single conversion,
there was no `-A`, no `-j`, no `-N`, no `-S`, no `-w`, no `--endian`, no `z`
trailer, no duplicate-line elision, and none of the traditional
`od -c file +20` grammar. It is now ~1500 lines and matches GNU on every case
the harness puts to it. Six things worth carrying forward:

- **A module can be right at its exit and wrong at its entrance.**
  `od -t fF` on the bytes `fc fd fe ff` printed `nan` where GNU prints `-nan`.
  `extfloat`'s renderer was *not* at fault — it has had
  `assert_eq!(f("%Lf", -ExtF80::NAN), "-nan")` since it landed. The bug was
  `ExtF80::from_f32`/`from_f64`, which returned the bare `ExtF80::NAN` constant
  for any NaN and so dropped the caller's sign bit on the way *in*. Widening to
  `long double` is an x87 `FLD`, which copies the sign through. The general
  lesson: a module test that builds its input from the module's own constants
  never exercises the conversion the callers actually use, and that conversion
  is where the sign went. Both directions now have tests
  (`widening_carries_the_sign_of_a_nan`).
- **gnulib's `ftoastr` never looks at `errno`, and that is load-bearing.** It
  is the shortest `%.*g` that reads back equal, and its precision floor drops
  from 18 to **1** for a subnormal. Our first cut checked the round trip with
  `extfloat::xstrtold`, which rejects a `range_error` — and every subnormal
  underflows, so the check said "did not round-trip" for the exact values the
  floor exists to serve. The loop then ran out to its bound and printed
  `3.6451995318824746025e-4951` where GNU prints `4e-4951`. Upstream uses plain
  `STRTOF` and compares only the value. The strict wrapper was the wrong tool:
  it answers "is this a valid complete numeral?", not "does this text denote
  this number?".
- **A NUL inside a diagnostic is invisible to `$(...)`.** `od -A ''` reports
  `invalid output address radix ''; it must be one character from [doxn]` with
  the *rejected byte* interpolated — and that byte is NUL. Comparing
  `$(cat ours.err)` against `$(cat gnu.err)` drops it from both sides, with a
  bash warning, so the harness was silently not testing the one character the
  case exists to test. The comparison is now `cmp -s` on the files, with the
  text forms kept only for the failure report. Any harness for a utility that
  echoes a caller's byte back should do the same.
- **Do not edit a shell script while a run of it is in flight.** Bash reads a
  script incrementally by byte offset, so inserting a function above the point
  it has reached shifts everything below and it resumes mid-token: run 2 died
  with `syntax error near unexpected token 'in'` in a file that `bash -n`
  passes. Copy to a scratch path and run the copy, which is what run 3 did.
- **An `xfail` on `--help` must not swallow the body.** GNU's last five lines
  name the GNU project, its bug address and its manual, so they must differ —
  but xfailing the whole option leaves the other 73 lines (every option's
  spelling and indent, the SIZE and BYTES paragraphs) unchecked, which is
  precisely the part a hand-written help text gets wrong. `help_body` strips
  the tail and compares the rest byte for byte; the tail alone is the xfail.
- **Two deliberate divergences, both narrow.** GNU 9.4 writes
  `if (s_err != LONGINT_OK || w_tmp <= 0) xstrtol_fatal (s_err, …)`, so a
  *well-formed* non-positive width reaches `xstrtol_fatal` with `LONGINT_OK`
  and it `abort()`s — `od -w0` dies of SIGABRT with no message. We print the
  diagnostic upstream evidently meant. And the block buffers are obtained with
  `try_reserve_exact`, so an absurd `-w` is `memory exhausted` (upstream's own
  `xalloc_die` wording) rather than an abort.

21 unit tests, clippy clean with `--all-targets`.
