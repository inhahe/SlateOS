### TD-OILS-A-REPRINTED-SUBSTITUTION-BODY-LOST-BASHS-LEADING-SPACE-GUARD. `$( (echo a) )` came back `$(( echo a ))` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/unparse.rs` — `part_src`'s
`CmdSubBody::Parsed` and `WordPart::ProcSub` arms.

**What was wrong.** Both wrote the re-print straight after the opening
delimiter. bash does not: `parse_comsub` checks the re-print's first byte and
prepends a space when it is `(` (parse.y:4221–4227, comment
`") need a space to prevent arithmetic expansion"`), because a body that opens
with a subshell would otherwise make the whole construct start `$((`.

```text
                            bash 5.2.37            osh (before)
: $( (echo a) )             : $( ( echo a ))       : $(( echo a ))
: <( (echo 2) )             : <( ( echo 2 ))       : <(( echo 2 ))
: ${a[$( (echo 2) )]}       : ${a[$( ( echo 2 ))]} : ${a[$(( echo 2 ))]}
```

The first is a silent meaning change — the printback is an arithmetic
expansion of `echo a` — and the second does not parse at all, so
`eval "$(declare -f g)"` on such a function was a syntax error.

The guard belongs to `parse_comsub`, and `read_token_word` sends all three of
`$(...)`, `<(...)` and `>(...)` through that one call (parse.y:5028–5042) —
which is why the process substitution needed it too. A backtick body (never
re-printed) and a `$((` that fell back to a substitution (whose leading `(` is
the one the source wrote) are both correctly left alone.

**Fixed** by `unparse::comsub_reprint`, which both arms now share: it takes the
opening delimiter, renders the body, and inserts the space when the body opens
with `(`.

Corpus case:
`tests/corpus/a-re-printed-substitution-body-that-opens-with-a-subshell-keeps-a-space.sh`.
Found while measuring
TD-OILS-AN-ARITHMETIC-STRING-NAMES-ITS-COMMAND-SUBSTITUTION-AS-WRITTEN, which
also turned up TD-OILS-A-REPRINTED-COMPOUND-COMMAND-IS-KEPT-ON-ONE-LINE.
