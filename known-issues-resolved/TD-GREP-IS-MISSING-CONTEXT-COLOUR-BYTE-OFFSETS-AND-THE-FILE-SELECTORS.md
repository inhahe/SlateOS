## TD-GREP-IS-MISSING-CONTEXT-COLOUR-BYTE-OFFSETS-AND-THE-FILE-SELECTORS (lane B, 2026-08-25) — **resolved 2026-08-25** (every feature the title names is built; what is left is diagnostic wording, tracked with the getopt debt)

**In short:** our `grep` handled matching well and was missing a lot of the
things people actually type at it. All of it is now built. `-d` (what to do when
an operand is a directory), `-D` (the same question for a device or a FIFO) and
`--include`/`--exclude`/`--exclude-dir`/`--exclude-from` (search only some of the
files a recursive search would reach) landed on 2026-08-25, joining context
selection (`grep -C 3 pattern file`, show three lines either side of each hit),
`-b` (print each hit's byte offset), `-T` (line up the output in columns) and
`--color` (paint the matched text, and the rest of the line, in terminal
colours) from earlier the same day. Five further things were wrong rather than
absent — the recursive walk followed symbolic links it should have skipped, an
unreadable directory did not raise the exit status, a failed write to a closed
output was thrown away, `-q` stopped too late, and a long option refused a value
given as the next argument — and all five were fixed on 2026-08-25; they are kept
below because the fix for each is what the harness now pins.

**How they were found.** All of it in one run, by `scripts/grep-diff.sh`'s
first execution after it was moved onto `diff-wsl.sh` (2026-08-25). None of it
was visible before, because the harness had been comparing a Windows build
against MSYS2's Cygwin-derived grep on a Windows host; six cases were even
recorded as *deliberate* divergences over a path separator the harness itself
had introduced. Each item below is a live case in that harness carrying a `?`
marker, which means the harness fails the moment the gap closes — deleting the
marker is part of closing it, and nothing here can be fixed and forgotten.

**Nothing is missing any more.** A table here used to list the two
file-selection groups as the last gaps, with the note that they had to land
together because `-d recurse` *is* `-r`, `-d skip` is the degenerate case of
`--exclude`, and all of them want the same glob matcher and the same hook in the
recursive walk. They did land together, and the write-up is below.

**Defects — all fixed 2026-08-25**, and each now pinned by plain (unmarked)
cases in `scripts/grep-diff.sh`, so a regression fails the harness:

1. **`-r` followed a symlink met during the walk.** GNU follows a symlink met
   during the walk only under `-R`; both flags follow one *named on the command
   line*. Ours did not make the distinction, so `grep -r foo symdir` reported
   `symdir/tosub/s1` that GNU does not — and a tree containing a link back to
   one of its own ancestors did not terminate. *Fixed:* `-R` now sets its own
   `deref_links` flag; the walk asks `symlink_metadata` (which does not follow)
   and skips a symlink unless `deref_links` is set, while the operand loop asks
   `is_dir` (which does). `-R` additionally keeps a stack of canonicalised
   ancestors and reports `grep: PATH: warning: recursive directory loop` on
   re-entry — measured against GNU 3.11 as a warning that `-s` silences and
   that does *not* raise the exit status.

2. **An unreadable directory left the exit status at 1.** `grep -r foo nolist`
   on a `chmod 000` directory printed the right message and exited 1; GNU exits
   2, and `-s` silences it as it does for an unreadable file. *Fixed:* the
   walk's `read_dir` failure now goes through the same "note an error, honour
   `no_messages`" path as an unopenable file, and the error raises the status.

3. **A failed write to a closed stdout was discarded.** `grep a abc >&-` exited
   0 having said nothing; GNU exits 2 with `grep: write error: Bad file
   descriptor`. Two separate causes: the old code ended in `let _ =
   out.flush()`, and the Rust runtime reopens a closed descriptor 1 on
   `/dev/null` before `main` so the write would have succeeded anyway.
   *Fixed:* `coreutils::guard_std_fds!()` plus `stdfd::restore()` recover the
   real descriptor table, and the run writes through `stdfd::Stream` and ends
   at `stdfd::close_stdout_with("grep", …, 2)`.

4. **`-q` answered too late, and could be outranked by an unrelated error.**
   Two halves. The walk collected the whole tree before searching any of it, so
   `grep -Rq foo tree` emitted diagnostics for files GNU never opens — visible
   on a looping tree, where GNU prints the loop warning for `-Rq zzz` and not
   for `-Rq foo`. And `grep -q foo nonexistent words` exited 2, where POSIX
   requires 0: a selected line answers the question "even if an error was
   detected". *Fixed:* the walk streams — searching each file as it reaches it,
   which also puts each diagnostic between its neighbours' matches rather than
   ahead of all of them — and a `-q` that found a match reports 0 regardless of
   `had_error`.

5. **A long option refused a value given as the next argv entry.** `grep
   --regexp foo words`, `--max-count 1`, `--context 1` and
   `--group-separator XX` were all rejected as missing arguments: `parse_long`
   read only the text after an `=`. `getopt_long` accepts both spellings, so
   GNU accepts both, and a caller writing an option out in full is the caller
   least likely to guess that only one form works. *Fixed:* `parse_long` now
   takes `args` and the argv index and falls through to the next entry when
   there is no `=`.

**Context selection — implemented 2026-08-25.** `-A`/`-B`/`-C`, the digit
shorthand (`-2`, and `-12` meaning twelve), the long spellings,
`--group-separator=SEP` and `--no-group-separator` all landed together, and the
14 `?` cases that pinned their absence are now plain cases. The parts that were
not obvious, each measured against GNU 3.11 rather than recalled:

* The three lengths are `Option`s, and `-A`/`-B` fall back to `-C` *after*
  parsing rather than during it. That is what makes `-A 3 -C 1` and
  `-C 1 -A 3` the same command; a plain `usize` would let the later flag
  clobber the earlier one.
* `-A 0` is not "no context": it still puts `--` between non-adjacent groups,
  which plain `grep` never does. So the separator is gated on *whether context
  was asked for*, not on the amount.
* A context line is punctuated with `-` in place of `:` in **every** prefix
  field — `ctx-2-2` against `ctx:3:HIT` — which is the only thing that tells a
  caller which lines of the output actually matched.
* Trailing context outlives `-m`. Once the count is satisfied the remaining
  owed lines are printed without being tested against the pattern, so a line
  that *would* have matched prints as context: `grep -n -m1 -A2 HIT` over three
  `HIT`s gives `1:HIT`, `2-HIT`, `3-HIT`.
* Under `-o` a context line prints nothing at all, prefix included — but it
  still counts for grouping, because the separator is decided by how far the
  file has been read and not by how many bytes came out. Hence `grep -oA1`
  gets a `--` between its two bare `HIT`s and `grep -oC2` does not.
* A file's first group is never adjacent to anything, so it takes a separator
  whenever the run has printed before — even when it starts at line 1. That is
  why the "printed before" flag lives on `Run` and not in `search_stream`.
* `-c`, `-l`, `-L` and `-q` ignore context outright, separator included.

**`-b` and `-T` — implemented 2026-08-25.** The seven `?` cases that pinned
their absence are now 33 plain cases. Both were measured against GNU 3.11 with
`od -c` rather than recalled, and twice the recollection was wrong:

* **`-T`'s field width is computed from the file's `st_size` before a single
  line is read**, which is the whole reason it can be applied to a stream at
  all. The rule is `num = size; num += 1 if -n; width = decimal_digits(num)` —
  the `+1` because a file of N bytes can hold N+1 lines. So a 99-byte file pads
  line numbers to three columns and byte offsets to two. There is *one* width,
  shared by both numeric fields, not one per field. An input with no size (a
  pipe) uses what a signed `off_t` holds, giving 19 columns; `grep -Tn HIT
  < file` is *not* that case, because redirected stdin is a regular file. This
  is why `scripts/grep-diff.sh` grew a `w99` fixture: every other fixture is
  small enough that a hardcoded width of 1 would pass.
* **The tab goes after the last separator, with no backspace.** (`\t`, `\b`
  and `\0` below are this entry's notation for the bytes themselves; grep
  emits the bytes. They were written literally here until 2026-08-25, and the
  two raw NULs made git treat the whole 4.5 MB file as binary -- so it stopped
  normalising line endings in it, and a CRLF-rewriting edit script went
  straight into the object store unnoticed.) GNU's
  `print_line_head` reads as if it emitted `"\t\b"` *before* the separator;
  the dump says `a.txt: 1:\tfoo`. And `-Z` does not suppress it —
  `a.txt\0 1:\tfoo`, and `a.txt\0\tfoo` with no numbers at all. What *does*
  suppress it is having no field to follow: bare `-T` prints no tab, and
  neither do `-c`, `-l` or `-L`, which print no line prefix.
* **`-b` reports the offset of what is printed, not of the line.** Under `-o`
  each match carries its own, so `grep -bo foo` over `foo bar foo` prints `0`
  and `8`. Under `-z` the NUL separators count as bytes like any other. A
  context line gets an offset too, punctuated with `-` like its other fields.

Implementation notes: `line_prefix` now takes a `Prefix` struct rather than
five positionals, and `search_stream` a `Source`, because the alternative was
tripping `clippy::too_many_arguments` in a crate that denies pedantic. The
"is this a regular file, honestly" test that `-T` needs on stdin was already
written for `wc`; it moved to `coreutils::filekind::borrowed_stdin` and `wc`'s
private copy was deleted rather than a third one written.

**`--color`/`--colour` and `GREP_COLORS` — implemented 2026-08-25.** The
seven `?` cases that pinned their absence are now 36 plain cases. The whole
model below was **measured** against GNU 3.11 with `od -c` — three throwaway
probe scripts, ~96 dumps — and not recalled, because two of its rules are ones
recall gets wrong.

The selection rules are small:

```
matching    = selected ^ invert
line_color  = if selected ^ (invert && rv) { sl } else { cx }
match_color = if selected { ms } else { mc }
```

An escape is `\e[<cap>m\e[K` — text — `\e[m\e[K`, with the `\e[K` (erase to
end of line) dropped under `ne`. **An empty capability emits nothing at all**,
which is not the same as emitting an empty escape, and is what makes the next
point observable.

* **The body is printed in two independent stages, and either can be skipped.**
  The *middle* runs only when `matching && !match_color.is_empty()`: for each
  non-empty match it emits `start(line_color)`, then the text since the previous
  match — **which is never closed** — then the matched text wrapped in
  `match_color`. The *tail* runs only when `!line_color.is_empty()`:
  `start(line_color)`, the rest of the line, `end(line_color)`. Whatever neither
  stage claimed is written plainly. The consequence recall gets wrong is that
  `GREP_COLORS='ms='` does not produce the default output minus one escape — it
  produces a *differently shaped* output, because switching off the match colour
  switches off the whole middle stage, so the line becomes one closed `sl` run
  instead of a sequence of unclosed ones.
* **A value capability written without `=` is ignored, not set to empty.**
  `GREP_COLORS='ms'` leaves the match colour at its default; `GREP_COLORS='ms='`
  turns it off. The booleans `rv` and `ne` fire either way. This is the second
  thing recall gets wrong, and it is the difference between "no highlight" and
  "the default highlight".
* An unknown key, a value that is not SGR parameters (digits and `;`), and an
  empty item between two colons are all ignored in silence. `mt` sets `ms` and
  `mc` together, and the last assignment in the string wins.
* **`-T`'s padding goes inside the number's escape**, not before it: the dump is
  `\e[32m\e[K  12\e[m\e[K`, which matters on a terminal whose `ln` sets a
  background. `-T`'s tab and `-Z`'s NUL are the two delimiters that stay
  *outside* every escape — painting whitespace would drag a background across
  the gutter, and the NUL is for a machine.
* Every other prefix field carries its own capability: the file name in `fn`,
  the line number in `ln`, the byte offset in `bn`, and **every** separator
  (`:`, `-`, and the `--` between groups) in `se`. The newline after a group
  separator is plain.
* **`-o` ignores `sl` and `cx` entirely.** Everything it prints is matched text,
  so everything it prints is `ms` — there is no line to colour.
* `-c` paints the name and the `:` but never the count; there is no capability
  for a count. `-l`/`-L` paint the name and nothing else.
* A `\r` that ends a line is terminator, not text: the tail run stops before it.
  A `\r` in the middle of a line is ordinary text and is painted. Hence the
  `crlf` fixture in the harness.
* An empty match is not painted, for the same reason `-o` does not print one:
  it occurs at every position, and highlighting it would bury the line in
  escapes.
* `GREP_COLOR` (singular) is deprecated: it warns on stderr and sets both match
  colours. An empty value warns nothing and changes nothing. Neither variable is
  read when colour is off, so `GREP_COLOR=1;32 grep --color=never` is silent.
* `--color=auto` and a bare `--color` mean "only if stdout is a terminal", which
  is `IsTerminal` — and off in the harness, which is why the harness always says
  `always`.

**`-d`, `-D` and the file selectors — implemented 2026-08-25.** The five `?`
cases that pinned their absence are now 65 plain cases and one `?` (`-d bogus`,
whose diagnostic is the getopt-shape debt rather than anything about `-d`). The
model below was **measured** against GNU 3.11 — five throwaway probe scripts —
and in one place the measurement flatly contradicted what reading gnulib from
memory had concluded.

* **`-d` and `-r` are one setting, not three flags.** `-d recurse` *is* `-r`;
  `-d read` is the default that says `Is a directory` and exits 2 (`-s` silences
  the message, not the status); `-d skip` says nothing and exits 1, because
  skipping is not an error. Being one setting, the last one written wins in both
  directions — `-r -d skip` skips and `-d skip -r` recurses. `-R` differs from
  `-r` only in *also* turning on symlink dereferencing, which a following `-d`
  leaves behind when it takes the recursion away. Ours therefore stores a
  `Directories` enum where it had a `recursive: bool`, with
  `Options::recursive()` reading it.
* **`-D` is tri-state, and that is what stops `grep -r pat /` hanging.** A device
  *named on the command line* is read; a device the *walk finds* is skipped.
  Neither is spelled by an argument, so the default is its own variant. Opening a
  FIFO with no writer blocks forever, so the skip has to be decided from a `stat`
  and never from an open — which is why the predicate is
  `coreutils::filekind::is_device(&Metadata)` and not a question asked of an open
  `File`. (GNU dodges the same hang from the other side, by adding `O_NONBLOCK`
  when devices are to be skipped.) It is four `S_IS*` tests — char device, block
  device, socket, FIFO — and no others: a directory is not a device, so `-D skip`
  still descends into one.
* **The combination rule is neither "includes win" nor "the last one wins".**
  Consecutive options of the same kind coalesce into a segment; the segments are
  scanned **newest first** and the first one holding a matching glob decides; a
  name no segment matches is dropped only if the **oldest** segment is an
  include. So the same two options in the other order are not the same command:

  | command | `t1.txt` | `l1.log` |
  |---|---|---|
  | `--include='*.txt' --exclude='t1*'` | dropped | dropped |
  | `--exclude='t1*' --include='*.txt'` | **kept** | **kept** |

  Reversing them makes the exclude the *newer* segment, so `t1.txt` is reached by
  the include first and kept; and it makes the include the *older* segment, so a
  name neither matches — `l1.log` — is kept rather than dropped.
* **Which list a glob joins is decided by the option, not by what it names.**
  `--exclude` and `--exclude-from` ask about files only, `--exclude-dir` about
  directories only, and there is no `--include-dir`. `--exclude=drop` therefore
  does not stop the walk entering `drop/`, and `--exclude-dir=t1.txt` does not
  stop `t1.txt` being searched. Trailing slashes are stripped from an
  `--exclude-dir` pattern, because `--exclude-dir=drop/` is what tab completion
  produces.
* **A command-line operand is matched as written *and* at every suffix that
  starts after a `/`.** Recalling gnulib said the opposite — that command-line
  names are `EXCLUDE_ANCHORED` and so match whole or not at all — and the probe
  disproved it: `--exclude=deepfile.txt` excludes `a/b/c/deepfile.txt` and
  `--exclude=top.txt` excludes `./top.txt`. During the walk only the base name is
  ever offered, so the suffix loop is unobservable there; it is written once and
  used by both paths rather than special-cased.
* The globs are `fnmatch` with **no** `FNM_PATHNAME` and **no** `FNM_PERIOD`: `*`
  crosses a `/` and matches a leading dot, which is the opposite of what a shell
  would do and the assumption a reader is most likely to bring. `\` still
  escapes.
* **Selection happens after the `stat`, not instead of it.** `grep --exclude='*'
  foo abc /nonexistent` still reports the missing file and still exits 2. Stdin
  is exempt from the whole mechanism, there being no name to match.
* **`--exclude-from` has no comment syntax.** One glob per line, a final line
  without a newline still counted, `#t1*` a glob and not a remark, and an empty
  file excluding nothing — which is not the same as excluding everything. It is
  read at parse time and its globs join the neighbouring `--exclude` segment, so
  where it sits among the other options changes the answer.
* **With no operand at all, `-r` walks `.` and prints the names without the `./`**
  that naming `.` explicitly would have kept. That is a property of the
  defaulting rather than of the walk, so it rides on its own flag set at the end
  of `parse_args`.

**One harness lesson, kept because it nearly cost the whole tranche.** The `-D`
cases can hang rather than fail if the "skip a device found by the walk" rule is
ever lost, so `scripts/grep-diff.sh` wraps each side in `timeout 20`. Written as
`env PATH=$bindir/$side timeout 20 grep`, that resolves `timeout` through the
PATH it has just set — a directory holding one symlink named `grep` — so
`timeout` was not found, **both** sides exited 127, and all 435 cases agreed at
once. A completely clean run is what a harness that cannot fail looks like from
the outside. `timeout` now comes before `env`, and `DIFF_NEED` names both it and
`sort` so a missing one skips the run instead of greening it.

**Diagnostic wording** — four kinds, four `?` cases, every one of lower value
than the above, and all four now belong with
`TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE` rather than with grep:
`grep` with no operands answers `grep: missing PATTERN` where GNU prints the
usage summary; `--zzz` is `unknown option: --zzz` against GNU's `unrecognized
option '--zzz'`; `-m x` names the offending value where GNU's does not; and `-d
bogus` prints gnulib's argmatch block but stops before the `Usage:`/`Try 'grep
--help'` pair and exits 2 where GNU exits 1. (`-D bogus` is *not* among them: GNU
does not use argmatch for it, and we reproduce its one-liner and its status
exactly.)

The fifth kind was the leading quantifier under `-E`: `grep -E '*a'` matched the
same lines GNU's does but said nothing, where GNU also writes `grep: warning: *
at start of expression`. **Fixed 2026-08-25** (`9a570ca64`). It was the one of
the five that was not merely wording — that line is the only thing telling a
user their pattern is not doing what they think — and it needed a parser change
rather than a message change, so it is recorded in design-decisions.md §384
along with the measured rule for *which* patterns warn.

**Severity.** Low, and only wording is left. Every feature this entry was opened
for is implemented and pinned by plain cases in `scripts/grep-diff.sh`, which now
runs **467 agreeing cases** against GNU 3.11 with five deliberate divergences and
four wording gaps. The deliberate five are the two choices recorded elsewhere:
we never suppress binary output, and we list a directory sorted where GNU uses
readdir order (design-decisions.md §380).

#### TD-A-POISON-CHECK-READS-EVERY-BYTE-ONE-AT-A-TIME-WHILE-THE-FILL-USES-REP-STOSB (lane A, 2026-08-25) — **open**

**In short:** the debug heap writes a known filler byte over every block it
frees and reads it back when the block is handed out again, so that a program
still writing to memory it gave back is caught. Writing the filler is one bulk
CPU instruction; reading it back is a loop that fetches **one byte at a time**,
and the loop is deliberately built so the compiler cannot speed it up. For the
largest blocks that is 4096 separate fetches on every single allocation. The
comment above the switch claims the whole thing costs "~5-20ns per
alloc/dealloc", which is true only for the smallest blocks.

**Where.** `kernel/src/mm/heap.rs`: `check_poison` (~316) against `poison_free`
(~131) and `poison_alloc` (~193). The asymmetry is one line each way —

```rust
// poison_free: one bulk `rep stosb`
rawmem::fill_u8(ptr.add(12), FREE_POISON, class_size.saturating_sub(12));

// check_poison: class_size scalar volatile loads, and no bulk counterpart
for i in 12..class_size {
    let byte = unsafe { rawmem::read_u8(ptr.add(i)) };
    if byte != FREE_POISON { … }
}
```

**Why it cannot simply be vectorised in place.** The volatility is load-bearing
and documented: `check_poison` carries `#[inline(never)]` because with thin LTO
the compiler otherwise inlines it into `pcpu_slab_alloc`, constant-propagates
across the free→alloc boundary, "knows" what `poison_free` wrote, and deletes
the read entirely — a check that is optimised away is a check that never runs.
`rawmem::read_u8` exists to defeat that, and it also keeps the read out of
KASAN's instrumentation, which has the slot marked freed. So the fix is not
"drop the volatile"; it is to give `rawmem` a bulk *scan* with the same
guarantees that `fill_u8` already provides for the write side.

**Proper fix.** A `rawmem::scan_u8(ptr, value, len) -> Option<usize>` built on
`repe scasb` — repeat *while equal*, which stops on the first byte that differs
and leaves the offset in the count register, i.e. exactly the "first corrupted
byte, and where" that `check_poison` reports. One instruction, opaque to the
optimizer for the same reason `fill_u8`'s `rep stosb` is, and correspondingly
fast under TCG where the whole string operation is a single helper call rather
than 4096 translated loads. `check_poison` then becomes a call and a branch, and
the "~5-20ns" comment becomes true for every class rather than the small ones.

**Suspected impact, not yet measured.** This is the leading candidate for
self-test rung 27 spending 15+ s between its own label and rung 28's — long
enough that it was reported as a `SYSTEM HANG` on two boots (see
`TD-A-LIVENESS-WATCHDOG-FALSE-FIRES-ON-CAPTURED-SELF-TEST-WORK`). The watchdog's
RIP ring from that failure is eight-sixteenths inside the heap, and every
symbolised frame is on a poison path:

| samples | symbol |
|---|---|
| 2 | `mm::heap::check_poison` |
| 4 | `mm::rawmem::fill_u8` |
| 1 | `mm::heap::HeapInner::size_class_index` |
| 1 | `alloc::vec::Vec::push` |

That is evidence, not proof, and the entry should not be closed on it. **Measure
first**: time N alloc/free cycles at the largest slab class with `enable_poison`
on and off, from `clock_monotonic`, as a self-test rung that stays in the tree —
the heap is on the performance-critical list (target < 200 ns for common sizes)
and has no poison-overhead figure at all right now. Then fix, then re-measure,
and only then decide whether rung 27 is explained or whether the VFS walk itself
is also at fault.

**Severity.** Low correctness — the check is *right*, just slow, and slowness in
a debug-only path harms nothing a user sees. Medium friction: it inflates every
boot test, and it inflates it specifically in a silent stretch, which is what
turned it into two false hang reports and two wasted ~15-minute cycles.

#### TD-A-FOLD-GUESSED-AT-EVERY-ARGUMENT-IT-COULD-NOT-READ (lane A, 2026-08-25) — ✅ FIXED (`6490dae29`)

**In short:** `fold` wraps long lines to a width you give it. Ours ignored most
of what you could type at it and carried on as if it had understood — a bad
width silently became 80, `-w0` silently became 1, a second file name was glued
onto the first, and any flag it did not know became part of the file name. Worse
than any of those: a line containing an accented or non-Latin character could
come out **empty**, with `fold` reporting success. All of it is fixed, and a
self-test rung now pins each case.

**Where.** `kernel/src/kshell.rs`: `parse_fold_args` and `fold_process`, both
rewritten, plus `cmd_fold`/`cmd_fold_input`.

**What was silent.** Every row reported exit 0:

| written | what it did | what it looks like |
|---|---|---|
| `fold -w abc f` | folded at **80** | the width you asked for was honoured |
| `fold -w0 f` | folded at **1** (`w.max(1)`) | ditto |
| `fold -s -w20 f` | opened a file named `-s -w20 f` | `-s` was unimplemented, and so was the error |
| `fold f1 f2` | opened one file named `f1 f2` | two operands became one name |
| `fold f -w20` | folded at 80, then failed to open `f -w20` | flags were recognised only at the *start* of the line |
| `fold -q f` | opened a file named `-q f` | an unknown flag became part of the name |
| `printf 'zzα' \| fold -w3` | printed **nothing at all** | see below |

The last one is the reason this moved to the front of the queue. `fold_process`
measured the line in bytes (`line.len()`) and then sliced it with
`line.get(pos..end)` — and when a chunk boundary landed inside a multi-byte
character, `get` returned `None`, whose arm wrote nothing. The chunk was not
truncated or mangled; it was **dropped**, silently, and `fold` exited 0. Any
text that is not pure ASCII could lose whole runs of characters at a width that
happened to land wrong.

**Fixed by** the shape the `cut` rewrite established (`e43ef0307`):
`FoldSpec { width, spaces, bytes, files }`, a `FoldParseError` whose `report()`
uses GNU's wording, `split_words` instead of a position-dependent scan of the
raw argument line, and a `fold_run(spec, stdin)` that attempts every operand and
reports the worst status. `-` names the pipe. `-s` and `-b` are implemented
rather than ignored, short flags bundle as getopt's do, and `fold -20` is
accepted as GNU's obsolete spelling of `fold -w20`.

The measuring loop is now a restatement of GNU's `fold_file` without its `goto`:
input is split into indivisible units — a whole character, or one byte under
`-b` — each unit is measured against the width, and a unit that would overrun
ends the line and is measured again against the fresh column. A unit wider than
the whole width (a tab under `-w4`) gets a line to itself rather than vanishing
or looping. Columns follow GNU's `adjust_column`: a tab advances to the next
multiple of 8, a backspace retreats one, a carriage return returns to the left
margin, everything else counts one.

Two further faults were found while writing the single loop:

- **The final newline was invented.** `shell_println!` per line meant
  `printf 'abcd' | fold -w2` wrote `ab\ncd\n`, where GNU writes `ab\ncd`. Same
  class as the `sed` trailing-newline drift (`5e523d20a`), and fixed the same
  way — the newline that was read is the newline that is written.
- **`str::lines` ate a carriage return.** It strips a `\r` that precedes the
  `\n`, which GNU treats as data *and* as a column reset. Splitting with
  `split_inclusive('\n')` keeps it.

Covered by self-test rung 39, which asserts whole byte strings rather than
searching them: the failures here are about *where* the breaks fall and *which*
bytes survive, and only a byte comparison can see either.

**Resolution, part two — the same bug, by the other door (`c4fbbd776`).**

The paragraph that stood here said `-b` could not read input that was not
valid UTF-8, called it "the shared byte-clean issue, not a `fold` one", and
left it. Re-reading the finished rewrite, that was wrong twice over.

It was wrong about *scope*: the rewrite's whole subject is a `fold` that
answers a question it could not read, and `fold_run` still contained

```rust
let text = core::str::from_utf8(&data).unwrap_or("");
```

for every named file. A file `fold` could not decode was folded as the empty
string — no output, exit 0. That is not the shared narrowing at all; it is a
fresh instance of this entry's own bug class, arriving from the file half
instead of the pipe half, and strictly worse than the pipe's behaviour, which
at least *said* it could not cope.

And it was wrong about *cost*: making the pipe half byte-clean was not the
"shared issue" either, because the narrowing at `dispatch_with_input` is
per-command — the comment there says so — and `fold` no longer needed it.

So `fold` is now byte-clean end to end: `fold_units` takes `&[u8]`,
`fold_process` splits with `split_inclusive(|&b| b == b'\n')` and
`strip_suffix(b"\n")`, `fold_run` passes the VFS bytes straight through, and
`"fold"` moved up into the byte-clean arm of `dispatch_with_input`.

In `-b` nothing changed: every byte was already one unit. In column mode the
bytes are decoded incrementally — `from_utf8`, then `valid_up_to()` and
`error_len()` on the error — and each byte that belongs to no valid character
becomes a one-column unit of its own. That is what GNU does in the C locale,
and it is the only option that leaves the bytes alone: dropping the byte loses
data, and substituting U+FFFD *changes* it, in a program whose entire job is to
insert newlines and change nothing else. A sequence cut short by the end of
input (`error_len()` returns `None`) is written back as the bytes it is. The
loop always consumes at least one byte — `error_len()` is never `Some(0)`, and
a `None` only arises when something follows the valid prefix — so it cannot
spin.

Rung 39 grew four cases: `\xff\xfe\xfd\xfc` folds at width 2 and exits 0;
`\xff` followed by α at width 1 proves the decoder resynchronises and keeps α
whole; `zz\xce` proves a truncated sequence survives; and a *file* of
undecodable bytes folds rather than reporting itself empty — the case the
`unwrap_or("")` answered with silence and success.

**Left standing:** the same `from_utf8(&data).unwrap_or("")` is still live at
seven other sites in `kshell.rs`, each turning an undecodable file into an
empty one and exiting 0:

| function | command |
|---|---|
| `cut_run` | `cut` |
| `cmd_tr` (twice — the two-file and one-file paths) | `tr` |
| `cmd_mapfile` | `mapfile` / `readarray` |
| `cmd_base64` | `base64 -d` only — encoding already passes the bytes through |
| `cmd_sed` | `sed` |
| `cmd_awk` | `awk` |

They are all the same bug as this entry's and want the same treatment, command
by command; the `dispatch_with_input` narrowing is deliberately per-command so
each arm can move up as it converts.

`base64 -d` is the odd one out and the one to take first, because the
`unwrap_or("")` is the *smaller* of two faults sitting next to each other —
see `TD-A-BASE64-D-DESCRIBES-ITS-OUTPUT-INSTEAD-OF-WRITING-IT`.

**Noticed in passing:** `parse_cut_args` does not bundle short options, so
`cut -sd: -f1` is refused as an unrecognized option where getopt would accept
it. The fix is `parse_fold_args`'s flag loop. Not urgent — a refusal is honest,
not a silent guess — but it is a gratuitous difference from every other `cut`.

#### TD-A-BASE64-D-DESCRIBES-ITS-OUTPUT-INSTEAD-OF-WRITING-IT (lane A, 2026-08-25) — ✅ FIXED (`4ddcbd9d9`)

**In short:** `base64 -d` is supposed to write the decoded bytes. Ours writes
an English sentence about them — `<binary: 4096 bytes>` — whenever the result
is not valid text. So `base64 f | base64 -d > g` cannot reproduce `f`, which is
the one thing base64 exists to do. Exit status is 0 either way.

**Where.** `kernel/src/kshell.rs`, `cmd_base64`, the `if decode` arm
(around line 119560).

```rust
let text = core::str::from_utf8(&data).unwrap_or("");
match base64_decode(text) {
    Ok(decoded) => {
        // Print as text if valid UTF-8, otherwise show hex summary.
        if let Ok(s) = core::str::from_utf8(&decoded) {
            shell_println!("{}", s);
        } else {
            shell_println!("<binary: {} bytes>", decoded.len());
        }
    }
    ...
```

**Three faults, in order of severity.**

1. **The decoded bytes are replaced by a description of themselves.** The
   whole point of base64 is to carry data that is not text; the branch that
   handles that case is the branch that throws the data away. `shell_write_bytes`
   is right there — `fold`, `tee`, `sort` and the rest already use it — so this
   is a two-line fix, not a design problem.
2. **`shell_println!` adds a newline the input never had.** Even for the
   valid-text path, decoding `YWJj` (`abc`, no newline) writes `abc\n`. Same
   class as the `sed` and `fold` trailing-newline drift.
3. **`from_utf8(&data).unwrap_or("")` makes an undecodable input into an empty
   one.** Base64 text is ASCII by construction, so an input that fails to
   decode as UTF-8 is not base64 at all and deserves a diagnostic. Instead the
   empty string decodes successfully to zero bytes, and `base64 -d` prints a
   blank line and exits **0** — a wrong answer reported as success, the class
   `TD-A-FOLD-GUESSED-AT-EVERY-ARGUMENT-IT-COULD-NOT-READ` is about.

**Proper fix.** Take the file bytes undecoded; reject a non-ASCII input with a
diagnostic and exit 1 (GNU: `base64: invalid input`); on success write the
decoded bytes with `shell_write_bytes` and add nothing. Cover it with a
self-test rung that asserts a genuine round trip over bytes that are not valid
UTF-8 — the assertion the current code cannot pass and the reason the fault
survived: nothing yet checks that `base64 -d` produces *bytes*.

**Noticed while** making `fold` byte-clean and auditing the remaining
`from_utf8(&data).unwrap_or("")` sites in `kshell.rs`.

**Resolution (`4ddcbd9d9`).** All three faults, plus the argument handling they
were sitting on top of, which turned out to be the same silent-guess shape as
`fold`'s.

The decoded bytes now go out through `shell_write_bytes` with nothing appended,
so `base64 f | base64 -d` reproduces `f`. The input is read as `&[u8]` and
`base64_decode` refuses what is not base64 instead of decoding the empty string
and reporting success.

`parse_base64_args` replaces "look at the first word": `-d`/`--decode`,
`-i`/`--ignore-garbage`, `-w N`/`--wrap=N` (0 = never wrap) and `--help`, with
getopt-style bundling, `--` to end the options, and `-` or no operand naming the
pipe. `base64` also joins the byte-clean arm of `dispatch_with_input`, which it
had never been in at all — `cat f | base64` used to print a usage message and
fail.

The decoder tightened where it used to guess (`Xy=z` is refused rather than
read as `Xy==`; nothing may follow the padding; a partial group is an error)
and stayed lenient in the one place compatibility demands it (trailing bits,
and line breaks as structure). Those two calls are argued in
design-decisions §292.

Covered by self-test rung 40, which leads with the round trip: ten bytes that
are not valid UTF-8, encoded, fed back through `base64 -d`, and required back
byte for byte.

**Noticed in passing:** there is a *second* `base64_encode` in the kernel, at
`kernel/src/net/http.rs:1271`, with its own tests at `:1652`. Two encoders of
the same format is the duplication class `sed`'s two transform loops belong to
— one of them will drift. Neither is wrong today, so this is tech debt rather
than a bug: the fix is for `net::http` and `kshell` to share one, most
naturally in a small `base64` module, and the `net/websocket.rs` caller at
`:193` moves with it.

#### TD-A-THE-KERNEL-SHELLS-AWK-MATCHES-REGEXES-WITH-SUBSTRING-SEARCH (lane A, 2026-08-25) — ✅ FIXED

**Update, 2026-08-25.** Lane B made `ere` unconditionally `no_std` (their reply
is `requests/b-a-ere-is-no-std-now-take-it.md`; no feature flag, because Cargo
*unions* features across a build graph and `default-features = false` in one
crate cannot stop another from turning `std` back on). The kernel now depends
on it, and **`awk`'s `/pattern/` is a real POSIX extended regular expression**:
`awk_compile_pattern` builds an `ere::Regex` when the *program* is read, so an
invalid regexp is refused before any input is opened, and nothing recompiles
per record. `kshell::self_test` rung 45 pins the three rows lane B asked for —
`/^err/` anchors, `/a.c/` matches `abc`/`axc`, `/x*/` matches every line — plus
brackets, alternation, groups, an undecodable record, and the two distinct
refusals (`invalid regular expression` vs `unsupported pattern`).

**`sed` followed in the same shape** — addresses *and* `s///` in one commit, so
the command was never half a regex engine. `sed_addr_matches` takes a compiled
`SedAddr::Regex`; `sed_replace_first`/`sed_replace_all` (`str::find` and a
`str::find` loop) are gone, replaced by `sed_substitute`, which walks
`capture_spans_iter`. Three deliberate differences from `awk`:

- **BRE, not ERE.** `ere::bre::compile`, because without `-E` sed's patterns are
  *basic* regular expressions: `a+b` is three literal characters, `a\+b` is the
  repetition, `\(…\)` is a group and `(` is ordinary. Compiling them as EREs
  would pass every anchor and `.` test and still silently change what a working
  script means.
- **`&` and `\1`…`\9` are now replacement references** rather than literal text,
  parsed once into a `Vec<SedRepl>` template at script-parse time. `\N` with no
  group `N` is a *script* error (GNU: `invalid reference \3 on 's' command's
  RHS`), not an empty expansion at run time; `\U` and the other GNU case
  conversions are refused rather than emitted as their own letters.
- **An empty pattern is refused.** GNU's `s//X/` means "the last regexp used",
  which this shell does not keep; matching the empty string everywhere instead
  would interleave the replacement between every character and exit 0.

`find_unescaped` was fixed in the same commit: it looked *back* one byte from a
candidate delimiter, which cannot tell `\/` from `\\/`, so `sed 's/x/\\/'` — a
replacement of one literal backslash — was reported `unterminated command`. It
now steps over an escape instead.

`kshell::self_test` rung 46 pins all of it: the two anchors in an address and in
`s///`, `.`, the four BRE-vs-ERE spellings, `\1`, `&`, `\&`, `\\`, `g` vs not,
the zero-width-match advance (`s/x*/-/g` on `ab` is `-a-b-`, as GNU has it), an
escaped delimiter, and the four refusals — each of which must exit non-zero
*and* print nothing, since a sed that emits its input unchanged and exits 0 is
indistinguishable from one that did the edit.

**In short:** `awk '/^err/ {print}'` in the kernel shell does not match lines
that *start* with `err`. It matches lines that *contain* the four characters
`^err`, which almost nothing does — and it exits 0, so the empty output looks
like an honest "no matches". Lane B fixed exactly this in the userspace `awk`
months ago and built a shared engine so it could not come back; the kernel
shell has its own copy of `awk` that was never wired to that engine.

**Where.** `kernel/src/kshell.rs`, `awk_pattern_matches` (~122305). Its own
comment states the fault:

```rust
// /regex/ pattern — literal string match.
if pattern.starts_with('/') && pattern.ends_with('/') && pattern.len() >= 2 {
    let pat = &pattern[1..pattern.len() - 1];
    return line.contains(pat);
}
```

`sed_addr_matches` (~121581) has the same shape for `sed`'s addresses.

**What it costs.** Every metacharacter is read as itself, and every row exits 0:

| written | what it matches | what awk means |
|---|---|---|
| `/^err/` | the literal `^err`, anywhere in the line | lines beginning `err` |
| `/a.c/` | the literal `a.c` | `abc`, `axc`, `a c`, … |
| `/x*/` | the literal `x*` | every line — `x*` matches the empty string |
| `/err$/` | the literal `err$` | lines ending `err` |

The third row is the worst of them: a pattern that in awk matches *everything*
here matches almost nothing, so a filter that should be a no-op silently
discards the whole input.

**Why it was open** (the original entry; lane B has since answered). This is the
same bug as `B-FOUR-PROGRAMS-MATCHED-REGULAR-EXPRESSIONS-WITH-str::contains`,
and it has the same right answer: use the `ere` crate lane B wrote for it. But
`ere` could not be linked into the kernel —

```toml
bstr = { version = "1.13.0", default-features = false, features = ["std"] }
```

— and `userspace/**` is lane B's tree. Filed as
`requests/a-b-ere-is-std-only-so-the-kernel-shell-still-matches-regexes-with-contains.md`,
asking for a `no_std` + `alloc` build of the crate. `ere` wants `bstr` for one
function (`char_indices`), which `bstr` offers under `alloc`, so the change may
be a feature flag and some `std::` → `core::`/`alloc::` paths.

**Why not just write one here.** A second ERE in `kernel/` is the outcome the
`ere` crate exists to prevent — its Cargo.toml says so directly — and two
engines that must agree about `[[:alpha:]]`, leftmost-longest and backreference
semantics will not agree for long. A kernel-resident copy would also have to be
re-verified against gawk independently, duplicating `scripts/awk-diff.sh`.

**The fallback, had lane B declined** — not needed; they said yes. Refuse rather
than guess: any `/.../`
pattern containing a metacharacter reports `awk: regular expressions are not
supported in the kernel shell` and exits 2. Worse for the user, but a refusal
is detectable and a wrong answer is not.

**Not `grep`.** `kshell`'s `grep` matches by substring too (`grep_matches`,
~96861), but it advertises "search for pattern in files", has no `-E`, and a
fixed-string search is a defensible reading of that. It is left alone
deliberately; only `awk` and `sed`, where the syntax itself promises a regex,
are counted here.

#### TD-A-AWK-DEMANDED-A-FILE-FOR-A-PROGRAM-THAT-READS-NO-INPUT (lane A, 2026-08-25) — ✅ FIXED (`908043883`)

**In short:** `awk 'BEGIN{print "hi"}'` — the way almost everyone first uses
awk — refused to run in the kernel shell, because it insisted on being given a
file even for a program that reads nothing. Four other things it did wrong were
worse, because they *did* run and exited 0: a file it could not decode came out
empty, `-F':'` silently made `$1` the entire line, and a Windows-style text file
quietly lost a character off its last field. All fixed, and pinned by a
self-test rung.

**Where.** `kernel/src/kshell.rs`: `cmd_awk` and `cmd_awk_input`, replaced by
one `awk_run`; `parse_awk_args`, `awk_field_sep`, `awk_records`,
`awk_split_fields`, `awk_exec_action`, `awk_format_print`, `awk_eval_expr`.

**The refusal.** `cmd_awk` checked `files.is_empty()` and returned before the
BEGIN loop:

```rust
if files.is_empty() {
    shell_println!("awk: no input file specified");
    set_exit(1);
    return;
}
```

POSIX is explicit that awk reads input only if the program has a rule that
still needs it. A `BEGIN`-only program is finished when BEGIN is; an `END`
block, by contrast, *does* need input, because it reports the final `NR`. The
driver now asks `rules.iter().any(|r| !r.is_begin)` and reads only then.

**The four silent ones.** Every row exited 0:

| written | what it did | what it looks like |
|---|---|---|
| `awk '{print}' some.png` | printed nothing | the file was empty |
| `awk -F':' '{print $2}' f` | printed a blank line per record | field 2 did not exist |
| `awk -F'[,;]' '{print $2}' f` | printed the whole record as `$1`, blank for `$2` | ditto |
| `awk '{print $NF}'` on CRLF | dropped the `\r` from the last field | the file had no `\r` |
| `awk 'BEGIN{print "a   b"}'` | printed `a b` | the program said one space |

- **The undecodable file** is the shared `from_utf8(&data).unwrap_or("")` fault,
  the same one `fold` and `base64 -d` had. Records and fields are now `&[u8]`
  slices of the input and `print` writes them back through
  `shell_write_bytes`, so `awk` moves up into the byte-clean arm of
  `dispatch_with_input`. The *program* is still `&str` — it came from the
  command line, which is — but the data never is.
- **`-F':'`** kept the quotes the shell had left on it, so the separator was
  the three characters `':'`; `awk_split_fields` answered a separator it could
  not use with `alloc::vec![line]`, one field holding everything. The argument
  line now goes through `split_words`, which honours quotes — the same
  function `fold` and `base64` use.
- **The rejoin.** The old parse ran `split_whitespace` over the raw argument
  line and then rebuilt a quoted program by pushing words back with a *single*
  space between them. Runs of spaces inside a string literal did not survive
  that, so `print "a   b"` printed `a b`.
- **A multi-character `-F`** is an extended regular expression in POSIX (a
  single character is always literal, which is why `-F.` splits on dots). With
  no regex engine there are two readings, and the old code picked neither. It
  now splits **literally where the two readings cannot differ** — no
  metacharacter in the separator, which covers the overwhelmingly common
  `-F', '` — and **refuses, with exit 2**, where they can. A refusal is
  detectable; a whole record in `$1` is not.
- **The carriage return.** Records came from `str::lines`, which strips a `\r`
  that precedes the `\n`, and fields from `split_whitespace`, which breaks on
  one. awk treats `\r` as ordinary data, so `$NF == "ok"` was true on a CRLF
  file where awk says false. Records now split on `\n` alone (`awk_records`,
  which also declines to invent a trailing empty record), and the default `FS`
  is space/tab/newline as POSIX says, rather than every Unicode space.

**The duplication.** `cmd_awk` and `cmd_awk_input` carried a copy each of the
BEGIN/record/END driver — the class recorded as
`TD-A-SED-KEPT-TWO-COPIES-OF-ITS-TRANSFORM-LOOP` — and they had already
drifted: only the file copy did anything about a read error, and the pipe copy
delegated wholesale to `cmd_awk` the moment any operand was named, which threw
the pipe away even when `-` asked for it. There is now one
`awk_run(spec, stdin)`; `-` names the pipe; every operand is attempted and the
worst status is reported, so a readable second file cannot erase an unreadable
first.

Covered by self-test rung 41, which asserts whole byte strings rather than
searching them.

**Left standing at the time:** `/re/` was still a substring match, and the
statement executor still ignored anything it could not run. Both are closed —
the statements by the entry below, and `/re/` by the `ere` dependency, under
`TD-A-THE-KERNEL-SHELLS-AWK-MATCHES-REGEXES-WITH-SUBSTRING-SEARCH`.

#### TD-A-AWK-IGNORES-EVERY-STATEMENT-IT-CANNOT-RUN (lane A, 2026-08-25) — ✅ FIXED

**Fixed** by the parse-time refusal described under "What the fix looks like",
plus the pattern half of the same fault, which this entry did not originally
cover. `awk_validate_program` walks the parsed rules before any of them runs and
refuses the whole program with `awk: unsupported statement: '…'` or
`awk: unsupported pattern: '…'` and exit 2. Rationale and the decision not to
implement the missing language instead: design-decisions §294. Pinned by
`kshell::self_test` rung 42, which asserts all four rows of the table below plus
the pattern case.

Three things came with it:

- **The pattern fallback is gone.** `awk_pattern_matches` used to end in
  "treat as a literal substring match", so `awk '$1 > 5 { print }'` searched
  each record for the seven characters `$1 > 5` and printed nothing. It is now
  `awk_pattern_eval`, returning `Option<bool>`, and `None` is a refusal.
- **`NF` gained the three comparison operators it was missing.** The `NR` and
  `NF` chains were separate copies and had drifted — `NR` handled all six
  operators, `NF` only `>=`, `>` and `==` — so `awk 'NF < 3'` fell out of the
  bottom and became a substring search. One `awk_compare` now serves both. This
  is `TD-A-SED-KEPT-TWO-COPIES-OF-ITS-TRANSFORM-LOOP` in a second place.
- **A bare word in `print` is refused rather than echoed.** `print total`
  printed the five characters `total`; awk prints the value of the variable
  `total`, which here is empty. An *integer* literal is still supported, because
  that is the one case where echoing the text and evaluating it provably agree
  — a decimal is not, since awk formats `5.0` through `OFMT` and prints `5`.

**`/pattern/`, the carve-out this entry left standing, is now closed** — not by
a refusal but by the engine, which was always the better answer. The kernel
links `ere`, and `awk_compile_pattern` builds a real `ere::Regex` from the
pattern text. That also turned §294's check from a convention into a type:
`awk_validate_program` used to evaluate each pattern against a dummy empty
record and ask only whether an answer came back, which was sound only because
of a hand-written invariant ("whether an answer exists depends on the pattern
alone") that nothing enforced and that rung 42 had to pin by hand. It is now
`awk_compile_program`, whose *output* is what runs, and
`awk_compile_pattern(&str)` cannot see a record at all. See
`TD-A-THE-KERNEL-SHELLS-AWK-MATCHES-REGEXES-WITH-SUBSTRING-SEARCH`.

The original entry follows.

**In short:** the kernel shell's `awk` understands exactly one statement,
`print`. Anything else — an assignment, an `if`, a function call — is skipped
in silence and the program reports success. So a program that does real work in
a real awk does *nothing* here, and says it worked.

**Where.** `kernel/src/kshell.rs`, `awk_exec_action`:

```rust
} else {
    // Unknown statement — ignore.
}
```

**What it costs.**

| written | what it does | what awk does |
|---|---|---|
| `awk '{ n = n + 1 } END { print n }'` | prints an empty line | prints the count |
| `awk '{ if ($1 > 5) print }'` | prints nothing, ever | prints the matching records |
| `awk '{ printf "%s\n", $1 }'` | prints nothing | prints field 1 |
| `awk '{ sub(/a/, "b"); print }'` | prints the record unchanged | prints it substituted |

Each exits 0. The last row is the shape that matters most: the output *looks*
like plausible awk output, so nothing downstream can tell the substitution
never happened.

**What the fix looks like.** Report and stop, rather than skip:
`awk: unsupported statement: '<stmt>'` on stderr with exit 2, checked once when
the program is parsed rather than per record — gawk reports a syntax error
before reading any input, and reporting per record would emit one line per
input line. `parse_awk_program` already walks every rule, so the check belongs
there.

**Why it was not done with `908043883`.** That commit fixed the driver — which
sources are read, and what the bytes are — and this is about the *language*.
Bundling them would have made one commit that could not be reverted in halves,
and the refusal needs a decision the driver fix did not: whether a program
using an unimplemented statement should fail at parse time (gawk's behaviour,
and it means `awk '{n=1} {print}'` stops printing) or fail only when the
statement is reached. Parse-time is almost certainly right, but it is a
user-visible behaviour change to a command that currently "works", so it wants
its own commit and its own rung.

**Not a regression.** This has been true since the command was written; the
`908043883` rewrite neither caused it nor made it worse. It is recorded now
because the rewrite is what made it visible.
