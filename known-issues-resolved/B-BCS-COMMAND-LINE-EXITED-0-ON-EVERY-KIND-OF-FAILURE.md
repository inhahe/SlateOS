## B-BCS-COMMAND-LINE-EXITED-0-ON-EVERY-KIND-OF-FAILURE (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** `bc` is the calculator. Every way of getting its command line
wrong — naming a file that does not exist, typing an option it does not have,
typing `-e` with no expression after it — was reported with a message and then
**ignored**: `bc` carried on and exited **0**, the code that means "success".
A shell script doing `bc calculations.bc || echo failed` never printed
`failed`, and `$(bc missing.bc)` produced an empty answer that looked like a
computed zero. On top of that the parser called `env::args()`, which *panics*
on an argument that is not valid text, so `bc café.bc` on a system whose file
names are not UTF-8 aborted before it could name the file at all.

Found by the size-gap audit (see
`B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`), and surfaced by the
`diagnostics_quote_names` lint: moving `bc.rs` into
`userspace/coreutils/src/bin/` brought it under a test it had never faced,
which flagged the hand-written quotes in one diagnostic. Reading the function
to fix that one line found six more defects in the same twenty lines.

**Every sentence and status below was measured against GNU bc 1.07.1 through
WSL, and recall was wrong three times.** The instrument for the option table
is `bc --=x`: an empty prefix matches every long option, so `getopt_long`
prints the whole table in declaration order.

### The seven defects

| # | What `main` did | What GNU does | Cost |
|---|---|---|---|
| 1 | a file that will not open printed a message and the loop **continued**, exiting `0` | `File NAME is unavailable.`, **stop**, exit `1` | `bc a.bc missing.bc c.bc` ran `c.bc` with the definitions `missing.bc` was to have provided — a wrong answer instead of an error, and a `0` status saying it was right |
| 2 | `env::args()` | — | panics on a non-UTF-8 argument, so the file could not even be named |
| 3 | `fs::read_to_string` | — | an invalid byte *in the file's contents* was reported as `cannot open 'x'`, naming the wrong failure: the file had been opened and read in full |
| 4 | `stdin.lock().lines()` with `Err(_) => break` | — | one stray byte in a piped script **silently truncated the program** and still exited `0` |
| 5 | an unknown option printed `bc: unknown option: -Z` and **carried on**, exiting `0` | `bc: invalid option -- 'Z'` on stderr, usage on stdout, exit `1` | a mistyped flag ran the calculation anyway, with the flag not applied |
| 6 | `-e` with nothing after it wrote `None` into an unchecked `Option` | `bc -e` is not even an option upstream; ours needs an argument | `bc -e` became an **interactive session**, so a script that built its expression from an empty variable hung waiting on stdin |
| 7 | the option loop iterated `flag.chars()` | bytes | `-é` reported `invalid option -- 'é'`, an option nobody typed |

Defect 1 is the one that matters most, and defects 1, 4 and 6 share a shape:
**a failure was detected, described, and then not acted on.** That is worse
than not detecting it, because the `0` status actively certifies the wrong
answer to everything downstream.

### Three measurements that contradicted recall

1. **`-e` and `-f` do not exist in GNU bc 1.07.1.** `bc -e 2+2` answers
   `bc: invalid option -- 'e'` and exits 1; `bc --expression=2+2` answers
   `unrecognized option '--expression=2+2'`. Our `-e` is a SlateOS extension
   (it is Gavin Howard's bc that has one), and the usage text now says so, so
   nobody ports a script to a GNU host expecting it.
2. **The long-option table is alphabetical and has eight entries**, one of
   which — `--compile`, which emits dc code — the usage text does not mention:
   `'--compile' '--help' '--interactive' '--mathlib' '--quiet' '--standard'
   '--version' '--warn'`. The order is observable output, because
   `getopt_long` lists an ambiguous prefix's candidates in it.
3. **A file operand does not end the run.** `printf '9+9\n' | bc a.bc` prints
   a.bc's output *and then* evaluates standard input, in one interpreter — so
   a file may define functions a later interactive session uses. Ours returned
   after the files. Also measured: a bare `-` is **not** standard input, it is
   a file name that fails to open (`File - is unavailable.`, exit 1) — the
   comment in our operand arm claimed the opposite.

### The fix

`main` is now three functions plus a pure `parse_args` that can be tested:

- `env::args_os()` and byte-wise option matching throughout, so no argument
  can panic and `-é` reports the byte it is.
- `-e`/`--expression` and file operands go into **one ordered list**, so
  `bc -e 'define f(x){…}' use.bc` and `bc use.bc -e '…'` are different
  programs, as the order typed says they are.
- Standard input is read after the operands, as GNU does — unless `-e` was
  given, since `bc -e '2+2'` dropping into an interactive session is nobody's
  behaviour.
- Every failure returns `ExitCode::FAILURE`, and a file that will not open
  stops the run at that file.
- Diagnostics come from `coreutils::getopt`, so they are glibc's sentences
  rather than sentences invented here, and file names go through
  `coreutils::quote` so a name cannot forge a line of output.
- 20 new tests, each citing the GNU command that was run to establish what it
  asserts. `cargo test -p coreutils --bin bc`: 77 passing, up from 57.

### Two limitations this leaves, on purpose

**`-s`/`--standard` and `-c`/`--compile` are now refused rather than
ignored.** We implement neither. Ignoring `-s` is *silently wrong*: it makes
non-standard constructs errors, so a bc that accepted and ignored it would run
a program POSIX bc rejects and print an answer. Measured: `echo 'print 1,2' |
bc -s` prints `(standard_in) 1: Error: print statement` and computes nothing,
where plain `bc` prints `12`. `-c` is worse still — it emits dc code instead
of results. Refusing is loud and cannot mislead; the proper fix is to tag
every extension in the parser with whether POSIX has it, which is a real piece
of work in `Parser` and not a flag.

`-w`/`--warn` is the counter-example, and is accepted as a **no-op**:
ignoring it omits an advisory on stderr and leaves every computed value
identical, so refusing it would break working scripts to no purpose. The line
between the two is *whether the flag's absence changes an answer*.

**A source file that is not UTF-8 is refused, where GNU lexes it byte by
byte.** Our `Lexer` takes `&str`. The old code silently truncated at the first
bad byte and exited 0; the new code says `bc: 'x.bc': not valid UTF-8` and
exits 1, which is honest but still stricter than GNU. The proper fix is to
make the lexer byte-oriented, at which point a stray byte becomes a lex error
on one line rather than a whole-file refusal.

### Why the tests did not catch any of this

`bc` had 57 unit tests and a 200-case differential harness against GNU bc, and
**neither touches the command line.** The unit tests call `Parser`/
`Interpreter` directly; `scripts/calc-diff.sh` feeds every program on *stdin*
with no arguments at all. So the whole of `main` — argument parsing, file
reading, exit status — was covered by nothing, in a binary otherwise tested
better than most in the tree. This is the same shape as
`B-FOUR-COMPONENTS-DISAGREED-ON-THE-NAME-OF-THE-SYSTEM`'s cause: a test suite
can be thorough and still leave a hole, if every test enters through the same
door.

The 20 new tests enter through `parse_args`, which is why it was extracted as
a pure function returning a `Request` rather than left as statements inside
`main`.
