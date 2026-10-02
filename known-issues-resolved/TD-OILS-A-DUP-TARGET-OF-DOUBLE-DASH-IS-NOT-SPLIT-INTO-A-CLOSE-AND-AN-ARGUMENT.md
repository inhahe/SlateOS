### TD-OILS-A-DUP-TARGET-OF-DOUBLE-DASH-IS-NOT-SPLIT-INTO-A-CLOSE-AND-AN-ARGUMENT. `>&--` is taken as one word — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/lexer.rs` — whatever collects the word after a
`<&`/`>&`; bash's lexer takes the `-` as the whole redirection target the moment
it sees it, and leaves anything after it to be an ordinary word.

**What.** bash reads `>&-` as a close and the remaining `-` as an *argument*:

```text
$ bash -c 'f(){ true 1>&--; }; declare -f f'   →   true - 1>&-
$ bash -c 'exec 3>&1; echo z 1>&--'            →   echo: write error: Bad file descriptor
$ osh  -c 'f(){ true 1>&--; }; declare -f f'   →   true >&--
$ osh  -c 'exec 3>&1; echo z 1>&--'            →   (silent, rc=0)
```

osh keeps `--` as one target word, so it never closes fd 1 and never grows the
extra argument. Pre-existing — `ast::dup_move_source` deliberately declines a
source that begins with `-`, so the move work neither caused nor changed it.

**Proper fix.** Stop the dup-target scan at a leading `-`: the target is exactly
`-`, and the rest of the token is a separate word.

**Impact.** Very low; `>&--` is a typo, not an idiom. Worth fixing only because
it is a one-line lexer rule and the current behaviour is silently wrong rather
than an error.

**Fixed** in the lexer, where bash puts it. `read_token` (`parse.y`) returns the
`-` as a token of its own *before* `read_token_word` is ever entered —

```c
/* Hack <&- (close stdin) case.  Also <&N- (dup and close). */
if MBTEST(character == '-' && (last_read_token == LESS_AND ||
                               last_read_token == GREATER_AND))
  return (character);
```

— and the grammar has literal `'-'` productions (`GREATER_AND '-'`, `NUMBER
LESS_AND '-'`, `REDIR_WORD GREATER_AND '-'`, …) to receive it. osh now has the
same arm, guarded on `out.last()` being `Op::LessAnd`/`Op::GreatAnd`. Blanks
before the dash make no difference in either shell, since both skip them first.
Only a *leading* dash: `1>&2-x` never reaches the arm, because the `2` starts an
ordinary word that swallows the rest.

**And it was hiding a second bug**, found by sweeping every neighbouring
spelling once the first was fixed — see the entry below.
