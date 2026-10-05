## A-KSHELL-BRACE-EXPANSION-DELETED-EVERY-QUOTE-IN-THE-SHELL — ✅ FIXED 2026-08-24 (lane A)

**In short:** the kernel shell threw away quotation marks before it read the
command line. `echo 'a > b'` did not print `a > b`; it printed `a` into a file
called `b`. `echo 'a && b'` ran two commands. `echo 'a   b'` printed one space.
And `trap 'grep zeta f' ERR` — the case that surfaced it — installed a trap on
a signal named `zeta`. One function was responsible for all of it.

**Where:** `kernel/src/kshell.rs::expand_braces`, called from `execute` on
every line the shell runs.

### What it was

`expand_braces` was written as:

```rust
let tokens = split_words(input);
for token in tokens { ...; result.push(' '); }
```

`split_words` is a *word splitter*, and word splitters remove quotes — that is
their job, and it is correct there. It is catastrophic here, because
`expand_braces` runs at `execute` line 5528, **before the line is parsed at
all**. So the rejoin handed every downstream stage a line with all quoting
deleted and all whitespace runs collapsed:

| written | what the parsers actually received |
|---|---|
| `trap 'grep zeta f' ERR` | `trap grep zeta f ERR` |
| `echo 'a   b'` | `echo a b` |
| `echo 'a && b'` | `echo a && b` — two chained commands |
| `echo 'a > b'` | `echo a > b` — an output redirect |

The function's own doc comment promised "Tokens without `{` or `}` pass through
unchanged." The rejoin broke that promise for every token in the shell.

### How it was found, and the tell that it was systemic

Boot test batch52 failed in kshell self-test §13 with `last_exit()` reading 0
where 1 was expected. The serial log showed `trap` printing *two* errors:

```
trap: unsupported signal 'zeta'
trap: unsupported signal '/tmp/kshell_trap_selftest.txt'
```

`cmd_trap`'s own quote parser was hand-traced against the failing string and
proved correct — it handles `trap 'cmd args' SIG` exactly right. It simply
never received a quote.

The tell that this was not a `trap` bug: `split_chain_operators` (5601),
`parse_redirect` (6035) and `parse_input_redirect` (6190) all carefully track
`in_sq`/`in_dq` and refuse to split inside a quoted region. **Every one of
those checks was dead code**, because no quote ever reached them. The shell was
written throughout for quotes to survive to the parsers, and this one function
was quietly guaranteeing they never did.

### The fix

Two stages, each doing one thing:

1. **`expand_braces` is now byte-preserving.** It scans tokens itself, tracking
   quote state, and re-emits separators, quotes, prefixes and suffixes exactly
   as written. Only a token containing an *unquoted* `{`…`}` is rewritten, so
   `echo '{a,b}'` is literal, as in bash. New helpers `unquoted_positions` and
   `split_unquoted` keep the brace/comma scans quote-aware; the expansion
   arithmetic itself moved unchanged into `expand_braces_token`.
2. **Quote removal became an explicit, named, per-command stage.**
   `remove_quotes` deletes quote characters and moves nothing else — it is not
   a splitter, which is how `'a   b'`'s interior spacing now survives — and it
   is applied in `dispatch` and `dispatch_with_input`, after the parsers have
   had their look at the line with the quoting still in it.

Commands listed in `command_parses_own_quotes` are handed the line as written
instead. `trap` is the founding member: its handler *is* a command line
followed by another argument, so where the handler ends is precisely the fact
quote removal destroys.

**Covered by** kshell self-test §15 ("quoting survives expansion"): the two
stages asserted in isolation, then end-to-end through `capture_command` for
quoted whitespace, a quoted `&&`, a quoted `>` (including that no file was
created by it), and the `trap` handler round-trip.

**Lesson:** a parser full of careful quote handling, in a shell where quotes
never reach a parser, is not evidence that quoting works. It is evidence that
someone upstream is eating them.
