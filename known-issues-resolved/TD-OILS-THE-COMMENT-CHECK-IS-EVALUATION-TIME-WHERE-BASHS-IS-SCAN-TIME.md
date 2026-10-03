### TD-OILS-THE-COMMENT-CHECK-IS-EVALUATION-TIME-WHERE-BASHS-IS-SCAN-TIME. `x=5; echo "${x:-$(( #5 ))}"` reports in bash and not in osh, because bash finds the eaten `))` while *scanning* the word — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/ast.rs` — `WordPart::first_scanned_arith`;
`userspace/oils/src/interp.rs` — `Shell::begin_word` / `Shell::end_word` and
the seven word-entry sites they replace (`expand_word`,
`expand_redirect_word`, `expand_case_pattern`, `expand_case_subject`,
`expand_assignment_value`, `expand_word_pattern`, `expand_operand_fields`).

**What:** the first fix raised the complaint when the `$(( … ))` was
*evaluated*. bash raises it when the word is *scanned*, which is earlier and
happens whether or not the arithmetic is ever reached:

```sh
x=5; echo "${x:-$(( #5 ))}"    # bash: bad substitution; osh printed 5
x=5; echo "${x/a/$(( #5 ))}"   # bash: bad substitution; osh printed 5
echo "$(touch f)$(( #5 ))"     # bash never runs the `touch`; osh did
```

**Fixed** by walking the parsed word before expanding any of it.
`WordPart::first_scanned_arith` yields the first `$(( … ))` body bash's word
scanner would *reach*, and `Shell::begin_word` — one helper now standing in
for seven copies of the "name this word" prologue — runs it at the outermost
word entry. The match is **exhaustive**, with no `_ =>` arm, so a new
`WordPart` variant is a compile error rather than a silent gap; a miss here
would be invisible, being a diagnostic that simply never appears.

"Would reach" is the substance of it, and each case was measured rather than
assumed. Reached: every operand, pattern, replacement, subscript and slice
bound of a `${ … }`, and the contents of a double-quoted run. **Not** reached:
a `$( … )` or `<( … )` body — `${x:-$( echo $(( #5 )) )}` names the *inner*
`$(( #5 ))` and leaves the outer status at 0, because the complaint is raised
by the substitution's own expansion.

The evaluation-time check stays as the backstop for bodies no word walk
reaches. Pinned by
`the_closer_is_checked_when_the_word_is_scanned_not_when_it_is_evaluated` and
by seven new sections in the corpus case.

**Standing lesson:** *when* a shell decides something is part of its observable
behaviour, not an implementation detail. A check moved one phase earlier
changes which side effects happen at all — here, whether a command
substitution in the same word ever runs.
