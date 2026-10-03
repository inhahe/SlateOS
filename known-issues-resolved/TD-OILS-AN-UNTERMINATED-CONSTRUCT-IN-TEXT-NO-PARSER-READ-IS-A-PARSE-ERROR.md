### TD-OILS-AN-UNTERMINATED-CONSTRUCT-IN-TEXT-NO-PARSER-READ-IS-A-PARSE-ERROR. `$(echo`, `${x:-a`, `` `echo `` and `$((1+` in a here-doc all kill the script — 2026-08-09 — ✅ RESOLVED 2026-08-09

**Where:** `userspace/oils/src/lexer.rs` — every scan that ends in
`eof_matching(…)` returns a `LexError`, which becomes a script syntax error even
when `here_text` is set. The AST has nowhere to record "this construct was never
closed", so the failure cannot be deferred to expansion time.

**Reproduce** — five rows, all with the body on line 2 of the script and
`echo after=$?` on line 4. bash prints the diagnostic, sets `$?` to 1 and runs
the rest of the script; osh dies with a parse error and nothing after runs.

| body | bash |
|---|---|
| `$(echo` | `S: command substitution: line 3: unexpected EOF while looking for matching `)'` |
| `${x:-a` | ``S: line 1: bad substitution: no closing `}' in ${x:-a`` |
| `` `echo `` | ``S: line 1: bad substitution: no closing "`" in `echo`` |
| `$((1+` | ``S: line 1: bad substitution: no closing `)' in $((1+`` |
| `$[1+` | ``S: line 1: bad substitution: no closing `]' in $[1+`` |
| `"abc` | *(no error — an unterminated quote in unread text is just text; osh already agrees)* |

**Why.** Nothing in the text was read by a parser, so an unclosed construct in it
cannot be a parse error — the scanner that meets it is the *expansion-time* one
in `subst.c`, and it reports at run time. Two different reporters, by construct:

* **`$( … )`** goes through `xparse_dolparen`. When `base[*indp] != ')'` it calls
  `parser_error (start_lineno, "unexpected EOF while looking for matching `%c'",
  ')')` and `jump_to_top_level (DISCARD)` (parse.y:4360-4365) — recoverable, so
  the enclosing command dies with `$?` at 1. The message carries the
  `command substitution` input name because it is raised from inside
  `parse_string (…, "command substitution", …)`, and its line is
  `current_line + newlines(rest-of-text-after-the-`$(`) + 1`: `parse_string` does
  `push_stream (0)` and so numbers the string's line *k* as `line_number + k`.
  Measured: `cat <<E` on line 1 with body `$(echo⏎` reports line 3; on line 2
  with body `x⏎$(echo⏎y⏎` reports line 5.
* **`${ … }`, `$(( … ))`, `$[ … ]` and `` ` … ` ``** are found by
  `extract_dollar_brace_string` (subst.c:1785, 1980), `extract_delimited_string`
  (subst.c:1498) and `param_expand`'s backquote arm (subst.c:11290). All four say
  `bad substitution: no closing `%s' in %s`, at plain `current_line`, then
  `exp_jump_to_top_level (DISCARD)`. The `%s` is the **whole text being
  expanded**, not the construct — `cat <<E⏎q${x:-a⏎E` echoes `q${x:-a⏎`, and a
  three-line body echoes all three lines — *except* the backquote site, which
  prints only the text after the backtick.

**Under `no_longjmp_on_fatal_error`** (a prompt expansion — `PS4`, `${x@P}`) all
four take the `else` branch instead: they set `*sindex = i` and return NULL, and
what the caller then reports differs per construct. Measured on `v='a${x:-b'`:
`${v@P}` prints `S: line 3: a${x:-b: bad substitution` and yields the text
*unchanged*; the backquote form still prints its `no closing` message and yields
the text unchanged; and `v='a$((1+'` prints nothing at all and yields `a`.

**Proper fix.** Give the lexer, when `here_text` is set, a way to end a scan
"unterminated" rather than erroring: a `closed: bool` on `CmdSubBody::Unread`
(with the body running to end of text) for the `$(` case, and a new `WordPart`
carrying the closer and the text for the other four. Expansion then reports as
above and `arm_discard(1)`, with the `Shell::prompt_expanding` branch selecting
the `no_longjmp_on_fatal_error` behaviour. The unterminated-quote row needs
nothing: an unread `"` really is just a character.

**Resolved**, by the fix sketched above. A scan that runs off the end of unread
text now ends in `LexError::unclosed`, carrying an `UnreadEof` that says which
of bash's two reporters owns the failure; `Lexer::unclosed_seg` turns that back
into a segment — a `Seg::Unclosed` for the four `subst.c` scanners, a
`Seg::CmdSub(…, SubBody::Unread { closed: false })` for `$( … )`, whose body is
simply the rest of the text. `Shell::expand_unclosed` reports the first and
`comsub_unclosed_error` the second. Covered by
`tests/corpus/an-unterminated-construct-in-text-no-parser-read-is-a-runtime-failure.sh`
(32 rows, plus the `declare -f` print-back and the four `@P` prompt shapes).

**Three things the entry above had wrong or missing**, all found by measuring
rather than by reading:

- **Promotion is errexit-only.** The first draft armed `FatalWhen::ErrexitOrPosix`
  on the `subst.c` sites by analogy with the assignment-error paths. It is
  wrong: all five sites report through `report_error`, whose entire promotion
  rule is `if (exit_immediately_on_error) exit_shell (…)` (error.c:201), and
  none of them consults `posixly_correct`. `set -o posix` still reaches the
  `echo after=$?`; `set -e` does not.
- **`$( … )` is never scanned for at all.** `extract_command_subst` hands
  `xparse_dolparen` everything from the `$(` to the end of the text and lets a
  *real parse* decide where the body stops — so the failure is whatever that
  parse hits first, not necessarily the missing `)`: a `$(fi` names the token,
  an unclosed `'` inside it names the quote, and a nested `$( … )` is
  `parse_comsub`'s own `jump_to_top_level (FORCE_EOF)` and stops the shell
  outright (`after=127`). Only when the body parses cleanly to its end is the
  message the `unexpected EOF` one, whose line is the shell's line plus the
  1-based line of the failure *within the body*.
- **Which reporter fires turns on `SX_COMMAND`.** An enclosing scan that meets a
  nested `$(` hands it over and reports as a command substitution — but only if
  it carries `SX_COMMAND`, since that is what `extract_delimited_string` tests
  before parsing one (subst.c:1429). `${` and `$((` carry it; `$[` passes a bare
  `0` (subst.c:1303), so `$[1+$(echo` steps over the `$(` as text and keeps its
  own ``no closing `]'``.

**And one prediction that held.** The `no_longjmp_on_fatal_error` branch really
does report unconditionally and suppress only the jump, so a prompt expansion
carries on to *run* an unclosed `$( … )`'s body — and `xparse_dolparen`'s
`substring (ostring, 0, nc - 1)`, which is there to drop the `)`, costs a real
character when there was none. Hence `v='a$(echo'; ${v@P}` runs `ech`.
