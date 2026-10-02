### TD-OILS-COMMAND-POSITION-WAS-GUESSED-FROM-THE-TOKEN-TAIL-IN-TWO-PLACES-AT-ONCE. `coproc f[1`, `(( 0 )) f[1`, `[[ a == a ]] f[1`, `time -p (( 1 ))` — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/lexer.rs`. The lexer answered bash's one question
— what `last_read_token` is and what it permits — with **two** independent
approximations that had grown up separately:

* `starts_command`/`time_tok`/`pipe_refuses_time`, a backwards walk over the
  token tail, answering `command_token_position` for the subscript slurp; and
* `RwAccept`, a forward fold over a hard-coded `CMD_INTRODUCERS` list, answering
  `reserved_word_acceptable` for `arith_cmd_position`.

Neither carried the other's entries, so each was wrong exactly where the other
was right. Five divergences fell out of that split, all measured:

**Reproduce.**

```sh
eval 'coproc f1[1';                  echo "1 rc=$?"
eval '(( 0 )) f2[1';                 echo "2 rc=$?"
eval '[[ a == a ]] f3[1';            echo "3 rc=$?"
eval 'time -p (( 1 ))';              echo "4 rc=$?"
eval 'true | time (( 1 ))';          echo "5 rc=$?"
```

| row | bash 5.2.37 | osh (before) |
|---|---|---|
| 1–3 | `` …unexpected EOF while looking for matching `]' ``, `rc=2` | 1: `rc=0`; 2, 3: `` syntax error near unexpected token `f…[1' `` |
| 4 | the arithmetic command runs, `rc=0` | `1: command not found`, `rc=127` |
| 5 | `` syntax error near unexpected token `(' `` | `` syntax error near unexpected token ` 1 ' `` |

Rows 1–3 are `COPROC`, `ARITH_CMD` and `COND_END` — all three are in
`reserved_word_acceptable` (parse.y:5367-5415) and none was in the subscript
model's list. Rows 4–5 are `TIMEOPT`/`TIMEIGN` and the pipe: both are in
`time_command_acceptable` but were known only to the subscript model, so the
arithmetic one could not see them.

**The fix** was to stop having two models. `RwAccept` is deleted and `CmdPos` —
the fold introduced by the previous commit — now carries everything: `ok`
(`reserved_word_acceptable` for the token about to be read), `redir_list` +
`first` (`PST_REDIRLIST`, set only by a redirection that is a simple command's
*first* element, per make_cmd.c:526-535), `after_pipe` (`time`'s extra
restriction) and `cond` (the `[[ … ]]` freeze). The three questions are three
readers of that one state — `reserved_ok`, `arith_ok` (which adds
`parse_dparen`'s `for` branch) and `at_command` (bash's three
`command_token_position` clauses, written out literally).

Corpus: `the-rest-of-reserved-word-acceptable-is-command-position-too.sh`
(22 rows).

`CaseScan::cmd_pos` was a **third** approximation, and is now folded in as well
— see
TD-OILS-A-CASE-SPELLED-INSIDE-AN-ARITHMETIC-SPAN-OPENED-A-PATTERN-LIST-THAT-NEVER-CLOSED.
