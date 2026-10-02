### TD-OILS-AN-UNCLOSED-SUBSCRIPT-IN-AN-ASSIGNMENT-POSITION-IS-NOT-A-READER-ERROR. `f[1=R` is a command, not `unexpected EOF` — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/lexer.rs`, the word reader. A `[` that follows a
name at the head of a word — where an assignment could stand — is one of bash's
*reader* constructs: `read_token_word` calls
`parse_matched_pair (0, '[', ']', &dollar_present, P_ARRAYSUB)`, so an unclosed
one is a syntax error found while reading, not a word that happens to hold a
`[`. osh reads it as ordinary text and runs it as a command.

**Reproduce.** (Behind `eval` so the search for the `]` swallows only the
string.)

```sh
i=1
eval 'f2[b[$i]=R';   echo "1 rc=$?"
eval 'f3[1=R';       echo "2 rc=$?"
eval 'f6[1 x';       echo "3 rc=$?"
eval 'echo x; f7[1'; echo "4 rc=$?"
eval 'echo a[1';     echo "5 rc=$?"
eval '1[1=R';        echo "6 rc=$?"
```

| row | bash 5.2.37 | osh |
|---|---|---|
| 1–4 | `eval: line N: unexpected EOF while looking for matching `]'`, `rc=2` | `f…: command not found`, `rc=127` |
| 5 | `a[1`, `rc=0` — past the first word there is no assignment to test for | same |
| 6 | `1[1=R: command not found` — `1` is not a legal variable starter | same |

Row 4 shows the reader really does swallow the rest of the input: `echo x` runs
first, and then the error. Row 3 shows it swallows past a blank, so the `[` run
is not word-terminated by one.

osh's diagnostic also carries a stray newline (`f2[b[1]=R⏎: command not
found`), which suggests the lexer already scans for the `]`, runs off the end,
and then keeps the text instead of failing — so the fix may be as small as
turning that give-up into the reader error.

A second round of probing (2026-08-10) pinned the guard and widened the row
set. bash's condition is parse.y:5145-5149:

```c
      else if MBTEST(character == '[' &&		/* ] */
		     ((token_index > 0 && assignment_acceptable (last_read_token) && token_is_ident (token, token_index)) ||
		      (token_index == 0 && (parser_state&PST_COMPASSIGN))))
        {
	  ttok = parse_matched_pair (cd, '[', ']', &ttoklen, P_ARRAYSUB);
	  if (ttok == &matched_pair_error)
	    return -1;		/* Bail immediately. */
```

with `assignment_acceptable(t)` = `command_token_position(t) && (parser_state &
PST_CASEPAT) == 0` (parse.y:2989) and `token_is_ident` = `legal_identifier` on
the token so far (parse.y:4846). Further measured rows, all `eval`-wrapped:

| source | bash 5.2.37 | osh |
|---|---|---|
| `if f[1; then :; fi` | `` …matching `]' ``, `rc=2` | `syntax error: unexpected end of file`, `rc=2` |
| `{ f[1; }` | `` …matching `]' ``, `rc=2` | same wrong shape |
| `x=1 f[1` | `` …matching `]' ``, `rc=2` | `f[1⏎: command not found` |
| `f[']'` | `` …matching `]' ``, `rc=2` | `f[]⏎: command not found` — a quoted `]` does not close it |
| `f["]` | `` …matching `"' ``, `rc=2` | same (already right) |
| `f[$(echo 1` | `` …matching `)' ``, `rc=2` | same (already right) |
| `case x in f[1) :;; esac` | `rc=0`, no error — `PST_CASEPAT` | same |
| `f[1⏎echo hi]=R` | subscript spans the newline; arithmetic error on `1⏎echo hi` | same |
| `a=([1` | `` …matching `]' ``, `rc=1` | `` …matching `)' ``, `rc=1` — osh scans the literal for its `)` before lexing elements, so it names the wrong bracket |

**The fix.** Read the `[` as a matched pair when the word so far is a name and
the word is in assignment position (bash's `assignment_acceptable`), and raise
`unexpected EOF while looking for matching `]'` when it does not close.
`Lexer::read_word_inner` (lexer.rs:4646) already slurps the subscript with a
`sub_depth` counter that treats newlines, `;`, `#` and unquoted spaces as
content, so almost all of the reading is right: what is missing is the check at
the end of the loop (lexer.rs:4853) that `sub_depth > 0` is
`Err(eof_matching(']'))` rather than a word. The `a=([1` row is a separate,
smaller ordering difference and may be left as-is.
Corpus row: `a-nested-bracket-in-an-assignment-target-is-still-an-assignment.sh`
row 20 is the *unaffected* half, and points here.

Found while writing that case.

**Fixed** as described, and it was that small: `read_word_inner` now records the
line the outermost `[` stood on when `sub_depth` goes 0→1, and where the loop
used to fall out and flush the literal it raises `eof_matching(']').at(sub_line)`
if `sub_depth` is still non-zero. The opening line is what bash reports —
`parser_error (start_lineno, …)` with `start_lineno` taken on entry to
`parse_matched_pair` (parse.y:3701, 3711) — so a subscript that runs off the
end over several lines names the line it began on rather than the last one read.

The `a=([1` row came right along with it: osh names `]` now, because the element
reader reaches the same check before the enclosing literal ever runs out of
input. Nothing had to be done about the ordering after all.

One row of the probe set is *not* fixed by this and does not belong to it:
`f[1⏎echo hi]=R` closes its subscript and then fails the arithmetic, whose
`DISCARD` osh did not carry out of the `eval` — that was
TD-OILS-A-DISCARD-STOPS-AT-THE-EVAL-OR-FUNCTION-IT-WAS-RAISED-IN, since fixed.

`time f[1` is also still wrong, for an unrelated reason: `starts_command`
(lexer.rs:2620) omits `time` from its reserved-word list, so the `[` is not read
in assignment position after it. That is
TD-OILS-TIME-DOES-NOT-EXTEND-COMMAND-POSITION-FOR-THE-LEXER, below.

Corpus: `an-unclosed-subscript-in-an-assignment-position-is-a-reader-error.sh`
(33 rows).
