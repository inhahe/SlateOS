### TD-OILS-CMDSUB-ERR-FATALITY. `osh` reported a command-substitution syntax error at parse time in both spellings; bash defers the backtick form and makes the `$( )` form fatal to the caller — 2026-07-28 — RESOLVED 2026-07-29

**Where:** `userspace/oils/src/parser.rs` `parse_cmdsub_body` / `seg_to_part`;
`userspace/oils/src/ast.rs` `CmdSubBody`; `userspace/oils/src/interp.rs`
(`command_sub_body` / `backtick_sub`, `parse_error_flow`,
`syntax_error_prefix`).

**What:** The *message* now matches bash for both spellings — a `$( … )` body
that runs out mid-construct is blamed on the substitution's closing `)`,
because bash parses that body in the enclosing token stream, while a backtick
body is blamed on its own end of input. Two behavioural differences remained,
both only observable when the substitution is inside `eval`; at script level
the whole unit fails to parse in either shell, so they agree there.

1. **bash parses a backtick body lazily.** — **RESOLVED 2026-07-29.**
   ``echo `if` `` in bash prints
   `<name>: command substitution: line 2: syntax error: unexpected end of file`
   at *expansion* time and then runs `echo` with an empty substitution, so the
   command succeeds. osh parsed the body up front, so the command never ran.
2. **A `$( )` body error is fatal to bash's caller.** — **RESOLVED
   2026-07-28.** `( eval 'echo $(if)' )` exits the subshell with status 1 in
   bash, whose comsub parser calls `jump_to_top_level` and unwinds past `eval`;
   osh returned 2 from `eval` and carried on.

**Reproduce (both, before the fixes):**
```sh
( eval 'echo `if`'; echo "inner:$?" ); echo "sub:$?"
# bash: `command substitution:` error, blank line, inner:0
# osh (old): `eval:` error, no blank line, inner:2
( eval 'echo $(if)'; echo "NOT REACHED" ); echo "sub:$?"
# bash: sub:1 (the subshell is unwound);  osh (old): "NOT REACHED", sub:0
```

**Fix (item 2).** `ParseError` gained a `fatal` flag, set by
`parse_cmdsub_body` (non-backtick) and `parse_procsub_body` on everything they
return — the *body* is the thing whose failure has no way back through the word
being read, which is exactly what bash's `jump_to_top_level(DISCARD)` after
`top_level_cleanup()` expresses. `Shell::parse_error_flow` turns the flag into
control flow at the one place every read-eval loop reports a parse error
(`Shell::run_source_flow_out`): `Flow::Exit`, which `eval` and `.`/`source`
already forward through `pending_builtin_exit`, so the unwind crosses them and
takes the shell — or the nearest subshell — with it.

Statuses, all measured against bash 5.2:

| context | direct | inside `eval`/`source` |
|---|---|---|
| `bash -c` | 127 | 127, shell exits |
| script / stdin | 2 | 1, shell exits |
| inside a subshell / cmdsub / pipeline stage | — | 1, that child exits |

The `direct` column is the outermost read-eval loop, where there is no caller
to unwind to — abandoning the rest of the input is the whole effect, and osh
already did that — so only the status differs, and only under `-c`. (bash's
`-c` really does use 127 rather than the 1 its own `run_one_command` would
suggest: an `EXIT` trap sees `$?` = 127 there, and 257 = `EX_BADSYNTAX` in a
script. osh reports 1 for the script case rather than 257 — `$?` above 255 is
not representable in its status model, and the truncated exit code agrees.)

Two adjacent bugs fell out of the same measurement and are fixed with it:

* `Shell::syntax_error_prefix` tagged a diagnostic from inside a **sourced
  file** with the outer shell's `-c` token — `./bad.sh: -c: line 2:` where bash
  says `./bad.sh: line 2:`. The token names the *input source* being reported,
  so it is now dropped once `source_stack` is non-empty. (An `eval` inside a
  sourced file still says `eval`: that is then the innermost source.)
* The `$( )` probes in `tests/corpus/cmdsub-paren-terminator.sh` had their
  statuses deliberately unprinted because of this divergence; the header now
  points at the new corpus file instead.

**Fix (item 1).** `WordPart::CommandSub` now carries a `CmdSubBody`
(`ast.rs`): `Parsed(Program)` for `$( … )`, which the enclosing parse still
recurses into, and `Backtick { src, verbatim, close_line }`, which it does not
read at all. `Shell::backtick_sub` reads it at expansion time by handing `src`
to `run_source_flow_out` — the ordinary read-eval loop — inside the
substitution's subshell, with `line_base = close_line - 1`. Consequences, each
measured against bash 5.2 and covered by the corpus file below:

* **Lazy.** A body that is never expanded is never parsed, so
  ``if false; then echo `for`; fi`` is silent.
* **No cache.** The loop re-reads the body on every expansion, so an error in a
  loop body is reported once per iteration. That is not merely bash fidelity:
  a `shopt -s extglob` run between two expansions genuinely changes how the
  body lexes, so a cached parse would be *wrong*.
* **Incremental.** The body is read one logical line at a time, so commands
  before a syntax error have already run and their output is kept
  (``x=`echo ok<newline>for` `` substitutes `ok`, with side effects applied),
  and an `alias`/`shopt` in the body changes how the rest of it parses.
* **Not fatal to the reader.** The enclosing command still runs, with an empty
  substitution. `Shell::parse_error_flow` gives the body's own read-eval loop
  its own rule — status 2, or 1 when the body's error was itself a `$( … )`
  body's — keyed on the new `Shell::backtick_body` flag, which is set only on
  the substitution's subshell and cleared by `clone_for_subshell`.
* **`command substitution` source token.** `syntax_error_prefix` emits it for
  that same loop, in place of the enclosing `-c`. A nested `eval`/`.` inside
  the body reports as itself, which falls out of gating on
  `at_outermost_read_eval()`.
* **Plain line offset.** A backtick body's `$LINENO` counts up from
  `close_line - 1`, *not* by the rank-based renumbering `parse_cmdsub_body`
  applies to a `$( … )` body — measured: a body of `echo $LINENO`, a blank
  line, `echo $LINENO` closing on line 4 reports 3 and 5, where the `$( … )`
  spelling of the same layout reports 3 and 4. `parse_backtick_body`, added
  and then removed the same day, is unnecessary: `IncrementalParser` already
  takes a `line_base`.

Two further divergences fell out of the same measurement and are fixed with it:

* `Shell::comsub_read_file` accepted the `$(< file)` fast path whenever *some*
  fd-0 read redirect was present. bash's optimisation is all-or-nothing: with
  a second input (`$(< a < b)`) or an unrelated `2>…` attached it runs the null
  command instead, which writes nothing, so the substitution is empty. Now
  requires exactly one redirect. The fast path applies to the backtick spelling
  too, and only when the redirect is the body's whole content — hence
  `IncrementalParser::exhausted()`, which lets `comsub_text_read_file` ask
  whether the unit it just parsed was the only one.
* Aliases/`shopt` set inside a substitution body did not affect the rest of it,
  because `command_sub` executes a pre-parsed `Program`. Fixed for the backtick
  spelling by the incremental loop above, and for `$( … )` on 2026-07-29 — see
  `TD-OILS-CMDSUB-DOLLAR-NOT-INCREMENTAL`.

**Coverage:** `tests/corpus/cmdsub-error-fatality.sh` (eval, nested eval,
function, sourced file, command substitution, pipeline stage, and the
non-fatal ordinary syntax error for contrast);
`parser::only_a_paren_body_error_is_fatal_to_its_reader`; the `fatal`
assertions in `parser::cmdsub_body_is_terminated_by_its_closing_paren`.
Item 1: `tests/corpus/backtick-body-deferred.sh` (laziness, re-parse per
expansion incl. the extglob case, incremental execution and side effects,
`eval`/`.` reporting as themselves, the fast-path rules, and the line
numbering); `interp::cmdsub_syntax_error_names_the_closing_paren`;
`interp::lineno_inside_backticks_is_a_plain_offset_from_the_closing_tick`.

**Not covered — see `TD-OILS-SYNTAX-ERR-ERREXIT-ECHO`:** under `set -e` bash
reports 2 for every row of the table above, so the corpus file avoids errexit.
