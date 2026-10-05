### TD-OILS-AN-ARITHMETIC-STRING-NAMES-ITS-COMMAND-SUBSTITUTION-AS-WRITTEN. `$(echo a>&2)` should come back as `$(echo a 1>&2)` — 2026-08-07 — ✅ FIXED 2026-08-07 for the expansion spellings; the `(( … ))` command is tracked separately

**Where:** `userspace/oils/src/parser.rs` — `parse_arith_comsubs`, which parsed
each nested body and threw the parse away, and `seg_to_part`'s `Seg::Arith` arm,
which kept the scan's source text; `userspace/oils/src/lexer.rs` —
`CmdSubSpan`, which recorded no position for the body it collected.

*(The first note here guessed `interp.rs` and `Shell::bad_sub_word`. Wrong
layer: the string is fixed at parse time and `interp` only reports it.)*

**What:** bash does not keep the source text of a `$( … )`. `xparse_dolparen`
parses the body and stores `make_command_string`'s *re-print* of the parse, so a
diagnostic that quotes an arithmetic string back shows the reconstruction —
redirections normalised (`>&2` → `1>&2`), compound commands re-laid-out across
lines. Backticks are not re-parsed this way and do come back verbatim.

osh already reconstructs on the **subscript** path (`${a[${x!}+$(echo b>&2)]}`
matches bash exactly), so this is only the `$(( … ))` / `$[ … ]` / `(( … ))` /
`let` family.

**Repro** (bash 5.2.37 left, osh right):

| input | bash | osh |
|---|---|---|
| `echo $(( ${x!} + $(echo a>&2) ))` | `` ${x!} + $(echo a 1>&2) : bad substitution`` | `` ${x!} + $(echo a>&2) : bad substitution`` |
| `echo $(( ${x!} + $(if true;then echo 1;fi) ))` | re-printed over three lines | one line, as written |

**Confirmed in the source, 2026-08-07.** `parse_comsub` ends
`tcmd = print_comsub (parsed_command); … return ret;` (parse.y:4219–4241) — the
text it appends to the enclosing scan **is** the re-print, not the source. The
comment on osh's `parse_arith_comsubs` (parser.rs) says the opposite ("keeps only
the *text* (`APPEND_NESTRET`)") and needs correcting along with the code: bash
throws the *source* away, not the parse.

**Measured shapes (bash 5.2.37, `LC_ALL=C`, `x` unset).** Everything outside the
`$( … )` is verbatim source; only the bodies are replaced.

```text
$(( ${x!} + $(echo a>&2) ))            ${x!} + $(echo a 1>&2)
$(( ${x!} + $(echo a # hi
) ))                                   ${x!} + $(echo a)          comment dropped
$(( ${x!} + $(echo "a  b" | cat) ))    ${x!} + $(echo "a  b" | cat)  quoting kept
$(( ${x!} + `echo a>&2` ))             ${x!} + `echo a>&2`        backtick verbatim
$(( ${x!} + $( ( echo a ) ) ))         ${x!} + $( ( echo a ))
$(( ${x!} + $( { echo a; } ) ))        ${x!} + $({ echo a; })
$(( ${x!} + $(cat <<E
hi
E
) ))                                   ${x!} + $(cat <<E
                                       hi
                                       E
                                       )                          here-doc body follows
$(( ${x!} + $(if true;then echo 1;fi) ))
                                       ${x!} + $(if true; then
                                           echo 1;
                                       fi)
$(( ${x!} + $(for i in a b; do echo $i; done) ))
                                       ${x!} + $(for i in a b;
                                       do
                                           echo $i;
                                       done)
```

The `$( ( … ))` spacing is bash's own guard: `if (tcmd[0] == '(')` inserts a
space so the result cannot re-read as `$((` (parse.y:4221–4227). `$[ … ]`,
`(( … ))` and `let` all show the same re-print.

**It *is* observable in execution — an earlier note here said otherwise and was
wrong.** The claim was that `$LINENO` reports the source line regardless. It
does not; the re-print's extra newlines are counted:

```sh
q=$(echo $(( 0 + $(if true; then echo 0; fi) )); echo L=$LINENO)
#   written on line 19            bash: L=21          osh: L=19
```

The three-line re-print of the `if` puts the `echo L=$LINENO` after it two lines
further down inside the same `$( … )` body, and bash counts them. That makes it
a *consequence* of the layout, not of the splice: every re-print measured that
stays on one line leaves `$LINENO` alone, so this row moves only when
TD-OILS-A-REPRINTED-COMPOUND-COMMAND-IS-KEPT-ON-ONE-LINE is fixed, and moves to
agreement then. Every other difference (dropped comments, normalised redirects)
is semantics-preserving.

It also confirms the shape of the fix: bash really does splice the re-print into
the stored text, so a diagnostic-only side channel would have been wrong.

**Fixed 2026-08-07 for `$(( … ))`, `$[ … ]` and the `$((`-that-is-not-an-
expression fallback.** `lexer::CmdSubSpan` gained the byte range the `$( … )`
occupied in the scan's buffer (`shift_spans` moves the ranges when one scan's
buffer is spliced into another's, and the `$((` arm slides them one byte left
because the expression is the buffer minus the counted `(`).
`parser::parse_arith_comsubs` now returns each body's re-print beside its range
instead of discarding it, and `splice_reprints` writes them back right-to-left.
The `let` and `${a[…]}` spellings already agreed — they reach the string through
a word, whose parts `unparse` was already re-printing.

Corpus case:
`tests/corpus/an-arithmetic-string-carries-its-command-substitution-re-printed.sh`.

**The `(( … ))` command and the `for (( … ))` header** were logged separately as
TD-OILS-AN-ARITHMETIC-COMMAND-DOES-NOT-PARSE-ITS-SUBSTITUTIONS, because for
those the *eager parse* was missing too, which was the larger half; that entry is
now ✅ FIXED as well. Still open for a `${ … }` body that defers, logged as
TD-OILS-A-DEFERRED-BRACE-BODY-KEEPS-ITS-SUBSTITUTION-AS-WRITTEN.
