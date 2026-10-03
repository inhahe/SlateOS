### TD-OILS-AN-ARITHMETIC-SCAN-REPORTS-NONE-OF-THE-READS-IT-MAKES. `$(( … ))` swallows the diagnostics its nested `$( … )` should raise, and loses the text after a read that stopped early — 2026-08-14

**Where:** `userspace/oils/src/interp.rs` — `Shell::arith_extent_expand` /
`arith_extent_frame` and the `$((` route out of `Shell::arith_extent_route`.

**What is wrong.** `param_expand` reaches a `$((` through
`extract_command_subst` with `SX_COMMAND` (subst.c:10575), so the paren count
*does* recurse into a nested `$( … )` — a real parse, reported where it is met.
osh runs the count but never reports, and in one shape stops in the wrong place.
Measured against bash 5.2.37 (`build/pgX.sh` rows a/c, `build/pgY.sh` d4/d5):

| word (inside `v='…'`, via `"${v@P}"`) | bash | osh |
|---|---|---|
| `A$((1+$(echo hi⏎q` | reports EOF, `[A]` | reports EOF, `[Ahi]` |
| `A$((1+$(for⏎q))B` | reports **twice** (`for`, then `` `(1+$(for' ``), `[AB]` | silent, `[A]` |
| `A$((1+$(for⏎xB` | reports `for`, `[A]` | reports `for`, **runs `fo`**, `[A⏎xB]` |

Rows 1 and 3 report because the read runs from `Shell::arith_nested_read`,
which does call `Shell::comsub_reparse_read`; what those two get wrong is the
*value*, both by performing the abandoned extent the way the string level does
and the brace level does not. Row 2 is the substantive one: the read stopped
part way, so bash's count resumed after the `for`'s line and found the `))`,
leaving `B` to the word. osh consumes to the end and loses it — and so never
reaches the read at all, which is why it is the one row that is also silent.

**What the proper fix looks like.** The `$((` count needs the same two-outcome
treatment `${ … }` got on 2026-08-14: `Shell::comsub_reparse_read` for the
report (which also decides jump vs. no-jump), and
`Shell::failed_extent_split`'s resume point for where the count carries on.
`Lexer::unread_comsub_stop` already puts the lexer in the right place; what is
missing is the interp half — an `arith`-side counterpart of
`Shell::unclosed_brace_reads`.

**Impact.** Diagnostics only for two of the three rows; a wrong value for the
third. Needs `@P`/`PS4`/here-doc text to be reachable at all.
