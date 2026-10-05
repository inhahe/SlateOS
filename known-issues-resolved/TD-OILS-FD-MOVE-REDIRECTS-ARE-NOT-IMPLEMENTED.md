### TD-OILS-FD-MOVE-REDIRECTS-ARE-NOT-IMPLEMENTED. `n>&m-` / `n<&m-` are rejected as an ambiguous redirect — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/ast.rs` — `dup_spelling` has no `Move` variant, so
a target of `m-` falls through to `DupSpelling::Word`; `userspace/oils/src/interp.rs`
— the `RedirectOp::DupIn` / `DupOut` arms of `Shell::resolve_one_redirect` and
`Shell::apply_persistent_redirect`, which then try to treat `m-` as a filename.

**What.** bash's *move* redirection duplicates fd `m` onto fd `n` and then closes
`m` — one operation, spelled with a trailing `-`. osh does not implement it at
all and reports the target as an ambiguous redirect.

```text
$ bash -c 'exec 3>&1; echo moved 1>&3-; echo "rc=$?"'
moved
rc=0
$ osh  -c 'exec 3>&1; echo moved 1>&3-; echo "rc=$?"'
osh: line 1: 3-: ambiguous redirect
rc=1
```

Found while measuring TD-OILS-NOUNSET-IN-A-REDIRECTION-WORD: bash's
null-command fork scan treats a bare-number move (`0<&2-`) as *not* forcing a
fork, unlike the word form (`0<&$v-`), which is the one case where that
classification is observable. `Shell::null_command_forces_fork` already
classifies both correctly, so it will keep working once moves exist — but a
corpus case cannot exercise the `0<&2-` row until then.

**Proper fix.** Add a `DupSpelling::Move` (bash's `r_move_input`/`r_move_output`,
and `_word` forms for a non-literal target), recognised by `dup_spelling` for a
literal `[0-9]+-`, and handle it in both redirect appliers as a dup followed by a
close of the source. `unparse` must print the trailing `-` back.

**Impact.** Any script using the move idiom fails outright. The idiom is
uncommon but not exotic — it is the tidy spelling of the `exec 3>&1 1>&2 2>&3
3>&-` descriptor shuffle.

**Fixed.** Implemented as proposed, with three things the entry above did not
anticipate, all measured against bash 5.2.37:

*The `-` is syntactic, and it is the source text that carries it.* bash sorts
the word at parse time, so the trailing `-` is never something an expansion can
supply or take away:

| written | is a move? | because |
|---|---|---|
| `>&3-` | yes | bare number + unquoted `-` |
| `>&$v-` | yes, whatever `$v` holds | the `-` is the source text's |
| `>&"3"-` | yes | only the `-` itself has to be unquoted |
| `>&3"-"` | **no** — the filename `3-` | the `-` is quoted |
| `>&$v` with `v=3-` | **no** — the filename `3-` | the `-` came from the expansion |

So `ast::dup_move_source` strips the `-` off the *word*, and `dup_spelling`
grew `MoveNumber`/`MoveWord` by classifying what is left.

*A self-move does not close.* `3>&3-` leaves fd 3 open, where the `3>&3 3>&-`
it otherwise means would close it — so the emitted close is skipped when the
source is the redirector.

*A move names a bad descriptor differently from the word form it looks like.*
The bare-number form names its source whatever the redirector is (`1>&9-` and
`3>&9-` both say `9`); every other form names the **redirector**, even where the
plain word form would have named the word — `1>&$v-` says `1` where `1>&$v`
says `$v`. `Shell::dup_error_subject` now strips the move first and skips the
word-or-redirector rule for what is left.

Corpus case: `a-move-redirection-duplicates-its-source-and-then-closes-it.sh`.
Two residual divergences found while writing it are logged separately as
TD-OILS-A-FAILED-MOVE-OMITS-BASHS-EXTRA-CANNOT-DUPLICATE-FD-LINE and
TD-OILS-A-DUP-TARGET-OF-DOUBLE-DASH-IS-NOT-SPLIT-INTO-A-CLOSE-AND-AN-ARGUMENT;
a third, `1>&3- 2>&3-` in one list, is TD-OILS14 (the order-free `RedirPlan`)
rather than anything about moves.
