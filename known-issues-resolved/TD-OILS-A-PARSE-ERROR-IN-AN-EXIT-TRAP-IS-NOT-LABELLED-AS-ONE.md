### TD-OILS-A-PARSE-ERROR-IN-AN-EXIT-TRAP-IS-NOT-LABELLED-AS-ONE. `( trap "'" EXIT )` — 2026-08-07 — ✅ FIXED 2026-08-08 by 51b188c48

**Where:** `userspace/oils/src/interp.rs` — `Shell::syntax_error_prefix`, which
derived the label from an ad-hoc chain of `-c`/`eval`/backtick tests.

**What:** bash runs a trap handler through
`parse_and_execute (…, "exit trap", …)` (trap.c), and that third argument is the
name the error is reported under — so a malformed EXIT handler says
`bash: exit trap: line 1: …` wherever it runs. osh labelled it with whatever it
happened to be inside, or with nothing at all.

**Repro** (bash 5.2.37 left, osh before the fix right):

| input | bash | osh |
|---|---|---|
| `( trap "'" EXIT )` | ``exit trap: line 1: unexpected EOF while looking for matching `'`` | the same with no `exit trap: ` |
| `x=$( trap "'" EXIT )` | `… exit trap: line 1: …` | `… command substitution: line 1: …` |
| `eval '. ./b1.sh'` | `./b1.sh: line 2: …` | `./b1.sh: eval: line 2: …` |

**The fix — the label is not a label, it is a stack.** The chain was the wrong
*shape*, not merely incomplete. bash keeps one `bash_input.name` per open input
source, `yy_input_name()` (parse.y:1468) reads the top of it, and `parser_error`
(error.c:367) picks between four shapes from two names and two flags:

```c
ename = get_name_for_error ();   /* BASH_SOURCE[0], else $0 */
iname = yy_input_name ();        /* the innermost open source */
if (interactive)               "%s: "               ename
else if (interactive_shell)    "%s: %s: line %d: "  ename iname
else if (STREQ (ename, iname)) "%s: line %d: "      ename
else                           "%s: %s: line %d: "  ename iname
```

Two things fall out of transcribing that rather than approximating it. The
`STREQ` arm *is* the reason the token usually doesn't appear — a script file, a
sourced file and piped stdin each name their input after the very file the error
is already blamed on — so the special case suppressing `-c` "once a file is being
sourced" turns out to have been a rediscovery of it, and the third row above (a
`source` reached through an `eval`, which the chain answered with both names)
came for free. And a stack pops: an `eval` inside a sourced file inside an `eval`
reports each level as it is entered and left, which no flag can express.

Push sites: `-c` (shell.c:766), `$0` for a script or piped stdin (shell.c:1756),
the sourced path (`_evalfile`), `eval` (eval.def), `command substitution` and
`process substitution` (subst.c), and the trap tags — `exit trap` (trap.c:1010),
`debug trap`/`error trap`/`return trap`/`interrupt trap` (trap.c:1208-1275) and
a bare `trap` for a signal (trap.c:449).

**The second flag, and why each source carries a bit.** `interactive_shell` is
"this shell has a prompt at all"; `interactive` is "it is reading the terminal
right now". Opening an input clears the latter — except `eval`. Measured:

```
$ bash -i -c "eval 'fi'"     bash: syntax error near unexpected token `fi'
$ bash -i -c ". ./b1.sh"     bash: ./b1.sh: line 2: syntax error near …
```

`source_file` passes `FEVAL_NONINT` (evalfile.c:374, acted on at :230); `eval`
passes `SEVAL_NOHIST|SEVAL_NOOPTIMIZE` only (eval.def:56), and
`parse_and_execute` touches the flag for nothing else (evalstring.c:270). osh has
only the one flag, so `interactive` is reconstructed as "interactive shell, and
no `nonint` source open" — see `InputSource::nonint`.

**Found on the way out**, fixed separately in b2f3492c4: a *signal* handler is
also re-read with `SEVAL_RESETLINE` (trap.c:449) and so numbered from 1, where
osh numbered it from the line it interrupted. The three synchronous traps are the
exception — `_run_trap_internal` omits the flag (trap.c:1117) — which is fitting,
since DEBUG, ERR and RETURN each fire *about* a command whereas a signal merely
arrives between two.

Covered by
`tests/corpus/a-syntax-error-is-named-after-the-innermost-input-being-read.sh`.

**Left divergent, deliberately:** in an *interactive* shell `get_name_for_error`
(error.c:96) ignores `BASH_SOURCE[0]`/`$0` and reports `base_pathname
(shell_name)` — plain `bash`. osh keeps its `error_source()` in all four arms.
That is a pre-existing difference in `ename`, untouched here and unreachable from
the corpus harness, which runs both shells non-interactively.
