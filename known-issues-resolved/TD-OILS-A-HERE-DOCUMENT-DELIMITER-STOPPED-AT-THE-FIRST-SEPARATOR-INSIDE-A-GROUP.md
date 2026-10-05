### TD-OILS-A-HERE-DOCUMENT-DELIMITER-STOPPED-AT-THE-FIRST-SEPARATOR-INSIDE-A-GROUP. `<<E$(a b)` was read as a delimiter `E$(a` and a stray word `b)` — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::read_heredoc_delim`, and the
new `Lexer::delim_group_at` / `take_delim_group_at` / `take_delim_group` beside
it.

**What.** bash reads a here-document delimiter with `read_token_word`, the same
function that reads every other word, so a `$( … )`, `$(( … ))`, `${ … }` or
`` ` … ` `` in it is scanned as a matched pair and becomes part of the
delimiter's text. osh's delimiter scan instead stopped at any metacharacter,
which cut those groups in half:

```text
cat <<E$(a b)          bash: delimiter `E$(a b)`
body                   osh:  delimiter `E$(a`, then a word `b)` — and then the
E$(a b)                      here-document was never delimited at all
```

Nothing is ever *expanded* — the text is compared against each body line
literally, so `<<E$(echo 1)` is closed by a line reading `E$(echo 1)`, not by
`E1`. bash only has to *scan* the substitution to know where the word ends. Nor
does a group quote the word: `<<E$(echo "a b")` still expands `$x` in the body,
unlike `<<"E"`.

**Also fixed: a group that never closes.** The first cut of the scan swallowed
input to the end and left the fragment in the delimiter. bash's
`parse_matched_pair` end-of-input path is fatal — `unexpected EOF while looking
for matching \`)'`, `rc=2`, the delimiter word discarded. Which line it blames
says which of bash's readers took the group, and they are not the same:

| delimiter | reader | line blamed |
|---|---|---|
| `<<E${` | `parse_matched_pair` | the opener's line (its `start_lineno`) |
| ``<<E` `` | `parse_matched_pair` | the opener's line |
| `<<E$((` | `parse_matched_pair` (`P_ARITH`) | the opener's line |
| `<<E$(` | `parse_comsub` | the **end of the input** |

Nesting is what makes the first three visible: `cat <<E${` / `body` / `E${` is
blamed on line 3, because the `${` on line 3 opened a recursive
`parse_matched_pair` call with a `start_lineno` of its own. The `$(` row stays
pinned to end-of-input however deep it nests, there being no `start_lineno` in
play at all — `cat <<E$({` / `a` / `b` / `c` is blamed on line 5, one past the
file. So the openers are kept apart in `take_delim_group_at` rather than lumped
into one paren scan, which is what an earlier attempt did (it got every
delimiter's *text* right and every unterminated one's *line* wrong).

Two smaller rules fell out of reading `parse_matched_pair` (parse.y:3775–3781,
3936–3966) rather than guessing:

* A bare `open` counts only where bash counts it. `${ … }` is entered with
  `P_FIRSTCLOSE`, so a lone `{` inside it neither opens nor closes anything;
  inside `$(( … ))` every `(` counts.
* Nothing nests inside a backquote pair — bash guards its `$(`/`${` branch with
  ``open != '`'`` — and the pair itself never nests. So `` cat <<E`a `` /
  `body` / `` E`a `` closes on line 3's backquote, wants a delimiter with two
  newlines in it that no line can equal, and leaves `b` as a separate argument.

**Pinned by** `tests/corpus/a-here-document-delimiter-is-a-whole-word-groups-and-all.sh`
(which fails on its *first* case without the fix) and two unit tests,
`a_here_document_delimiter_takes_a_group_whole` and
`an_unclosed_group_in_a_here_document_delimiter_is_fatal`.

**Left behind:** TD-OILS-A-PENDING-HERE-DOCUMENT-IS-NOT-GATHERED-WHEN-A-TOP-LEVEL-LIST-IS-REDUCED,
found while writing the corpus case.
