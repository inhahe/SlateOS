### TD-OILS-A-COMPOUND-LITERAL-IS-A-SYNTAX-ERROR-WHERE-BASH-PARSES-IT-AND-REFUSES-IT-AT-RUNTIME. `n[1]=(x y)` is `` syntax error near unexpected token `(' `` where bash says `n[1]: cannot assign list to array member` — 2026-08-05 — ✅ FIXED

**Where:** `userspace/oils/src/parser.rs` — the assignment-word rule, which
admits a compound literal only for an *unsubscripted* name.

**What.** bash's word rule for a compound literal is about the **word**, not the
destination: any `name[sub]=(…)` is an assignment word, and the objection to
putting a list in one element is raised later, when the value is bound. osh
decides at parse time, so the whole parse unit is lost instead — and lost
*noisily*, which is worse than the runtime complaint because a script that never
reaches the line still fails to parse. Measured:

```text
                                       bash                       osh
n=(a b c); n[1]=(x y)                  `n[1]: cannot assign list  `syntax error near
                                       to array member`, s=1      unexpected token `('`
n=(a b c); declare n[1]=(x y)          the same                   the same syntax error
n=(a b c); declare -n r=n
  r[1]=(x y)                           `r[1]: cannot assign list  the same syntax error
                                       to array member`
```

The name blamed is the one **written**, not the one it resolved to (`r[1]`, not
`n[1]`) — the rule an array-shaped write already follows; see
`Shell::assignment_write_refused`.

**Proper fix.** Let the parser build the assignment word with both a subscript
and an `AssignRhs::Array`, and refuse it in `Shell::apply_assignment_inner`
where every other destination objection lives — next to the reference refusal
recorded in
TD-OILS-A-COMPOUND-LITERAL-THROUGH-A-REFERENCE-TO-AN-ELEMENT-IS-STORED-INSTEAD-OF-REFUSED,
and blamed by the spelling rather than the resolved name. The status is 1 and,
being an ordinary failed assignment, it ends the parse unit.

**Impact.** Any script containing `a[i]=(…)` anywhere — a typo, or a
deliberately-unreached branch — refuses to parse at all under osh.

**Fixed 2026-08-05.** The lexer's `try_array_assign` now scans a balanced
subscript between the name and the `=`, carried on `Tok::ArrayAssign` as raw
source; `parse_simple` reads it into the assignment's `index` with
`word_verbatim_from_source`, exactly as the scalar form's is read. The refusal
lives in `Shell::refuse_list_to_array_member`, called from both places a
compound value is bound — `apply_assignment_inner` (which then arms the
ordinary abort) and `declare_compounds_scoped`'s operand loop, ahead of its
reference resolution, so `declare n[1]=(z) r=(x y)` is the subscript's
complaint while `declare r=(x y) n[1]=(z)` is the reference's.

Nothing is resolved or expanded first, as measured: the name blamed is the one
written (a nameref is never followed), the subscript is quoted as source
(`n[$((1/0))]` raises no division error, `n[$(f)]` does not run `f`, and
`n[1 ]` keeps its space), the literal's words are never expanded, and the
readonly guard is never reached. As a command *prefix* (`v=1 n[1]=(x y) true`)
it stays the other refusal — `` `n[1]': not a valid identifier ``, status
untouched, command run — which `prefix_assignment_name` already gave.

**Found alongside it:** *after* the command word osh named its own rule
(`syntax error: array assignment is only valid before the command word`) where
bash names the token that surprised it and echoes the line. That was not
specific to the subscripted form — `echo n=(x y)` diverged the same way — and
is fixed under
TD-OILS-A-COMPOUND-LITERAL-AFTER-THE-COMMAND-WORD-DOES-NOT-NAME-THE-TOKEN-THAT-SURPRISED-IT.

**Corpus:**
`a-subscripted-compound-assignment-parses-and-is-refused-when-it-binds.sh`.
