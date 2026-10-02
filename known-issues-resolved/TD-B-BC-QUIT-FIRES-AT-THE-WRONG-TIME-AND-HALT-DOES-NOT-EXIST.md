## TD-B-BC-QUIT-FIRES-AT-THE-WRONG-TIME-AND-HALT-DOES-NOT-EXIST (lane B, 2026-08-24) — ✅ **FIXED 2026-08-24**

**In short:** `bc` is a calculator, and it has two ways to stop: `quit` and
`halt`. Ours implements one of them, and implements it at the wrong moment.
GNU `bc` stops as soon as it *reads* the word `quit`, before running anything
on the same line; ours stops when it *reaches* it, having already run what came
before. So a one-line script that GNU answers with nothing, ours answers with
half its output. And `halt` — the one that really is meant to stop when
reached — we do not recognise at all, so a script using it dies with a syntax
error.

**Where:** `userspace/coreutils/src/bin/bc.rs`. The keyword table at the `"quit"
=> Token::Quit` arm; `Stmt::Quit`; `Interpreter::exec_stmt`'s `Stmt::Quit` arm;
`Parser::parse_program`; and `eval_input`, which parses a whole file in one go
where GNU compiles and runs it a line at a time.

**Measured**, GNU `bc` 1.07.1 under WSL:

| Input | GNU prints | Ours prints |
|---|---|---|
| `print "A"` / `quit` / `print "B"` on three lines | `A` | `A` |
| `print "A"; quit; print "B"` on **one** line | *(nothing)* | `A` |
| `print "A"` / `if (0) { quit }` / `print "B"` | `A` | `AB` |
| `print "A"` / `if (0) { halt }` / `print "B"` | `AB` | syntax error |
| `print "A"` / `halt` / `print "B"` | `A` | syntax error |
| `define f() { quit }` then `print "before"` | *(nothing)* | `before` |

The rule the table encodes is the one the GNU manual states outright: *"quit:
when this statement is read, the bc processor is terminated, regardless of
where the quit statement is found"*, versus *"halt: … an executed statement"*.
The granularity of "read" is one input line — that is why the three-line case
agrees and the one-line case does not, and why a `quit` buried in an
`if (0)` or in a function body that is never called still ends the run.

**The proper fix**, in three parts:

1. Rename the existing statement to `Stmt::Halt` and add `"halt" =>
   Token::Halt`. The execute-time machinery already exists and is already
   correct for it — `StmtResult::Quit`/`LoopFlow::Quit`/`RuntimeError::Quit`
   and `Session`, added in `abce71f46`, are exactly `halt`'s plumbing and
   should be renamed with it.
2. Make `quit` a *parse*-time event: `Parser::parse_program` truncates the
   statement list at the `quit` token and reports that it saw one, and the
   caller runs the truncated list and then stops. Nothing after the `quit` in
   the same chunk runs, which is what the one-line and `define` rows above
   require.
3. Give `eval_input` the same line-at-a-time chunking `eval_stdin` already has,
   or the file rows will still differ: GNU compiles and runs a file line by
   line, so `print "A"` before a `quit` on the next line does print.

**What is safe about the current state:** the exit *status* is right —
`abce71f46` removed the `process::exit(0)` that bypassed the `close_stderr`
funnel, so `quit` after an unwritable `print` now correctly exits 1. What is
wrong is only *how much runs first*, and `halt` being missing.

**Needs** a `bc-diff.sh` differential harness against GNU `bc` under WSL, like
the ones for `cmp`/`tee`/`echo`/`tty`; there is none yet, which is why this
went unnoticed.

### How it was fixed

`scripts/bc-diff.sh` was written first, and every row in the table above is now
one of its cases. All 24 quit/halt comparisons agree with GNU.

`halt` went in as planned: `Token::Halt`, `Stmt::Halt`, and the four existing
`…::Quit` variants renamed to `…::Halt`, since that machinery was always
`halt`'s and never `quit`'s.

**`quit` did not go in as planned, and the plan's step 2 was wrong.** It said
to *truncate* the statement list at the `quit`. Measurement says the whole unit
is discarded — `print "A"; quit` prints nothing, not `A`. The model that fits
is GNU's scanner: it calls `exit(0)` the instant it *scans* the token, and the
statements the parser has compiled but not yet executed die unexecuted with it.
So `Parser::saw_quit` asks the token stream, before a single statement is built,
and a unit containing one is thrown away entire. The confirming case is
`if (1)\nquit\n2`, which GNU answers with nothing: the `if` had a body, would
have run, and did not, because the `quit` on line 2 is inside the same unit.

### The part that was not in the plan at all: what a "unit" is

Step 3 said to give `eval_input` "the same line-at-a-time chunking `eval_stdin`
already has". Doing exactly that would have been a regression, because the
chunking `eval_stdin` had was itself wrong. It split on brace depth, so
`if (0)` on one line and its body on the next were two units and **the body ran
unconditionally**. Measured: `printf 'if (0)\nprint "x"\n' | bc -q` prints
nothing on GNU.

Both routes now share one `Chunker`, whose test for "is there more of this to
come?" is `Parser::truncated` — set when a parse hits `Eof` where a token was
*required*. Three things were measured to draw its boundary, and only the first
was guessable:

| Input | GNU | Therefore |
|---|---|---|
| `if (0)` / `print "x"` | prints nothing | a missing **body** waits for the next line |
| `x = 1 +` / `2` / `x` | `syntax error`, `2`, `0` | a missing **operand** does **not** — the newline ends the statement |
| `/* one` / `two */ 2+2` | `4` | an unclosed **comment** waits |
| `1 + \` / `2` | `3` | a trailing **continuation** waits |

The last two are invisible to the parser: `/* one` yields no tokens and reads
exactly like a blank line, and `1 + \` yields `1` and `+` and reads exactly
like the missing-operand case the parser is right *not* to wait for. Only the
scanner can tell them apart, so `Lexer::unfinished` reports them and
`Parser::new` folds it into `truncated`.

### Cost

`open_brace_depth` is gone, and with it the last of the brace counting. Each
line is re-lexed together with everything pending rather than on its own, which
is required for correctness (a line that begins inside a comment means something
else entirely) and costs a re-scan of at most a few lines.

Commits: `4256da9b6` (host build), and the bc change below. Harness result went
from 50 differences on its first run to 23, none of them quit/halt; the
remainder are the separate entries filed immediately after this one.
