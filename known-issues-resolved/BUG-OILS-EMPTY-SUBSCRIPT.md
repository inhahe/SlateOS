### BUG-OILS-EMPTY-SUBSCRIPT. A lexically empty subscript `a[]` was read as index 0, and `${a[]}` was a fatal *parse* error that killed the script — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/arith.rs` — the atom lexer's subscript capture,
which handed an empty bracket text straight to `Sub::parse("")` (yielding 0); and
`userspace/oils/src/parser.rs` — `split_name_subscript`, which returned
`Err(ParseError::new("empty array subscript '[]'"))`.

**Symptoms.** Two independent divergences, one of them script-fatal.

*Arithmetic.* `a[]` was silently treated as `a[0]`, so `((a[]=9))` wrote element
0 and `declare -i k=a[]` read it. bash refuses the empty subscript, but only
*complains* — the expression carries on either way:

| shape | bash | osh (before) |
|---|---|---|
| `((a[]))` (read) | `a[]: bad array subscript` **twice**, value 0, rc=1 | silent, reads `a[0]` |
| `x=$((a[]))` | same two lines, `x=0`, rc=0 | silent, `x=${a[0]}` |
| `((a[]=9))` (store) | `` ((: `a[]': not a valid identifier ``, store dropped, rc=0 | silent, wrote `a[0]` |
| `x=$((a[]=3))` | untagged `` `a[]': not a valid identifier ``, `x=3`, rc=0 | silent store |
| `let 'a[]=5'` | `` let: `a[]': not a valid identifier `` | silent store |
| `((a[]++))` / `((a[]+=2))` | two read lines *then* the identifier refusal | silent |
| `(( 1 ? 7 : a[] ))` | silent — never reached | silent |
| `((a[  ]))` | index 0 — blanks are not empty | index 0 (already right) |

The read line really is printed twice per read reached (`$(( a[] + a[] ))` prints
four), because bash validates the subscript once when resolving the reference and
again when fetching the value. The refusal does not depend on the name at all —
an unset name, a scalar and an associative array are refused identically — and a
nameref is **not** followed: `declare -n r=x; ((r[]=1))` blames `r[]`.

*Brace expansion.* `${a[]}` was a **parse** error in osh, so the whole script
died with status 2 before running a line — even a guarded
`if false; then echo "${a[]}"; fi`. In bash it parses fine and is only rejected
when the word is expanded, as an ordinary runtime "bad substitution": it abandons
the rest of its list, sets `$?`=1, and the script continues. `${#a[]}`,
`${!a[]}`, `${a[]:-def}` and `${a[]#o}` all behave the same, and the subject
named is the whole word with its quotes removed (`echo "X${a[]}Y"` →
`X${a[]}Y: bad substitution`).

**Fixed** in two places, matching where bash makes each decision.

`arith.rs` gained `Expr::EmptySub(String)`, produced by the lexer when the
bracket text is empty — deliberately *ahead* of the indexed/associative split,
since bash refuses without looking at the name. It is an lvalue (`Lvalue`/
`ResolvedLv::EmptySub`), so all five arithmetic shapes route through it, and the
two behaviours land on new `VarLookup` hooks: `warn_empty_subscript_read` (called
by `eval_expr` and by `load_rlv`, so a read-modify-write earns both the read
complaint and the store refusal, in bash's order) and
`refuse_empty_subscript_store` (called by `store_rlv`, returning `Ok` so the
expression keeps its value). Because the complaint is made at *eval* time, a
short-circuited `(( 1 ? 7 : a[] ))` stays silent, as in bash. `Shell`'s impls
print the two-line read complaint verbatim and reuse `warn_elem_not_identifier`
for the store, which already applies the `((`/`let` tag and drops the store —
the same shape as TD-OILS-NAMEREF-ELEM-ARITH-LVALUE below.

`parser.rs`'s `split_name_subscript` now returns a `NameSubscript` enum whose
`Deferred` variant means "parses, but bash complains at expansion time"; all
three callers turn it into the existing `WordPart::BadSubst(raw)`, which is
already the DISCARD-class bad-substitution path.

**Tests.** `interp::tests::a_lexically_empty_subscript_is_refused_rather_than_read_as_zero`,
`interp::tests::an_empty_subscript_in_a_brace_expansion_is_a_runtime_bad_substitution`,
and `tests/corpus/empty-subscript.sh` (all of the above, plus the `a[  ]`
control).
