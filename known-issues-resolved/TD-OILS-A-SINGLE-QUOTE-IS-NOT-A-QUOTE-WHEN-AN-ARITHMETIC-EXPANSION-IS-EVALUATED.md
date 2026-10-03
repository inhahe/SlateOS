### TD-OILS-A-SINGLE-QUOTE-IS-NOT-A-QUOTE-WHEN-AN-ARITHMETIC-EXPANSION-IS-EVALUATED. `echo $(( 1 + '$(fi)' ))` — 2026-08-07 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_arith_params` and the
new `Shell::arith_dolparen`; `userspace/oils/src/lexer.rs` —
`lexer::scan_cmdsub_body`.

**What (as filed).** The `$((` *scan* honours `'…'` (`shellquote` recurses for
it, parse.y:3844), so a `)` in one closes nothing. But the text that survives
the scan is expanded as a word, and by then the quotes are just characters
bash's arithmetic path does not re-honour — so the `$( … )` inside them runs.
osh was recorded as never running it and reporting `1 + '' : syntax error`
instead.

**What re-measurement found.** That half was already fixed by the time this
entry was picked up: `$(( 1 + '$(echo 2 >&2; echo 3)' ))`, `$(( '1' + 2 ))`,
`$(( 'x' ))` and `$(( '$x' ))` all matched bash byte-for-byte, because
`expand_arith_params` had stopped honouring `'…'`. **The recorded divergence was
stale.** What the re-measurement surfaced instead was the *rest* of the same
bash function, none of which osh modelled — so the entry is closed by fixing
that, not by fixing what it describes. (Standing rule: where a recorded
hypothesis and a fresh measurement disagree, the measurement wins.)

**The real divergence: osh never modelled `xparse_dolparen`.** bash reaches a
`$( … )` met while *expanding* an arithmetic string through
`extract_command_subst` (subst.c:1290), which — unless what follows is another
`(` — hands `string + *sindex`, **the whole rest of the string**, to
`xparse_dolparen` (parse.y:4248). That runs
`parse_string (string, "command substitution", …)`, so *the parser* decides both
where the substitution ends and whether it is well-formed, in the **parent**,
before `command_substitute` forks. osh instead matched parentheses
(`Shell::scan_paren_group`) and let the child discover any syntax error. Seven
consequences, all measured against bash 5.2.37 and all wrong in osh:

| | bash | osh (before) |
|---|---|---|
| `$(( '$(case x in x) echo 7;; esac)' + 0 ))` | body runs to the `)` after `esac` | cut at the `case` pattern's `)` |
| `$(( '$(echo a<newline>fi)' ))` | substitutes nothing — the parse failed before `echo a` ran | substituted `a`: the child reached the error a command at a time |
| the diagnostic | `command substitution: line N+1:` | `line N:`, and `eval:` inside an `eval` |
| the echoed source line | the whole remainder — `` `fi)' + 1 ' `` | the extracted body — `` `fi' `` |
| after the failure | word abandoned, no arithmetic error | an extra `'' + 1 : syntax error: operand expected` |
| `set -e` | shell exits **2**, one diagnostic line | continued, rc=1 |
| `set -o posix` | keeps running | **osh exited** |

**Fixed by** giving that text one reader instead of two:

1. `lexer::scan_cmdsub_body` exposes the lexer's own case-aware
   `read_subst_body` — the scan a `$( … )` written in source already gets — so
   the extent is the parser's answer for both. The cursor it returns is a
   *character* index, like the one `expand_arith_params` walks.
2. `Shell::arith_dolparen` parses that extent in the parent before running it,
   under a pushed `command substitution` input name and a `LineMap::Offset`
   of `current_line` rather than `current_line - 1`. The `+1` is not a fudge:
   `parse_string` opens the string with `push_stream (0)`
   (builtins/evalstring.c:627), which does *not* reset the line number while the
   first line fetched charges one anyway — its own comment says as much, and
   `xparse_dolparen` never subtracts. Only the parse is shifted; the body's
   *run* is the child's and lands on the enclosing line, so
   `run_command_sub_text` keeps its own base.
3. The diagnostic is formatted against `rest` (everything past the `$(`), which
   is what `parse_string` was reading — hence the echoed line running on past
   the `)`.
4. On failure the word expansion is given up: `arm_discard(1)` for bash's
   `jump_to_top_level (DISCARD)` (parse.y:4330), and under errexit a
   `FatalAbort { status: 2, demote: false }` for `parser_error`'s
   `if (exit_immediately_on_error) exit_shell (last_command_exit_value = 2)`
   (error.c). Posix mode deliberately does **not** promote it — this is a parse
   failure and never reaches the "an expansion error ends the shell" hook.

The `demote` flag is new: `Shell::unbound_error` now carries a `FatalAbort`
rather than a bare status, because `fatal_abort_status`'s demotion to 1 models
*longjmp interception* and `parser_error` does not longjmp — it calls
`exit_shell` outright, so its 2 stands in a script and inside a subshell alike.
Every pre-existing site arms `demote: true` and is unchanged.

**Covered by** the corpus case
`a-command-substitution-met-while-expanding-arithmetic-is-parsed-by-the-parent.sh`
(all seven groups above) and the unit test
`lexer::tests::a_deferred_command_substitution_ends_where_the_parser_says`.
