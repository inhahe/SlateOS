### TD-OILS-STARTS-COMMAND-DIVERGES-FROM-COMMAND-TOKEN-POSITION. `time h[1 2]=v` and `case … ;; h[1 2]=v` read the wrong `[` — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/lexer.rs:2620`, `starts_command`. This is the
lexer's own coarse "does a command begin after `prev`?" test, and its one
consumer is `assignment_acceptable` (lexer.rs:3202) — which decides whether a
`[` at the head of a word opens a *subscript* (unquoted spaces and operators
become content, and an unclosed one is a reader error) or is ordinary text. It
diverges from bash's `command_token_position` in two directions at once.

```c
#define command_token_position(token) \
  (((token) == ASSIGNMENT_WORD) || \
   ((parser_state&PST_REDIRLIST) && parsing_redirection(token) == 0) || \
   ((token) != SEMI_SEMI && (token) != SEMI_AND && (token) != SEMI_SEMI_AND && reserved_word_acceptable(token)))
                                                                       /* parse.y:2983-2986 */
```

**Too narrow: `time`.** `CMD_INTRODUCERS` (lexer.rs:2298) says it is "bash's
`reserved_word_acceptable` list, less `time`" — but bash's list does have `time`
in it, together with the two options that belong to it:

```c
    case BANG:		/* ! time pipeline */
    case TIME:		/* time time pipeline */
    case TIMEOPT:	/* time -p time pipeline */
    case TIMEIGN:	/* time -p -- ... */
      return 1;
```

(`time_command_acceptable`, parse.y:3140-3153; `reserved_word_acceptable` in
parser.h carries the same three). `-p` becomes TIMEOPT only directly after TIME,
and `--` becomes TIMEIGN after TIME or TIMEOPT and after nothing else
(`special_case_tokens`, parse.y:3292-3302):

```c
  /* Handle -p after `time'. */
  if (last_read_token == TIME && tokstr[0] == '-' && tokstr[1] == 'p' && !tokstr[2])
    return (TIMEOPT);
  /* Handle -- after `time'. */
  if (last_read_token == TIME && tokstr[0] == '-' && tokstr[1] == '-' && !tokstr[2])
    return (TIMEIGN);
  /* Handle -- after `time -p'. */
  if (last_read_token == TIMEOPT && tokstr[0] == '-' && tokstr[1] == '-' && !tokstr[2])
    return (TIMEIGN);
```

**Too wide: `;;`, `;&`, `;;&`.** `command_token_position` excludes those three by
name, because what follows one is the *next `case` arm's pattern*. osh's `Prev`
enum already documents this exactly (`Prev::CaseArmEnd`, lexer.rs:2311-2314) —
`starts_command` just does not implement it, and accepts `Op::DSemi`,
`Op::SemiAmp` and `Op::DSemiAmp`.

**Reproduce.**

```sh
eval 'time f1[1';                    echo "1 rc=$?"
eval 'time -p f2[1';                 echo "2 rc=$?"
eval 'time -- f3[1';                 echo "3 rc=$?"
eval 'time -p -- f4[1';              echo "4 rc=$?"
eval 'time time f5[1';               echo "5 rc=$?"
eval 'time -x f6[1';                 echo "6 rc=$?"
eval 'time h1[1 2]=v';               echo "7 rc=$?"
eval 'time -p h2[1 2]=v';            echo "8 rc=$?"
eval 'case x in y) :;; f7[1';        echo "9 rc=$?"
eval 'case x in y) :;; h3[1 2]=v';   echo "10 rc=$?"
eval 'case x in y) :;& f8[1';        echo "11 rc=$?"
eval 'case x in y) :;;& f9[1';       echo "12 rc=$?"
eval 'true | time f10[1';            echo "13 rc=$?"
eval 'true |& time f11[1';           echo "14 rc=$?"
```

| row | bash 5.2.37 | osh |
|---|---|---|
| 1–5 | `` …unexpected EOF while looking for matching `]' ``, `rc=2` | `f…[1: command not found`, `rc=127` (and the timing report prints) |
| 6 | `-x: command not found`, `rc=127` — `-x` is not one of `time`'s options, so it is the command word and `f6[1` is past the first word | same |
| 7, 8 | `1 2: syntax error in expression (error token is "2")` — the subscript swallowed the blank, exactly as a bare `h[1 2]=v` does | `h…[1: command not found` |
| 9, 11, 12 | `` syntax error near unexpected token `newline' `` — the `[` is ordinary text, so `f…[1` is a bare `case` pattern with no `)` | `` …unexpected EOF while looking for matching `]' `` |
| 10 | `` syntax error near unexpected token `2]=v' `` — two words, so the blank ended the first | `` syntax error near unexpected token `newline' `` |
| 13, 14 | `time: command not found` — after a pipe `time` is *not* the reserved word, so `f10[1` is past the first word | same |

Rows 7, 8 and 10 are the half that has nothing to do with the reader error: the
space-slurp is wrong in both positions, and has been all along.

**The fix.** Two changes to `starts_command`, one in each direction.

Drop `Op::DSemi`, `Op::SemiAmp` and `Op::DSemiAmp` from its operator list, and
say in the comment that `command_token_position` names them.

For `time`, the single previous token is not enough — `time -p` needs two. Give
the function the token slice (its only caller, lexer.rs:3802, already has `out`
and passes `out.last()`) and pair it with a mutually-recursive classifier that
returns which of TIME/TIMEOPT/TIMEIGN bash would have produced for the last
token, `None` for anything else; `starts_command` is then "an introducer, or a
`time` token". The recursion terminates because each step shortens the slice.

`time`-as-TIME is gated on `time_command_acceptable`, which is *narrower* than
`reserved_word_acceptable`. Measured, the only reachable difference is the pipe:
`true | time f[1` and `true |& time f[1` both make `time` an ordinary command
word (rows 13, 14) — as does a newline whose `token_before_that` was `|`. The
positions it also lists as rejected (`;;`, `fi`, `done`, `esac`, `}`) cannot be
followed by a word without a syntax error anyway, so only `|`/`|&` need
modelling. Note `starts_command` currently accepts `Op::Pipe`/`Op::PipeAmp`,
which is right for an *assignment* (`true | h[1 2]=v` does slurp) and wrong only
for `time` — so the pipe test belongs in the `time` classifier, not in
`starts_command`.

Found while fixing TD-OILS-AN-UNCLOSED-SUBSCRIPT-IN-AN-ASSIGNMENT-POSITION-IS-NOT-A-READER-ERROR.

**Fixed**, and a third divergence turned up while measuring: `)` was missing
too. `reserved_word_acceptable` has it, commented `/* only valid in case
statement */` — a `case` arm's pattern close, after which the arm's first
command begins. A subshell's close is the same token, and though no word may
follow one, the *reader* reaches the `[` before the grammar reaches the word, so
`(:) f[1` is `` unexpected EOF … `]' `` and not a syntax error. `case x in x)
h[1 2]=v;; esac` was the visible cost: the blank ended the word, so nothing was
assigned.

`starts_command` now takes the token run rather than the previous token alone,
drops the three case-arm terminators, adds `Op::RParen`, and defers to a new
`time_tok` for the `time` family. `time_tok` returns which of TIME/TIMEOPT/
TIMEIGN bash would have lexed, `None` otherwise, and the two call each other —
each step shortens the slice, so it terminates. The pipe test lives in
`time_tok`, not in `starts_command`, because `true | h[1 2]=v` *does* slurp its
subscript: a pipe is a command position for an assignment and only `time` is
fussy about it.

Two rows measured during this are *not* fixed and are logged separately:
`echo $(f[1` (TD-OILS-A-COMMAND-SUBSTITUTION-NAMES-ITS-OWN-PAREN-BEFORE-THE-BODYS-BRACKET)
and `true |⏎⏎time x` (TD-OILS-A-RUN-OF-NEWLINES-AFTER-A-PIPE-MAKES-TIME-RESERVED-AGAIN).

Corpus: `where-a-command-begins-decides-whether-a-bracket-is-a-subscript.sh`
(23 rows).

**Superseded.** `starts_command`, `time_tok` and `pipe_refuses_time` are all
gone: the look-back-at-the-token-tail model they shared was replaced by a
carried `CmdPos` (a fold over the tokens as they are pushed), and then `CmdPos`
absorbed `RwAccept` as well. See
TD-OILS-COMMAND-POSITION-WAS-GUESSED-FROM-THE-TOKEN-TAIL-IN-TWO-PLACES-AT-ONCE.
