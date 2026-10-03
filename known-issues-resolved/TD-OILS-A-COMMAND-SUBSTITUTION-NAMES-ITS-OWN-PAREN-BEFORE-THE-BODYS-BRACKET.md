### TD-OILS-A-COMMAND-SUBSTITUTION-NAMES-ITS-OWN-PAREN-BEFORE-THE-BODYS-BRACKET. `echo $(f[1` says `)` where bash says `]` — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/lexer.rs`, `read_dollar`/the `$(`…`)` body scan.
osh scans the substitution's text for its closing `)` before lexing the body, so
when the body holds an unclosed construct of its own the *outer* one is named.
bash reads the body with the same reader (`parse_comsub` → `parse_matched_pair`,
which recurses into the reader rather than counting parens), so the inner
failure is raised first.

This is the same shape as the `a=([1` row under
TD-OILS-AN-UNCLOSED-SUBSCRIPT-IN-AN-ASSIGNMENT-POSITION-IS-NOT-A-READER-ERROR,
which fixed itself when the subscript check landed because the array-literal
element reader already goes through `read_word_inner`. The `$(` body does not.

**Reproduce.**

```sh
eval 'echo $(f1[1';       echo "1 rc=$?"
eval 'echo $(time f2[1';  echo "2 rc=$?"
eval 'echo $(h[1 2]=v';   echo "3 rc=$?"
```

| row | bash 5.2.37 | osh |
|---|---|---|
| 1, 2 | `` …unexpected EOF while looking for matching `]' ``, `rc=2` | `` …matching `)' `` |
| 3 | `` …matching `)' `` — the subscript closes, so the substitution is the one left open | same |

Row 3 is the control: when the body holds nothing unclosed, both name `)`.

**The same root cause, the other way round.** The pre-scan also stops the
subscript *too early* when the `)` is present. bash's `[` scan does not know
about the substitution at all — it keeps reading, so it swallows the `)` and
whatever follows, and the pair that ends up unclosed is whichever one the text
after the `)` opens:

```sh
eval 'echo "$(f[1)""';    echo "1 rc=$?"
eval 'echo "$(f[1)"a"b"'; echo "2 rc=$?"
eval 'echo $(f[1)"';      echo "3 rc=$?"
```

| row | bash 5.2.37 | osh |
|---|---|---|
| 1 | `` …matching `]' `` — the two `"` after the `)` pair up, so the `[` scan runs to the end | `` …matching `"' `` |
| 2 | `` …matching `"' `` — three `"`, so the last one is the pair left open | `` …matching `]' `` |
| 3 | `` …matching `"' `` | same |

Row 3 matches only because osh's body error and bash's quote error happen to
name the same delimiter. All three are `rc=2` in both shells: the *fatality* of
a body that runs out is settled — see
`a-substitution-body-that-runs-out-costs-only-the-command.sh` — and what is left
is which delimiter gets named.

**The fix.** Read the `$(` body with the real reader and let its error out,
rather than pre-scanning for the `)` — and let a subscript the reader opens
inside it run past that `)` the way bash's does. Note the reporting line still
has to be the substitution's own when *it* is the construct left open (row 3 of
the first table), which `LexError::at`'s never-overwrite rule already gives.

Found while fixing TD-OILS-STARTS-COMMAND-DIVERGES-FROM-COMMAND-TOKEN-POSITION.

**Fixed**, though not by the rewrite proposed above — see below for why that is
not a deferral.

The body scan exists because a `$( … )`'s extent has to be known before its text
can be re-lexed, and it already mirrors every construct in which a `)` closes
nothing: quotes, `${ … }`, backticks, comments, here-documents, `case` patterns,
arithmetic spans, nested substitutions. An array subscript is simply the last
member of that set, and the *only* one that can run past the substitution's own
`)` and take it as text. Adding it completes the enumeration rather than
patching around a missing one, which is why it is the right shape of fix; the
one-pass rewrite would delete the scan altogether and is tracked as
TD-OILS-A-SUBSTITUTIONS-EXTENT-IS-FOUND-BY-A-SECOND-SCAN-RATHER-THAN-BY-PARSING.

`read_balanced_body` now carries a `sub_depth`/`sub_line` pair. It opens when
`CaseScan::at_subscript()` says so — bash's parse.y:5145-5146, which is
`assignment_acceptable (last_read_token) && token_is_ident (token, token_index)`
— and while it is up, nothing else in the scan runs: no `#`, no `<<`, no
here-document collection at a newline, no paren counting, no `case` feed. Only
`read_opaque_span` still runs ahead of it, which is what keeps `f[a"]"b`
looking for its `]`. At end of input a `sub_depth > 0` returns
`eof_matching(']').at(sub_line)` *before* the pending here-documents are
drained, because `read_token_word` bails on `&matched_pair_error` without ever
reaching the reduction that warns (parse.y:5150) — measured: `$(cat <<E; f[1`
names the `]` and warns about nothing.

Answering `at_command()` in there also needed the assignment shape, which
`CmdPos::word`'s `assign` argument had been fed `false` for. `AssignHead` is
bash's `assignment` (general.c) as a state machine over the characters —
`Name`, `Sub(depth)`, `SubEnd`, `Plus`, `Yes`, `No`. It has to be per-character
rather than a test on the finished word because bash runs `assignment` over the
token buffer *as written*: a quoted character in the name spoils it (`"v"=1` is
a command, so `$("v"=1 f[2` names `)`) while one in the value cannot (`v="a b"`
is an assignment, so `$(v="a b" f[2` names `]`).

Corpus: `a-subscript-in-a-substitution-body-outruns-the-substitution.sh`
(32 rows).
