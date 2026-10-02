## `B-XARGS-IS-A-STUB-AND-DASH-ZERO-CANNOT-CARRY-THE-BYTES-IT-EXISTS-FOR` (lane B, 2026-08-29) -- **FIXED 2026-08-29** (`631783b54`); see "Resolution" at the end

**In short:** `userspace/coreutils/src/bin/xargs.rs` is a 285-line invention, not
a transcription of GNU findutils' 1755-line `xargs.c`. The worst consequence is
that `xargs -0` -- the mode whose entire purpose is carrying arbitrary bytes from
`find -print0` -- reads stdin with `read_to_string` and so fails on exactly the
non-UTF-8 filenames it exists to handle. `find . -print0 | xargs -0 rm` is broken
for the case `-0` was invented for.

**Where:** `userspace/coreutils/src/bin/xargs.rs` -- `run_main()` line 34
(`env::args().skip(1).collect::<Vec<String>>()`), line 42
(`io::stdin().read_to_string`), `split_items()` line 152, `parse_args()` line 108.
Listed in `scripts/argv-utf8-baseline.txt` as `xargs.rs:argv-as-string`.

**Measured against `/usr/bin/xargs` (GNU findutils 4.9.0) on the dev machine.**
Each row was run, not assumed:

| # | Case | GNU 4.9.0 | Our stub |
|---|---|---|---|
| 1 | `printf "a'b c'd\n" \| xargs echo` | `ab cd` (one item; quotes are syntax) | `a'b c'd` (two items, quotes literal) |
| 2 | `printf 'x\ y\n' \| xargs echo` | `x y` (backslash escapes the space) | two items |
| 3 | `printf "a'b\n" \| xargs echo` | diagnoses `unmatched single quote` | accepted silently |
| 4 | empty input, no `-r` | **runs the command once** (`RAN`) | returns success without running |
| 5 | child exits 1 | xargs exits **123** | exits 1 |
| 6 | child exits 255 | xargs exits **124** | exits 1 |
| 7 | command not found | xargs exits **126** (see below) | exits 1 |
| 8 | `printf 'caf\351\0' \| xargs -0 ...` | passes the raw byte through (`63 61 66 e9`) | cannot represent it |

**Row 7 is 126 because of this machine's `PATH`, not because of `xargs`** --
established 2026-08-29, after the first measurement recorded the 126 without an
explanation and warned the next reader off "correcting" it. Both numbers are
real and the rewrite must produce both:

| `PATH` | status | diagnostic |
|---|---|---|
| the ambient WSL one | **126** | `xargs: nosuchcmd: Permission denied` |
| a single readable directory | **127** | `xargs: nosuchcmd: No such file or directory` |

`xargs.c:1360` is unambiguous -- `_exit (errno == ENOENT ? 127 : 126)` -- so the
variable is `errno` after `execvp`, and `execvp` is where the choice is made.
glibc searches every `PATH` entry and, having found nothing executable, reports
the *most specific* failure it saw rather than the last one: any `EACCES` on the
way beats the `ENOENT` at the end. This WSL's inherited `PATH` contains three
Windows-interop directories that are not searchable --
`/mnt/c/Program Files/Git/usr/local/bin`,
`/mnt/c/Users/Public/Documents/Embarcadero/Studio/23.0/Bpl/Win64`, and
`/mnt/c/WINDOWS/system32/config/systemprofile/AppData/Local/Muse Hub/lib` --
so the search yields `EACCES` and xargs exits 126.

Consequences for the transcription, both of which `scripts/xargs-diff.sh`
depends on:

- **Do not hard-code either number.** Ours must take the branch on its own
  `errno`, so that it tracks the reference on any host. A transcription that
  wrote `126` because that is what was measured here would fail everywhere else,
  and one that wrote `127` because that is what the source "says" fails here.
- **The harness sees 127, not 126.** `diff-wsl.sh` runs each side with `PATH`
  set to a single directory holding one symlink, which is exactly the clean-path
  row above. That is a feature -- it is the row that is a property of `xargs` --
  but it means the ambient-`PATH` 126 is *not* covered by the harness and is
  recorded only here.
- A non-executable file that *is* found is 126 on any `PATH` (`EACCES` from the
  file itself), and an absolute path that does not exist is 127 on any `PATH`
  (no search happens). Those two are the unambiguous cases and both are in the
  harness.

Row 4 matters more than it looks: the stub behaves as though `-r`
(`--no-run-if-empty`) were permanently on, so a script relying on the one
guaranteed invocation silently gets none.

**Also entirely absent:** `ARG_MAX` splitting. GNU's whole reason for existing is
building command lines up to the system limit (gnulib `lib/buildcmd.c`, 638
lines); our default path appends every item to a single command and will `E2BIG`
on large input. And 15 of 18 long options are missing -- `--arg-file`,
`--delimiter`, `--eof`, `--max-lines`, `--max-chars`, `--interactive`,
`--no-run-if-empty`, `--verbose`, `--show-limits`, `--exit`, `--max-procs`,
`--process-slot-var`, `--open-tty`, `--help`, `--version`.

**One upstream bug found while writing the harness, which we deliberately do not
copy.** `xargs -o` with no controlling terminal aborts GNU 4.9.0 rather than
diagnosing: the child's `/dev/tty` open fails, it dies through `die()` -- which
is `exit()`, not `_exit()` -- and so runs the *parent's* `atexit` hook, whose
first act is `assert (getpid () == parent)`. Measured:

```
xargs: '/dev/tty': No such device or address
xargs: xargs.c:1605: wait_for_proc_all: Assertion `getpid () == parent' failed.
xargs: echo: terminated by signal 6
```

The comment at `xargs.c:1600` says in as many words that child processes must
not call `exit ()` for exactly this reason; this path does. Ours should print
the first line and exit, and `scripts/xargs-diff.sh` therefore tests `-o` for
its option parsing only, with the reason written where the cases would be.

**Harness:** `scripts/xargs-diff.sh`, 319 cases. It cannot compare `echo`'s
output -- `echo` cannot show an empty argument, an argument containing a blank,
or how many times the command ran -- so both sides run a fixture that prints the
argument vector raw and NUL-terminated, and the whole stream is compared with
`od -An -c -v`. Baseline on the stub, 2026-08-29: **62 passed, 257 differed.**
Control run (`OURS=/usr/bin/xargs`): 319 passed, 0 differed.

**Proper fix:** transcribe `xargs.c` + `buildcmd.c` the way `df`, `du` and `ls`
were done, carrying argv and stdin as bytes throughout.
Reference tarball matches the installed binary exactly (findutils 4.9.0).

### Resolution, 2026-08-29 (`631783b54`)

Done as described: `xargs.c` and `buildcmd.c` transcribed into 2309 lines,
argv and stdin carried as bytes end to end. Every row of the table above now
agrees with GNU, and `xargs.rs:argv-as-string` is out of
`scripts/argv-utf8-baseline.txt`. **12 findings remain across 11 files** --
`diff ed fetch grep logger more patch ps sh tar time_cmd`, where `sh` carries
two of them (`argv-as-string` and `env-as-string`). The two numbers are easy
to confuse and `631783b54`'s commit message got it wrong, saying "11
findings"; `python scripts/argv-utf8.py --check` is the authority and prints
the finding count, not the file count.

**Harness after the rewrite: 334 passed, 0 differed, 2 differ on purpose** --
up from 319 cases because the transcription exposed a gap in the harness
itself, described below. Control run (`OURS=/usr/bin/xargs`): 334 passed, 0
differed, 2 XPASS, which is what the control is supposed to report for the two
`xfail`s.

**The 126-vs-127 requirement is met the way this entry asked for.** Nothing
compares against either constant: `Command::spawn()` delegates to libc's
`PATH` search, so its `io::Error` *is* the `errno` glibc chose, and the code
branches on `ErrorKind::NotFound` -- which is `ENOENT` -- exactly as
`xargs.c:1360` branches on `errno == ENOENT`. Re-measured against
`/usr/bin/xargs` after the rewrite, all four rows agree -- including the
ambient-`PATH` 126 that `diff-wsl.sh` cannot cover and that is therefore
recorded only here:

| Case | GNU | ours |
|---|---|---|
| `nosuchcmd`, ambient WSL `PATH` | 126, `Permission denied` | 126, `Permission denied` |
| `nosuchcmd`, `PATH` = one readable dir | 127, `No such file or directory` | 127, same |
| a mode-644 file found on `PATH` | 126, `Permission denied` | 126, same |
| `/nonexistent/cmd`, no search at all | 127, `No such file or directory` | 127, same |

#### The gap the harness had: every non-UTF-8 case put the byte in the *input*

This entry is about argv, and the 319-case harness tested the high byte only
on the side that already worked. Fifteen cases were added for the other side --
a byte that is not UTF-8 arriving as an INITIAL-ARG, around the `-I` pattern
(both directions: a non-UTF-8 item into an ASCII argument and an ASCII item
into a non-UTF-8 one), as `-E`'s logical EOF string, as a raw `-d` delimiter,
as `--process-slot-var`'s name, and as `-a`'s operand.

That last one is the load-bearing one and it settled a question that had been
open since `quoting`'s curly marks were introduced: **`-a` on a name that does
not exist makes the diagnostic quote a name it cannot decode**, and no ASCII
case can arbitrate what should happen to the undecodable byte. Ours and GNU's
`Cannot open input file ‘no<0xe9>such’: No such file or directory` agree byte
for byte, so `Style::locale_quote`'s output is right for this case and not
merely plausible.

#### Five upstream behaviours kept because they are observable, not because they are correct

Recorded here rather than only in the source, because each is the kind of thing
a later reader "fixes":

1. **`input_delimiter` is a C `char`, which is signed on x86-64.** `-d` with a
   byte at 0x80 or above sign-extends to a negative int, which can never equal
   a `getc` result in 0..255, so **the input never splits at all** and the whole
   stream arrives as one argument. Verified against GNU at 0xe9 and 0xff (no
   split, both) and 0x7f (splits, both). Ours stores `(b as i8) as i32` to
   reproduce it.
2. **`bc_push_arg` stores with `strcpy` but charges `cmd_argv_chars` the full
   length**, so an item holding an embedded NUL reaches the child truncated
   while still consuming its untruncated size against `-s`.
3. **`--show-limits` subtracts the environment size twice** -- once inside
   `posix_arg_size_max` and again in the "Maximum length of command we could
   actually use" line -- so that number is deliberately pessimistic. It also
   reads the environment *live*, so `--process-slot-var`'s `unsetenv` shrinks it.
4. **`parse_num` always names the short option letter**, so `--max-lines=0` is
   reported against `-l`; and its "invalid number" branch exits regardless of
   the `fatal` argument, because `usage (EXIT_FAILURE)` is followed by an
   unconditional `exit`. The `-s` "value too large" warning below it is
   unreachable for the same reason.
5. **`-i -n1` is excused but `-n1 -i` warns** (savannah patch #1500). The order
   matters and both orders are in the harness.

#### Five divergences, all forced by Rust rather than chosen

Documented in the module header as well:

- `endbuf` saturates where C does pointer arithmetic past the end of the buffer.
- `bc_do_insert` copies bytes rather than `strcpy`ing the item, and guards the
  zero-length match that makes upstream loop forever on `-I ''`.
- Children are reaped in slot order rather than arrival order.
- `-o`'s stdin redirect happens in the parent, so a missing `/dev/tty` exits 1
  with a diagnostic instead of aborting in the child -- which is the upstream
  `assert (getpid () == parent)` bug this entry already describes, and the
  reason not copying it was the plan.
- `--process-slot-var` gives each child an explicit `env` entry instead of
  `unsetenv`ing the variable in the parent, since `std::env::remove_var` is
  `unsafe` in edition 2024. The only observable part is the environment size,
  which `--show-limits` reports and which is decremented to match.

**Not fixed here, and unchanged by this work:** `userspace/xargs/` is a separate
1109-line implementation of the same utility, one of the 41 duplicated binary
names blocked on `B-Q7`. It was neither converted nor deleted; the gate covers
`userspace/coreutils/` only, so the twin is surveyed but not gated.

### Lesson 65: a check that reuses the assumption it is checking will agree with itself (lane C, 2026-08-29)

**In short:** a script converted thirteen `mutate.py` files to a shared harness,
copying each file's `MUTATIONS` table across verbatim. A second script checked
that no table was damaged in the move. Both found the table the same way -- look
for the statement `MUTATIONS = [...]` -- and one of the thirteen builds its table
in *two* statements, `MUTATIONS = [...]` followed by `MUTATIONS += [...]`. The
converter kept the first and dropped 55 of `wordsearch`'s 103 mutations; the
checker compared first-to-first, found them identical, and printed `OK`. The
check was not weak, it was **blind in exactly the place the converter was**.

**The shape of it.** The verifier was not lazy -- it was already careful. It used
`ast` rather than text scanning (an earlier version had scanned for a closing
`]` and stopped at a `]` that appeared *inside* a mutation's replacement text).
It normalised CRLF against LF so the five CRLF files would not report a false
difference. It compared the table body byte-for-byte *and* counted the entries
with `ast.literal_eval`. Every one of those refinements was real, and none of
them mattered, because the question it asked was "does the `MUTATIONS`
assignment match?" and the defect was "there is a second statement". Refining an
answer does not widen a question.

**Why the line count nearly did not save it.** The only visible symptom was that
`wordsearch` shrank by 428 lines where the other twelve shrank by 52-143. That
is the sort of number it is very easy to explain away -- "it has a long
docstring", "it had more boilerplate" -- and the verifier saying `OK` is exactly
the authority you would use to explain it away with. A green check next to an
anomalous number is more dangerous than no check at all, because it converts a
question into a settled matter.

**The fix, and the general form.** Compare *outputs*, not *the artefact you
believe produces them*: the verifier now `exec`s both modules and compares the
`MUTATIONS` lists they actually build, so however a file assembles its table --
one statement, two, a loop, a comprehension -- the check sees the result. That
is the rule. **When you verify a transformation, observe the transformed thing's
behaviour, not its structure** -- because your model of its structure is the
thing most likely to be wrong, and it is precisely the model the transformation
was built on.

**Where else to look.** Any migration validated by a script that shares a parser,
a schema, a glob, or a "find the interesting node" helper with the migrator: a
config rewriter checked by re-reading with the same loader; a codemod verified by
the same AST query it used to match; a data backfill audited by the query that
selected the rows to backfill. If the same assumption is on both sides of the
equals sign, the equation is `x == x`.

### Lesson 66: a harness that never watched the tests pass cannot tell "caught" from "nothing ran" (lane C, 2026-08-29)

**In short:** extracting that shared harness put `mutation_harness.py` in
`apps/`. `apps/*` is a Cargo workspace member glob, so the `apps/__pycache__/`
directory Python created on first import was read by cargo as a crate, and every
`cargo test` in the entire tree began failing with "failed to load manifest for
workspace member `apps/__pycache__`" -- before compiling anything. The mutation
sweep then ran 20 mutations, saw a non-zero exit and no named test failures every
time, classified all 20 as `[ok] caught -- the harness died`, printed
**"OK: all 20 mutation(s) caught by the tests named for them"**, and exited 0.
Not one test had been compiled, let alone run. And `__pycache__/` is in
`.gitignore`, so the directory that broke the build never appeared in
`git status`.

**Why the harness believed it.** Its classifier had a rule for the genuine case
where a mutant kills the test process before any test can report -- an abort, a
stack overflow -- which cannot be recognised by a named failure because there is
no name to read:

```python
crashed = compiled and not timed_out and not failed and out.returncode != 0
```

Every clause of that was true of a cargo that never started. `compiled` was
computed as `"could not compile" not in output`, and the manifest error is not a
compile error, so a run that compiled *nothing* scored `compiled = True`. The
predicate meant to say "the tests ran and died" actually said "something exited
non-zero", and those are the same sentence only when you already know the tests
ran.

**The two fixes.** *(1)* The classifier now requires positive evidence that a
test binary started -- `^running \d+ tests?$` in the output -- and a mutant that
compiles but produces no such line **stops the sweep** rather than earning a
verdict, because the tree changed underneath the run and nothing after that point
would mean anything either. *(2)* More importantly, `sweep` now runs the suite
once against the **unmutated** source before it mutates anything, and refuses to
start unless it compiled, ran, and passed. That single run is the whole class of
defect: a sweep that has never once observed the tests passing on the real
program has no baseline against which "the tests failed" is information.

**The rule.** *Before you accept failure as evidence, prove you can observe
success.* A negative-result harness -- mutation testing, fault injection, chaos
testing, a regression bisect, anything whose green condition is "the thing I
broke got noticed" -- must first demonstrate the unbroken case, or it cannot
distinguish "my sabotage worked" from "this was already broken". The failure mode
is silent and self-confirming: the more thoroughly the environment is broken, the
more mutations get "caught", and the louder the harness insists everything is
fine.

**And the location fix.** `mutation_harness.py` now lives in `scripts/`, which is
not a workspace member. The general point is smaller but sharp: **a directory
covered by a build-system glob is not a neutral place to put a file.** Dropping a
Python module, a README generator, or a scratch script into `apps/`, `crates/`,
`packages/*` or any other globbed member directory can add a member to the build
-- and the artefact that does it may be one the tool creates on its own, and one
your VCS is configured not to show you.

### Lesson 67: a test over a branch must prove the branch was entered (lane C, 2026-08-29)

**In short:** rush's `every_string_drawn_is_inside_the_window` measured every
string the frame drew and asserted it fitted the window. It passed. It could not
have failed, because at none of the ten window sizes it ran at was any string
wider than the space it was given — so the two mechanisms that keep a long string
inside a narrow window, the `max_width` limit and the footer's clip rect, were
never once the reason the assertion held. The test named them and exercised
neither. Two more in the same suite were vacuous the same way: the branch that
drops a car's letter when the glyph is taller than the car was never reached
(cells were never that small), and no string was ever elided.

**Why it is not the same as an untested line.** An untested line is *visibly*
untested — coverage says so, and mutating it produces a surviving mutant nobody
can explain. This is worse, because the test *names* the thing and appears in the
mutation table opposite it. The table entry "the footer's lines are not clipped
to the footer → `every_string_drawn_is_inside_the_window`" is a claim that a
human wrote and a sweep would have to disprove. Here the sweep would have
disproved it — the mutant survives — but only if the mutation was written in the
first place, which is exactly the point at which the hole was found. Had the
mutation table skipped that line as obviously-covered, nothing would ever have
said otherwise.

**The tell.** The three tests share a shape: an assertion of the form "for every
X, P(X)", where the interesting case is the one where P is *nearly* false. A
universally-quantified assertion is satisfied most easily by a fixture set in
which nothing is close to the boundary — and a fixture set is chosen for
convenience, so it will drift toward exactly that. The list of window sizes had
been picked to be "some small, some big"; nothing in it was *narrow*, because
narrow-and-tall is not a shape anyone reaches for.

**The fix, and it generalises.** Each test now carries a witness that the branch
was entered, checked after the loop:

```rust
assert!(
    cut_somewhere,
    "no window in the list is narrow enough to cut a single string, so the \
     clip and the width limits are branches this test never enters"
);
```

and a window (170x900) was added in which the footer's second line and the header
title are each wider than the whole window. The counting form — `dropped > 0` —
does the same job for the glyph test. **Any test whose subject is a conditional
should assert, in the test, that the condition was met at least once.** It costs
one line and it converts a test that cannot fail into one that fails the day
someone tidies the fixture list.

**And it found a real fault.** The window added to make those three tests real
immediately exposed one: at 170x900 the header drew its title at the left with no
width limit while drawing its counters against a flat 120-pixel reservation, so
the two were painted through each other. The fault had been there since the
rewrite, in code whose whole stated purpose was to stop text being positioned by
guessing. The vacuous test was not merely failing to test the mechanism — it was
concealing a live bug in it, which is the ordinary consequence and worth
expecting rather than being surprised by.

**Seen four more times the same day, in the same suite, by the sweep.** Once the
mutation table existed, four more of rush's tests turned out to name something
they never reached — and the shape was different each time, which is why "add a
witness" has to be a habit rather than a rule with one fixed form:

| Test | What it never reached |
|---|---|
| `ids_are_unique_and_never_a_position_in_the_vector` | Called `game()` and *then* `load_puzzle(i)`. `game()` has already spent the low ids, so every board it looked at had ids well past the index range whatever the counter started at. The one board where a zero-based counter puts id 0 at index 0 is the opening position — the board it skipped. Klotski's fault 14, repeated exactly, in the very next app wired — which is the strongest argument here for the witness being mechanical rather than remembered. |
| `clicking_past_a_blocker_slides_as_far_as_the_yard_allows` | Aimed at a cell the car could actually reach, so the clamp it is named after had nothing to clamp. It passed identically against a program that never clamps. |
| `every_centred_string_is_limited_to_the_box_it_is_centred_in` | Guarded its whole body on `max_width.is_some()`, so a string that *lost* its limit was skipped rather than caught. A test whose subject is "X is always there" must not use X's presence as its filter. |
| `the_board_survives_every_window_a_band_is_dropped_in` | Ran at ten window sizes, none of which dropped the band whose absence the line under test exists to survive. |

The common cause is not carelessness about the assertion — every one of these
asserts the right thing. It is that **the fixture and the assertion are chosen at
different moments**: the assertion states the rule while it is fresh, the fixture
is whatever was already lying around in the test module. That is also the
argument for the witness being an `assert!` inside the test rather than a note in
a comment: it is re-checked every run, at the same moment as the assertion it
guards, so the two cannot drift apart later.

### Lesson 68: a containment assertion has slack, and any fault that fits inside the slack is invisible (lane C, 2026-08-29)

**In short:** "the button is inside the band" and "the button is *centred* in the
band" are different claims, and only the second one is a placement. The first is
an inequality with room to spare, so a fault that moves the thing by less than
that room passes it. Sokoban's control-button test compared coordinates — it was
not lesson 57's mistake of checking only that a thing was drawn — and still could
not see the buttons slide to the top edge of the band they sit in.

**The fault.** `button_rects` gives each button a height of `controls.h - pad`
and places it at `controls.y + (controls.h - bh) / 2.0`, i.e. half a pad of
breathing room above and half below. The test asserted:

```rust
r.y >= l.controls.y - 0.01 && r.bottom() <= l.controls.bottom() + 0.01
```

Replace the `y` with a bare `controls.y` and the button moves down by half a pad
— visibly, in the drawn picture — and the assertion still holds, because
`controls.y >= controls.y` and the bottom edge, now `controls.y + controls.h -
pad`, is *further* inside the band than before. The mutation moved the button and
the test that owns button placement said nothing. The centring was, in effect,
untested code that happened to be correct.

**The rule.** *An assertion of the form `a <= x <= b` tests the endpoints, not the
value. If the value is determined by a formula — centred, thirds, golden — assert
the formula, not the interval it lands in.* The replacement is one line and says
exactly what the code does:

```rust
((r.y - l.controls.y) - (l.controls.bottom() - r.bottom())).abs() <= 0.01
```

"The gap above equals the gap below" is what the word "centred" means, written
down. Note that it is stated as a relation between two measured gaps rather than
as `r.y == controls.y + (controls.h - bh) / 2.0` — the latter is lesson 65's
mistake, a check that recomputes the thing it is checking and therefore agrees
with any formula the production code happens to hold.

**How it was found, and why that matters.** Not by reading the test — by trying
to write a mutation for a guard that had just been deleted. Faults 16 and 17 in
sokoban were five-plus-three unobservable guards (lesson 51), and deleting them
left three mutation slots with nothing to break. Reaching for something else in
the same function to break is what surfaced the hole. **Deleting dead defensive
code is worth doing for its own sake, but the mutation slot it frees is the more
valuable half**: it forces you to ask what else in that function is load-bearing,
and the answer is usually a line no test has ever disagreed with.

**Where else to look.** Every "stays inside the window", "does not overlap",
"fits in its box" assertion in these suites — they are the commonest shape in the
layout tests and every one of them is an interval. They are the right test for
the fault they were written against (a thing drawn off the edge) and say nothing
about position within the box. The tell is a test whose name contains *inside*,
*within*, *stays*, *fits*, or *does not overlap* and whose subject is computed by
a formula with a division in it.

### Lesson 69: a tool that matches bytes and a table written as text disagree about what ends a line (lane C, 2026-08-29)

**In short:** the mutation harness reported 35 of wordle's 75 anchors as
appearing "0 times" in a file that plainly contained every one of them. The
anchors were written in a Python file with `\n` between their lines; the Rust
file on disk had `\r\n`, because a splice script had written it with
`pathlib.Path.write_text`, which translates line endings to the platform's on
the way out. Every single-line anchor matched and every multi-line one did not,
and the harness printed the same verdict — `SKIP anchor appears 0x` — that it
prints for an anchor genuinely edited away.

**Why it took a while to see.** The verdict is honest and it is the right
verdict; it just has two causes with nothing to tell them apart. A sweep whose
survivors are scattered across nine sections of the program reads as "the suite
has holes all over", which is a plausible thing for a new suite to be, and it
sends you off reading tests. The tell — obvious afterwards — is the *shape* of
the failure: every multi-line anchor failed and no single-line one did. A defect
that respects a syntactic property of the anchor rather than a semantic one is a
defect in the matching, not in the tests.

**Two fixes, and both are wanted.**

1. **The file.** `apps/wordle/src/main.rs` was normalised to LF. Nothing in this
   tree wants CRLF: the committed blobs are LF and `core.autocrlf` is unset, so
   only the working copy was affected — and only the copy of it a Python script
   had rewritten.
2. **The harness.** `scripts/mutation_harness.py` reads and writes the source
   with `newline=""` so a restore is byte-identical; that is deliberate and must
   stay. It now also detects the source's line ending once and translates every
   anchor into it before searching:

   ```python
   eol = "\r\n" if "\r\n" in original else "\n"
   ```

   A harness that can only be driven from a file with one particular line ending
   is a harness that will one day report a clean sweep because it broke nothing
   at all — which is the failure lesson 66 was written about, arriving by a
   different road.

**The rule.** *Any Python that rewrites a source file in place must pass
`newline="\n"`, or read and write bytes.* `write_text` is not a round trip on
Windows: `read_text` strips the `\r`, `write_text` puts it back, so a script that
reads a file, changes one line and writes it out rewrites all 3,828 of them. The
`git diff` looks the same either way — git normalises — which is what lets it
through review.

**Where else to look.** Every splice or patch helper in `scripts/`, and every
throwaway `python -c` that edits a `.rs` or `.md` in place. The damage is
invisible until some other tool matches on bytes.

**How widespread it already is.** Fifty tracked files carry CRLF — `git ls-files`
plus a byte count finds them — headed by `apps/settings/src/main.rs` (8,227
lines), `apps/vpnmanager`, `apps/netmanager`, `apps/indexer`, `gui/credentials`,
`gui/notifications`, and eleven apps that already have windows and suites. In
every one of them the *committed blob* has the CRLF too, so this is history and
not a stale checkout, and `git checkout` will not fix it. **They are not being
normalised**: a fifty-file whitespace-only commit would bury the next few real
diffs and conflict with anything the other two lanes have in flight, and the
harness translation above already makes them safe to mutate. The cost of leaving
them is that a hand-written `grep`/`sed`/`python` one-liner against any of those
files must still expect `\r\n`; the cost of fixing them is paid by everyone
reading history for a year. If one is being rewritten wholesale for another
reason — as wordle was — normalise it in that commit.

### Lesson 70: a movement that is clamped can only be tested where it is free to move (lane C, 2026-08-29)

**In short:** nonogram's mutation sweep left five survivors, and four of them
were the same mistake wearing different hats. Every arrow-key test pressed its
arrow *at the edge the arrow stops at* — `Up` at row 0, `Left` at column 0, `Up`
at the top of the puzzle list. At that spot the guard (`if self.cursor_row > 0`)
refuses the key, so the line the guard protects never runs; and if you delete
the guard, `saturating_sub(1)` on 0 is still 0. Two different faults — a missing
bound check and a movement that goes the wrong way — both produce exactly the
cursor position the test asserts. The fifth survivor is the same shape in
geometry: the centring test measured vertical slack in a fixture where the grid
was height-constrained, so the slack was zero and "centred" and "flush against
the top" were the same number.

**The two halves of the rule.**

1. **To test a *direction*, press the key somewhere it can actually move.** `Up`
   from row 1 must land on row 0. Pressed from row 0 it asserts nothing about
   which way up is — the guard answers first, and `saturating_sub` answers
   after it.
2. **To test a *bound*, assert the verdict, not just the position.** At the edge
   the position is unchanged whether the key was refused or applied-and-clamped;
   what distinguishes them is that a refused key returns `EventResult::Ignored`
   and a clamped one returns `Consumed`. The window is told to repaint in the
   second case and not the first, so this is a real user-visible difference and
   not a technicality.

Written out, the pair covers a movement key completely: from an interior cell,
`Consumed` and the cursor one step in the named direction; from its own edge,
`Ignored` and the cursor where it was.

**And the geometry half.** A "centred" assertion of the form *slack on this side
equals slack on that side* is satisfied by `0 == 0`. `Grid::new` sizes its cells
by `min(width_fit, height_fit)`, so in **any** fixture exactly one axis is fully
consumed and has no slack at all. A single fixture can therefore only ever test
centring on one axis; testing both needs a wide area *and* a tall one, and each
assertion needs `assert!(slack > 0.0)` in front of it to prove it was measuring
something. This is lesson 67 (a test over a branch must prove the branch was
entered) arriving through arithmetic rather than control flow.

**Where else to look.** Every app with a cursor that clamps — sudoku, minesweeper,
2048, connect4, the file explorer's list — and every `(a - b).abs() < eps`
centring assertion in a layout suite. The tell is a test whose fixture puts the
thing under test at rest.

### Lesson 71: asking *whether* a click landed cannot tell a hit box in the right place from one a whole gap away (lane C, 2026-08-29)

**In short:** snake's board grows each square's click box by half the gap on
every side, so the gap between two squares is split down the middle and a click
in it goes to the square it is nearer. The test for that clicked the *middle* of
a gap and asserted the result was one of the two squares either side. Mutate the
growth so it happens on one side only — `let half = 0.0;`, leaving the
`r.w + self.gap` that widens it — and the box is still exactly the right size,
still leaves no gap belonging to nobody, and the test still passes. What has
changed is that every click box has slid a whole gap off its own ink: clicking
the left edge of a square now hits its left-hand neighbour, and the last column
of the board has a strip of dead ink past which the clicks fall off the board
entirely. Nothing in the suite noticed.

**The rule.** A hit-box assertion must name the target, not merely require one.
"Landed on something" is satisfied by a box of the right size in the wrong
place; "landed on *this* square" is not. Where a box is derived from ink by
growing it, click a point on **each** side of the ink and say which square owns
it — a box that has slid shows up on one side as the wrong neighbour and on the
other as nothing at all. Pick the point a quarter of the way into the gap rather
than half, so it is unambiguously in one square's half of it and the assertion
is an equality rather than a disjunction.

This is lesson 68 (a containment assertion has slack, and any fault that fits
inside the slack is invisible) arriving through the hit map instead of through
geometry: the `a || b` **is** the slack, and a box displaced by exactly one gap
is the fault that fits inside it.

**Where else to look.** Every `hit_test` assertion written as
`landed == Some(a) || landed == Some(b)`, and every control whose click box is
bigger than its ink: the padded switches in these apps' footers, list rows grown
to the row pitch, and any slider whose thumb carries a grab margin. The tell is
an assertion whose right-hand side is a set rather than a value.

### Lesson 72: "did everything I named fail?" is satisfied by naming nothing (lane C, 2026-08-29)

**In short:** the mutation harness decides a mutation was caught by asking
whether the tests the table named for it are among the tests that failed —
`set(expect) <= failed`. Subset is the right relation for the question, and it
has one answer nobody wanted: the empty set is a subset of everything. A row
whose `expect` is `[]` is therefore scored **caught** no matter what happened —
including when not one test failed. It is a mutation that cannot fail, sitting
in a table whose entire purpose is to fail.

`apps/maze/mutate.py` has carried such a row since it was written. It is there
for an honest reason, and the comment above it says so: deleting the "have I
already numbered this cell?" guard makes the breadth-first search re-queue cells
forever until the binary is killed on a two-gigabyte allocation, so *no named
test can report it* — the process dies before any test prints a verdict. There
is no name to put in the list, so the list was left empty.

That works only by accident of arm order. `sweep` tests `crashed` before it
tests the expectation, so the row has always been scored by the crash arm and
the empty list never consulted. Change the maze small enough that the runaway
allocation completes, or give the process more memory, and the row silently
stops being an assertion: it slides one arm down, `set() <= failed` accepts the
empty failure set, and the sweep prints `[ok]` for a mutation the suite did not
notice. Nothing in the output distinguishes that from a real catch.

**The rule.** A subset check needs a non-empty left side, or an explicit arm for
the empty one. Where "no named test can see this" is a legitimate thing for a
table to say, make the harness *require* what it means instead of letting it
fall through the general case: an empty `expect` now asserts the program died,
and a row that stops dying is reported `[BAD] expected the program to die; it
survived`. The spelling that was the most dangerous row a table could hold is
now the one that checks the hardest thing.

**How it was found.** Not by reading, but by running the new up-front table
check (`check_the_table`) over all thirteen `apps/*/mutate.py` at once. It had
been written to catch mis-indented anchors — snippets' first sweep spent 296
seconds to report that 3 of 19 anchors did not match — and the empty expectation
came out of the same pass for free. That is the argument for checking a table as
a table: the classes you did not think to look for are found by the pass you ran
for the class you did.

**Where else to look.** Any predicate of the shape "all of X are in Y" where X
is supplied by data rather than by code: `set(required) <= set(present)`,
`all(f(x) for x in xs)`, `xs.iter().all(...)`, and every "assert these all
appear" helper in the app test suites. Each is vacuously true on empty input,
and each is fed from a table someone can leave blank.

### Lesson 73: a guard against a shape the data cannot take is not a guard, it is a truncation (lane C, 2026-08-29)

**In short:** snippets' folder tree was walked with a `depth >= MAX_FOLDER_DEPTH
{ return; }` at the top of the recursion, and a comment saying why: two folders
could name each other as parent and the walk would never end. The comment
described a real shape. It did not describe a *reachable* one — and while the
guard was protecting against something that cannot happen, it was quietly
deleting something that can.

A folder has one parent, and the walk enters the tree from `None`. So a folder
is reached only if its parent chain ends at `None`, and every folder in a cycle
has a chain that never does: the cycle is a separate component the walk never
enters. The same argument bounds the recursion without any counter — a reached
folder's chain is finite and cannot repeat a folder, because a repeat *is* a
cycle and would not have been reached, so depth is at most `folders.len()`.

What the cap actually did was cut the tree off at eight. The New Folder button
files a new folder under whichever one is picked, so a user can build a ninth
level by clicking; at that depth the folder disappeared from the sidebar, with
the snippets filed in it, and nothing was said.

**The rule.** Before writing a guard against malformed data, work out whether
the code can *reach* the malformed shape. If it cannot, the guard has no
upside — and it always has a downside, because a limit that fires on nothing
bad still fires on something good. Write the reachability argument down in
place of the guard: it is the thing a future reader will otherwise re-derive
wrongly and re-add the cap.

**How it was found.** By mutation. Raising the cap to `usize::MAX` — "walk the
cycle for ever" — changed no test result, because the test named for it built a
two-folder cycle that the walk was never going to enter. Diagnosing why that
mutant survived is what surfaced both halves: the guard could not fire on a
cycle, and it could fire on real nesting. The replacement test nests twenty
deep through the program's own New Folder path and fails against the old code.

**Where else to look.** Every recursion over a parent-pointer structure with a
depth counter bolted on (`walk_folders` here; `delete_folder` beside it has the
same shape and correctly has no counter). More generally: any `MAX_*` constant
whose comment justifies it with data the program cannot construct. `grep` for
`MAX_` and read the comment — if it says "nothing here builds one today", that
is the tell.

### Lesson 74: a test that names the target delivers the event past the code that decides the target (lane C, 2026-08-29)

**In short:** snippets has four wheel tests, and the list's hit box could be
deleted outright with all four still passing. They called `a.scroll(Target::List,
dy)` — the app's *handler*, given the target directly. The step they were meant
to cover is the one in between: a wheel arrives at a point, `hit_test` decides
which panel that point is in, and only then is the handler called. A test that
supplies the target has already answered the question and is left checking that
the handler it called did what it does.

`Probe` was the reason: it had `click_at` and `key_at` but nothing for the
wheel, so there was no way to deliver a scroll at a coordinate and the tests
reached past it. That is a toolkit gap being paid for once per app — about forty
of the remaining programs have a wheel.

**The rule.** Deliver every event the way the window system delivers it: as a
position and a payload, never as a target. If the harness cannot express that,
fix the harness rather than the test — a per-app workaround is the same fault
forty more times.

**How it was found.** By mutating away `f.hit(Target::List, body)` and watching
nothing fail. Adding `Probe::scroll_at` (defaulting to `None`, so the programs
with no wheel need no impl) and `probe::scroll_at_point` fixed it, and forced a
second finding: the list's own box is drawn *before* its rows, and `hit_test`
takes the last hit recorded, so the box is only reachable in the strip below the
last row that `scroll_window::capacity`'s floor leaves over. A test that scrolls
at a row's centre exercises the row's route, not the panel's.

**Where else to look.** `probe::press` had the same shape of gap and now has
`probe::release` beside it: a suite built only from presses cannot tell a
program that ignores the key coming back up from one that runs every shortcut
twice per keystroke. Generally, look for a test calling a method the event loop
calls, rather than the entry point the event loop is given — `handle_key`
instead of `handle_event`, `press(target)` instead of a click at a point.

### Lesson 75: a witness that moves less than the tolerance has not moved (lane C, 2026-08-29)

**In short:** the test that a resize is remembered clicked the Stats button:
resize to 1600x900, find where Stats is in a 1600-wide frame, click its centre,
assert it opened. Throwing the resize away entirely did not fail it. The
toolbar's buttons are laid out from the *left* edge, so between a 1100-wide
window and a 1600-wide one Stats moves by a few pixels of padding — less than
its own width. The click at the wide position still landed inside the narrow
box, so both frames answered the same, and the test could not tell a program
that resized from one that ignored the event.

**The rule.** When a test proves something moved by clicking where it moved to,
the movement has to exceed the size of the thing that moves. Assert that
separately and up front — `wide.x >= narrow.right()` — rather than trusting the
click to notice. A witness whose displacement is inside its own tolerance is not
a witness; it is a coincidence that happens to be green.

**How it was found.** By mutation: the row that discards the resize was expected
to fail two tests and failed only one. The fix was to pick a control that
travels — the search box is measured from the right edge and moves by nearly the
whole difference — and to assert the two rectangles are disjoint before the
click is asked anything.

**Where else to look.** Any test whose subject is a *change in position*:
scrolling, resizing, reflow, drag. Related to lesson 68 (a containment
assertion has slack) but not the same fault — there the tolerance is written
into the assertion, here it is the size of the control being clicked, which
nobody wrote down at all.

### Lesson 76: a hit box for the whole sheet is not "anywhere on the sheet" — its own contents beat it (lane C, 2026-08-30)

**In short:** asteroids' game-over sheet dims the whole playfield and records
`Target::Overlay` over it, then draws a box in the middle holding the title, the
final score, the high score, the wave reached, and the "Press N" line — each
with a hit box of its own. `handle_mouse` had an arm for `Target::Overlay` and
an arm for `Target::NewGame`, which *reads* as "a click anywhere on the sheet
starts a new game". It was not. `Frame::hit_test` searches the recorded hits
**backwards** — last one wins, which is what makes a thing drawn on top
clickable — so the box's own lines, recorded after the overlay's, took every
click aimed at the middle. A click on "GAME OVER", or on the score the player
had just earned, did nothing; a click on the dim margin *around* the box started
a new game. The dead zone was precisely the part of the sheet a person looks at,
and the only part that worked was the part nobody aims for.

**The rule.** An arm that names a container's target has covered the container's
*margin*, not the container. Either name every child target in the same arm, or
record no child hit boxes at all. Whichever you pick, say which in a comment —
the two-arm version is the one that looks right, so the next reader will
otherwise simplify it back.

**How it was nearly missed.** The pause sheet has the same structure and did
*not* have the bug — because its middle line happens to be `Resume`, which had
an arm of its own doing the same thing. One sheet was correct by accident and
the other was wrong, from identical code. A suite that exercised only the pause
sheet would have concluded the pattern was sound.

**How it was found.** By a test that clicked `Target::Overlay` through the
probe. `probe::click` aims at the *centre* of the named rect, which for a
full-field overlay is exactly where the box is — so the click never reached the
target it asked for. That is lesson 74's family (the event is delivered past the
code that picks the target) turned inside out: there the test supplied the
answer and hid a routing bug, here the test aimed honestly and the *program*
routed it elsewhere, which is the failure the test existed to find. But note it
was findable only by luck of geometry — the box happens to sit at the centre.
The follow-up test names `OverlayTitle` and each `FinalStat` directly, so a
sheet whose box is off-centre is still asked the question.

**Where else to look.** Every overlay, dialog, sheet, tooltip, popup and context
menu in `apps/**` and `gui/**` that records a hit box for its backdrop *and* for
its contents. The tell is a match arm naming a backdrop target with no sibling
arm naming what is drawn on it.

**Swept 2026-08-30, and it comes back clean — for the right reason.** The other
three wired games with a real backdrop hit box (breakout `main.rs:1362`, pong
`main.rs:902`, tetris `main.rs:1727`) all draw their overlay's lines with
`centred`, which pushes text and records *no* hit box, so `Target::Overlay` is
the only hit in that region and a one-target arm really does mean "anywhere on
the sheet". That is the second of the two remedies above, arrived at by accident
rather than by decision — none of the three says so. Asteroids was the only app
that gave its overlay lines targets of their own, and giving them targets is
what turned the accident into a bug. So the risk here is not in the three that
are correct today; it is in the next person who adds one clickable line to one
of them. (The four unwired apps that match a grep for `Target::Overlay` —
fileassoc, startupmanager, taskscheduler, vpnmanager — are false positives:
`Overlay0`/`Overlay1` there are Catppuccin *colour* names, not hit targets.)

### Lesson 77: an expectation the code under test computes is not an expectation (lane C, 2026-08-30)

**In short:** asteroids' `every_asteroid_is_drawn_where_the_field_puts_it` took
each asteroid's hit box, asked `Field::to_screen` where that asteroid ought to
be, and asserted the two agreed to within a hundredth of a pixel. Both sides of
that comparison come from `to_screen`. Deleting the part of `to_screen` that
moves a scaled point onto the field — so every asteroid is drawn up in the
window's top-left corner, off the playfield entirely — left the test green: the
drawing pass and the expectation were wrong *in the same way*, and matched
perfectly. The picture would have been visibly broken and the suite silent.

**The rule.** At least one side of an assertion must be arrived at by a route
the code under test does not take: a constant, a hand-worked number, a property
(*inside* the field, *bigger* than before, *ordered* this way), or a second
implementation written from the specification rather than from the code. A test
whose expected value is a call into the subject asserts self-consistency, which
every deterministic function has for free.

**The tell.** The expected value in the assertion is produced by the same
function, or the same struct's method, that produced the actual value. In this
codebase that reads as `let (cx, cy) = field.to_screen(...)` sitting three lines
above `let (gx, gy) = rect.centre()` — and it looks *rigorous*, because it is
exact to two decimal places. Exactness is the disguise: a self-referential
assertion is always exact.

**The cheap fix is usually a property, not a second implementation.** Here it
was one line — every position in the world is inside the world, so every
asteroid must land inside `field.rect`. That is a fact about `to_screen` that
`to_screen` is never asked for, and the mutation cannot satisfy it. Reach for
the independent-implementation version (as tictactoe's negamax solver does) only
when no property is sharp enough.

**Where else to look.** Any `apps/**` test that computes a screen position, a
scale, a colour or a size by calling the drawing code's own helper and compares
it against what was drawn. Grep for a test body that calls `to_screen`,
`scaled`, `Layout::new` or a `*_rect()` helper and then asserts against a hit
box: if the helper is what the mutation would break, the test cannot see it.

### Lesson 78: a probe helper that sets the state itself tests everything except the code that sets it (lane C, 2026-08-30)

**In short:** `Probe::click_at` in asteroids calls `self.resize(w, h)` before
delivering the click, because a click has to be read against *some* size and the
probe has to supply it. That made
`a_click_lands_where_the_window_it_was_resized_to_put_the_control` — the test
named for resizing — blind to a `handle_event` that threw `Event::Resize` away
entirely, since the helper had already applied the resize by another door. The
app would have ignored every real resize the compositor sent, and the test that
exists to catch exactly that stayed green.

**The rule.** When the property under test *is* "the app noticed X", the test
must deliver X the way the window delivers it — as the event — and must not use
a probe helper that also performs X. Probe helpers are for setting up the state
a test is not about; the moment that state is what the test is about, drop to
`handle_event`/`on_event` and send the real thing.

**The tell.** A test whose name contains the name of an event (`resized`,
`focused`, `closed`, `scrolled`) but whose body never constructs that event.
`keys_reach_the_app_through_the_window` had the same shape from the other side:
it went through `handle_event` rather than `App::on_event`, so it asserted an
`EventResult` and never saw the `Response` — a build that answered `Idle` to
every change, and so never repainted, passed a test whose name says "through the
window".

**Where else to look.** Every `impl Probe` in `apps/**` whose `click_at` or
`key_at` calls `resize` (most do — it is the documented way to pass a size), and
every test named for an event. Two questions: does the test build the event, and
does it assert the type the window is actually handed?

### Lesson 79: a stack that ends empty is not a stack that was used correctly (lane C, 2026-08-30)

**In short:** `Frame::is_balanced` — the check every windowed app in this
campaign runs at every state and every size — was `self.clips.is_empty() &&
self.translations.is_empty()`. That catches a clip pushed and never popped. It
does not catch the mirror image: a pop with nothing to pop. `Vec::pop` on an
empty stack returns `None` and does nothing, so a helper that popped a clip it
had not pushed silently released *its caller's* clip, and the stack still ended
the frame empty. Everything drawn after the stray pop escaped the clip it was
supposed to be held inside, and `is_balanced` said the frame was fine.

**The rule.** When a test asserts that a resource stack is balanced, count the
operations that *failed to find anything to act on*, not just the depth at the
end. Depth at the end is a sum, and a sum cannot distinguish "never pushed"
from "pushed and over-popped" — the two errors cancel. The push-side error and
the pop-side error are different bugs with different symptoms and need
different counters.

**The tell.** A `pop()`/`remove()`/`decrement()` whose return value is
discarded inside a type that also offers an "is it balanced?" or "is it clean?"
predicate. If the pop can be a no-op and the predicate only reads the depth,
the predicate has a hole exactly the size of the no-op. `Frame::PopClip` was
`self.clips.pop();` — a discarded `Option` one line away from a doc comment
promising to catch "a bug that is invisible in the window it happens in".

**How it was found.** A mutation sweep on `apps/pacman`, deleting an
`f.clip(...)` and leaving its `f.unclip()` behind. Two mutation rows came back
`WRONG TESTS` because the balance test — the one whose whole job is that shape
— did not fail. Fixed by counting stray pops (`gui/toolkit/src/frame.rs`), with
a test for the plain over-pop and one for the damage: an over-popped clip stops
trimming the caller's hit boxes, so a control that should have been clipped
away stays clickable.

**Where else to look.** Any `is_balanced`-style predicate in the tree that is
written as an emptiness check over a stack whose pop is infallible. Also the
generalisation: an invariant asserted as a *final* value rather than as a
property of the whole sequence is blind to any pair of errors that cancel —
which for a stack is the commonest pair there is.

### Lesson 80: a box the frame has already trimmed cannot be found outside the frame (lane C, 2026-08-30)

**In short:** nearly every app in this campaign carries a test spelled
`nothing_is_drawn_outside_the_window`, and nearly every one of them walks
`Frame::hits()` and asserts each recorded box lies inside the window. That
assertion cannot fail. `Frame::hit` intersects the box it is given with the
active clip before recording it, and the apps clip to the window for the whole
frame — so a hit box outside the window is not something the frame declines to
produce, it is something the frame is *incapable* of producing. The test passed
on `apps/reversi` with the panel's whole band deliberately shifted twenty
pixels down and off the bottom of a 60x60 window.

**The rule.** A containment test is only a test if the thing it measures is
free to leave. When the type under test enforces the containment on the way in,
measure something the type does not touch — for a `Frame`, the *commands*,
which are recorded verbatim — or measure against a boundary the type does not
enforce. Reversi's replacement does the second: it asserts the panel's own
background band is painted exactly where the layout put the panel, which the
clip has no opinion about.

**The tell.** A test whose subject is "X is inside Y" where Y is also the
argument to a constructor, a clip, a `min`/`clamp`, or an `intersect` on the
path X takes to get recorded. Two questions settle it: *what code would have to
be wrong for this to fail*, and *is that code between the fault and the
assertion, or before it?* If the clamp sits between them, the assertion is
downstream of its own subject.

**The second half of the same finding.** Once the assertion is real, the
obvious rewrite — check every painted `FillRect`/`StrokeRect` against the
window — is *too* strong rather than too weak, and it failed immediately on
reversi at 60x40. The panel's rows are placed by a cursor walking down the
panel, and a panel too short for its rows runs the score bar off the bottom;
the app draws it anyway and the whole-window clip crops it, which is the
design. So the window is the wrong boundary in both directions: too generous to
catch a band in the wrong place, too strict to allow deliberate cropping. The
boundary that works is the layout's own band.

**Where else to look — surveyed and settled, 2026-08-30.** Twenty-one apps
carry an `outside_the_window`-shaped test, but only three of them assert on
`Frame::hits()`, and only those three could carry the tautology:

| App | Whole-frame clip? | Verdict |
|---|---|---|
| `apps/battleship` | yes, `f.clip(l.window)` | tautology — rewritten |
| `apps/freecell` | yes, `f.clip(l.window)` | tautology — rewritten |
| `apps/pdfviewer` | no — clips only individual bands | genuine; left alone |

The other eighteen assert on commands or on the layout and were never affected.
`apps/pdfviewer` never pushes a window-sized clip — `frame.clip` is called only
on `bar`, `strip`, `band`, the sidebar rect and `layout.content` — so a control
placed outside every band really is recorded outside the window and really does
fail the assertion.

Both are now `the_whole_frame_is_clipped_to_the_window`, keeping only the parts
that can fail: the outermost `PushClip` is the window, and the clip stack
balances. Deleting the vacuous loop then raises the real question — *was the
coverage it pretended to give present anywhere else?* — and the two apps answer
differently, which is why it is worth asking per-app rather than assuming:

- **`apps/battleship`: yes, already covered.** `the_cells_are_drawn_where_the_grid_says_they_are`
  compares all two hundred cells against a rectangle worked out by hand. A new
  mutation drawing the player's grid one cell right of where `Grids` laid it out
  is caught by four existing tests. So nothing was added; a test that duplicates
  an existing one is noise, and the clip test's comment now names the test that
  really holds the ground.
- **`apps/freecell`: no, genuinely uncovered.** Nothing compared a recorded box
  against `Table`'s geometry — the table tests exercise `Table` alone, without a
  frame. A new `at_a_size_that_fits_the_clip_crops_nothing` measures every free
  cell, foundation and column bottom against `top_slot`/`card_at`, and the
  matching mutation (free cells drawn one slot right) is caught by **that test
  and nothing else**.

The boundary that works in both cases is the layout's own rect, which the clip
has no opinion about: a band that drifts off the edge comes back smaller than
the layout said and loses the comparison.

Worth noting what made the old test not merely useless but *misleading*: both
apps carried the comment *"a hit box is recorded whether or not it would survive
the clip, so the boxes above cannot see a missing clip at all — only the
commands can."* Every clause of that is the reverse of the truth. `Frame::hit`'s
own doc says the rect "is moved by the translation in force and trimmed to the
clip in force, and is **dropped entirely** if nothing of it is visible." So the
boxes are the only thing that *can* see the clip vanish, and are blind to
everything else. A wrong comment beside a vacuous assertion is how a tautology
survives three readings.

**`guitk`'s own tests for `Frame::hit` — checked 2026-08-30, nothing to do.**
The worry was that the toolkit never pinned down the behaviour the app tests
were unknowingly leaning on, in which case it could drift and take fifty apps'
tests with it. It pins it down in five places: `a_target_is_trimmed_to_the_clip_in_force`
(a straddling box comes back cut, and the cut half is not clickable),
`a_target_entirely_outside_the_clip_is_dropped`,
`a_nested_clip_can_only_shrink_the_visible_region`,
`a_degenerate_clip_clips_everything_away`, and
`an_over_popped_clip_stops_trimming_the_callers_hits`, which is the same rule
read from the other side. So the lesson is about where an app puts its
assertion, not about a gap in the toolkit.

Still open: any test asserting a widget's rect is inside its parent where the
parent rect was used to compute the child's.

**It happened again in `apps/solitaire`, 2026-08-30 — the shape is more common
than the two apps above suggested.** `every_pile_is_drawn_inside_the_window`
walked every recorded box and asserted it lay within `0..w, 0..h`. The app
clips to the window, so that is exactly the tautology described above, written
independently by a different pass over a different app. A mutation fitting the
card size to the tableau alone — which runs the top row clean off the side of a
narrow window — passed it. What replaced it is a second usable spelling worth
remembering alongside "compare against the layout's own rect": **compare the
boxes against each other.** Every card in a card game is one size, so a card
that comes back narrower than its neighbours is a card the clip cut, and the
assertion is `w == max(w)` over the card targets rather than `x >= 0`. Any app
that draws a grid of identically-sized things — a board, a keypad, a palette,
a tile map — can use it, and unlike the "inside the window" form it cannot be
satisfied by the trimming itself.

### Lesson 81: a recorded hit box is not evidence that anything was drawn in it (lane C, 2026-08-30)

**In short:** the natural way to check that a panel drew all its rows is to ask
the frame whether each row's target has a box —
`probe::rect_of_sized(&app, Target::Captures, size).is_some()`. That checks the
row was *laid out*. It does not check the row was *painted*. In `apps/checkers`
the two are separable: `panel_row` computes the row's rectangle, draws the text
into it, and returns the rectangle, and the caller hands that rectangle to
`f.hit`. Delete the drawing and keep the return — one line — and every box is
still recorded, every `is_some()` still answers yes, and the panel is blank.
A mutation doing exactly that survived the test whose stated job was to catch
it.

**The rule.** A hit box and a painted pixel are two different outputs of the
drawing pass, and a test that reads one says nothing about the other. If what
you mean is "the user can see this", assert against the render commands. If
what you mean is "the user can click this", assert against the hit map. Say
which one you meant, and if you meant both, assert both — the box exists *and*
some `RenderCommand::Text`/`FillRect` origin falls inside it.

**The tell.** A drawing helper that returns the rect it was given rather than
the rect it drew, with a call site of the shape
`f.hit(Target::X, helper(f, ..))`. The rect flows to the hit map through a path
that does not pass through the painting. Also: any test whose only assertion is
`rect_of(..).is_some()` for a target whose *content* is the point of it. The
inverse shape — a control painted but never recorded — is caught by clicking
it, so it is the visible half that goes unguarded.

**How it was found.** A mutation sweep on `apps/checkers` gutting `panel_row`'s
body to `let _ = (f, s, ink); row`. The row was expected to be caught by
`the_panel_draws_its_rows_while_they_fit` and was instead caught only by
`the_captures_line_credits_the_side_that_did_the_taking`, which happens to read
the text. Fixed by requiring each of the panel's five boxes to contain the
origin of a line of text.

**Where else to look.** Every app in this campaign has a
"the panel/toolbar/sidebar drew its rows" test, and the cheap spelling of it is
`is_some()` over a list of targets. Wherever the drawing helper's return value
is independent of whether it drew — which is most of them, since returning the
laid-out rect is what makes `union` and the stacking cursor work — the test is
measuring the layout and reporting on the paint.

### Lesson 82: a `min` whose losing side never loses is an untested half of the code (lane C, 2026-08-30)

**In short:** layout code is full of "fit it to A, fit it to B, take the
smaller" — `size = by_width.min(by_height)`. Only one of the two ever *wins* in
any given state, and the tests are written against the states the app is
normally in, so the other side of the `min` can be deleted outright without a
single test noticing. In `apps/solitaire`, `Table::fit` sizes a card to the top
row and to the deepest fan a column can reach, and takes the smaller. The
deepest fan it reserves for is six face-down cards under twelve face-up. The
*opening deal* fans column 6 seven cards deep. Every geometry test in the suite
read an opening deal, so the fan half of the fit was slack in all of them, and
two mutations that removed it survived or were caught only by tests that had no
business firing.

**The rule.** A `min`/`max`/`clamp` in layout code is a branch, and a branch has
two sides. If the app's ordinary state always takes the same side, the other
side is unexecuted code wearing the same coverage number as the line it shares.
Write a test that *forces* the losing side — construct the worst case directly
rather than hoping the natural one contains it. In solitaire that meant
clearing a column and pushing eighteen cards into it by hand, which is a state
the game can reach and the opening deal never is.

**The tell.** Any `fit`/`solve` function that reserves room for a maximum
(`MAX_DEPTH`, `worst_case_rows`, "enough for the longest label") when the
fixtures are all typical. Also: a suite whose every drawing test starts from the
same `App::new()`, which for a game means the opening position — the position
most carefully chosen to be unremarkable.

**How it was found.** A mutation sweep on `apps/solitaire` replacing
`by_top.min(by_tableau)` with each half alone. `the card is fitted across
alone` and `the card is fitted to the top row alone` were expected to be caught
by the deep-column test and were not, because that test read the opening deal.
Fixed by `the_deepest_fan_a_deal_can_reach_still_fits_the_window`, which builds
the eighteen-card column and asserts the first and last card are the same size.

**Where else to look.** Every app in this campaign has a `Layout::solve` with at
least one `min` of two candidate sizes, and the reserve-for-the-worst-case side
is the one that only bites at a window size or a game state the fixtures do not
visit: sudoku's pencil-mark grid, chess's move history, checkers' capture
panel, and every app that reserves room for a scrollback it never fills in a
test.

**Addendum (lane C, 2026-08-30, `apps/yahtzee`).** The same shape appears one
step worse when the losing side is a *magic constant* and a later operation can
override it. `apps/yahtzee`'s scorecard was
`(w * 0.44).clamp(170.0, 380.0).min(w / 2.0)` — which reads as three
constraints and was two. At every window shape the app was tried at, either the
share or the half-window cap decided the width; the 170 floor never applied
once, so a mutation deleting it changed nothing anywhere. Two remedies, and
both are needed: (a) **derive the bound from the thing it exists to protect**
rather than writing a number — the floor is now the longest category name
measured at the font this window actually uses, widened back through the row's
own name/score split; and (b) **add a fixture at the shape where the bound is
the binding one**, which for yahtzee is 350x1000 and is nowhere near any of the
five shapes chosen to be "small, large, tall, wide, default". A bound nothing
binds at is not tested by *any* number of window sizes chosen for variety; it
is tested by one window size chosen by solving for it.

### Lesson 83: the window's own background makes "something is drawn here" true everywhere (lane C, 2026-08-30)

**In short:** Lesson 81 said a recorded hit box is not evidence anything was
drawn in it, and prescribed the obvious remedy — look for a paint command that
covers the box. That remedy is vacuous in every app in this campaign, because
the first thing every drawing pass does is fill the entire window with the
background colour. "Some `FillRect` covers this point" is therefore true of
*every point in the app*, including the middle of a widget that drew nothing at
all. `apps/yahtzee`'s `something_is_painted_inside_every_die_box` asked exactly
that question, and a mutation that recorded each die's hit box and then returned
before painting a single pixel of it walked straight past — past the one test
whose entire stated purpose was to notice a box with nothing in it.

**The rule.** Containment has to run the other way. Do not ask whether a paint
command *contains* the widget's centre; ask whether some paint command *fits
inside* the widget's box (grown by a pixel or two of slack). A full-window
background can never satisfy that, and a widget that drew itself always does,
because a widget that draws itself draws inside itself.

```rust
// vacuous — the background satisfies it for every point in the window
filled.iter().any(|r| r.contains(cx, cy))

// has teeth — nothing that isn't inside the die can satisfy it
let grown = Rect::new(die.x - 1.0, die.y - 1.0, die.w + 2.0, die.h + 2.0);
filled.iter().any(|r| {
    r.w > 0.0 && r.h > 0.0
        && r.x >= grown.x && r.y >= grown.y
        && r.right() <= grown.right() && r.bottom() <= grown.bottom()
})
```

**The tell.** Any assertion of the form "a command covers this point", in any
app whose `draw` opens with a background fill — which is all of them. The same
trap catches text: "some string was painted inside this rect" is nearly
vacuous for a rect that spans a column, because the neighbouring row's label
often has its origin on the boundary.

**Where else to look.** Every app in the campaign that has a
`something_is_painted_*` or `*_is_actually_drawn` test written after Lesson 81:
checkers, solitaire, mandelbrot, and the seven `is_some()`-only visibility
assertions Lesson 81 already lists as suspect. Lesson 81's "where else to look"
should be read as pointing at this remedy, not the point-containment one.

### Lesson 84: a test that computes its expectation with the function under test agrees with any bug in it (lane C, 2026-08-30)

**In short:** `apps/yahtzee`'s `every_category_box_carries_that_category_s_name`
checked that row *i* of the scorecard has category *i*'s name painted in it. It
got the expected name by calling `Category::at(i)` — which is the same function
the drawing pass calls to decide what to paint. A mutation making `at` return
the category *after* the one asked for relabelled every row in the game, and the
test passed, because the test's expectation shifted by one in lockstep with the
picture. The suite had two copies of the same wrong answer and compared them to
each other.

**The rule.** The expectation must come from somewhere the code under test
cannot reach. `Category::ALL[i]` is the data; `Category::at(i)` is the lookup
being tested. Index the data. More generally: when a `Probe` test needs to know
what *should* be on screen, spell it out from the rules of the game or from the
raw table, never from the accessor, formatter or index-mapper the drawing pass
uses. If spelling it out is inconvenient, that inconvenience is the test doing
its job.

**The tell.** Any `assert` whose left and right sides both contain a call into
the production module — especially a helper with "index", "at", "for", "of" or
"row" in its name, which is what a second description of an order looks like.
Also any test that reads `l.something` out of a `Layout` and then compares it to
a rect the same `Layout` produced: that is the same fault wearing geometry.

**Where else to look.** Every app in this campaign routes its tests through
`Probe`, and the natural way to write "the right thing is here" is to ask
production what the right thing is. Grep each suite's test module for calls to
production accessors inside `assert!`/`assert_eq!` arguments. The safe shape is
the one `clicking_a_category_lands_on_that_category_and_no_other` already
uses — click box *i*, then assert the set of filled boxes is exactly `{i}`,
which names no production helper at all.

### Lesson 85: "big enough for its contents" is a one-sided claim, and a constant can satisfy it forever (lane C, 2026-08-30)

**In short:** a test that says a box is *at least* as big as what goes in it can
never fail on a box that is *too big*. A hard-coded size that happens to clear
the bar at every window the app is tried at therefore passes such a test at
every size — not because it is right, but because the test only looks in one
direction. Both halves of yahtzee's second mutation sweep were this, and one of
them was a fault I had already "fixed" once.

**The two cases, both found by re-sweeping after a fix.**

1. **The scorecard's width floor.** Fault (11) in the roadmap entry: the floor
   was the magic constant `170` and never applied, because at every window shape
   tried either `w * 0.44` or the `w / 2` cap decided the width. The fix
   replaced it with a floor measured from the longest category name at the
   font *this* window uses, and added a sixth test window (350x1000) chosen as
   the one shape where the floor binds. **The re-sweep showed that was not
   enough.** At 17pt the measured floor comes out at 171 — so the old 170 was
   within one pixel of right *at that font*, and a card one pixel narrow still
   shows every name comfortably, because the measurement deliberately includes
   `pad * 1.2` of breathing room. The mutation survived a fixture built
   specifically for it.

   What the constant actually cannot do is **shrink**. Between 170 and 380 the
   share decided the width at every font, so a card whose rows are 8.8pt was the
   same 361 pixels as one whose rows are 17pt. The claim that catches it is
   `the_scorecard_is_sized_to_the_font_it_is_drawn_at`: two windows of one
   width and different heights, and the card must be narrower at the smaller
   font. Same shape as `the_button_grows_with_its_text`.

2. **The roll button's width.** `the_button_is_wide_enough_for_its_widest_legend`
   is a correct and useful claim — and it can never fail on the constant 140,
   because the font is capped at 17pt and the longest legend measures about 121
   there. The mutation that pins the width to 140 is caught only by
   `the_button_grows_with_its_text`, so that is the test it is now named for.
   Naming it for the legibility check was a lie the first sweep happened not to
   expose.

**The tell.** A test whose assertion is `actual >= needed` (or `<=`, or "fits
inside", or "does not overlap") where `needed` is computed from the content. It
constrains one side of the value and leaves the whole of the other side free. If
the quantity is *derived*, that is usually fine; if a mutation can replace it
with a constant and still pass, the claim was never about the derivation.

**Two remedies, and they are different.**

- **Pair every "big enough" with a "sized to".** Draw the same widget at two
  sizes that differ only in the input the size is supposed to depend on, and
  assert it *moves*. This is the only claim that distinguishes a measurement
  from a constant that happens to clear the bar.
- **Do not accept a fixture that makes a branch bind by a hair.** A fixture is
  only a fixture if the branch binds by more than the slack the code
  deliberately leaves — here, `pad * 1.2` of padding meant a floor that bound by
  1 pixel was invisible. *Solve* for a shape where the difference exceeds the
  slack, or find a different claim; do not pick a size, see the branch taken,
  and call it covered.

**And the process lesson underneath both: re-run the sweep after remediating
it.** Every one of these was a fix that looked right, was committed, and was
wrong — and the only thing that said so was running the same 68 mutations
again against the fixed tree. A mutation sweep is not a report you act on once;
it is the check that your action worked.

**Where else to look.** Grep every app's suite for `>=` / `<=` assertions
against a measured requirement — `is_wide_enough`, `fits`, `stays_inside`,
`no_..._is_painted_into_a_box_too_narrow` are all this shape — and ask of each:
*would a constant pass this at every size in `SIZES`?* Where the answer is yes,
add the two-size "sized to" partner. `apps/yahtzee` now has two of these
(`the_button_grows_with_its_text`,
`the_scorecard_is_sized_to_the_font_it_is_drawn_at`); every other app in the
campaign has fitted widgets and, so far, none.
