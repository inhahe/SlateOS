### TD-OILS-NOSPLIT-SLICE-AND-KEYS-JOIN-WITH-IFS. `IFS=:; a=${n[@]:0:2}` is `x:y` in osh and `x y` in bash — 2026-08-04 — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the `ArraySlice` and `ArrayKeys` arms
of `Shell::expand_dynamic_with`, which both end in `join_derived(&items, star)`.
For `star == false` that is `at_sep()` (`$IFS`'s first character), and it is the
right answer in the *quoted* and *redirect-target* contexts but not in the
one-string ones.

**What.** In a context that wants one string and does no splitting — an
assignment RHS, a `[[ ]]`/`case` word or pattern, a here-string — bash joins an
*unquoted* `[@]` list with a space, whatever `$IFS` says. It does that for
`${n[@]}`, `$@`, `${n[@]:-d}` and `${!ref}` (all of which osh already matches,
via `join_elements`), and also for the two forms below, which osh joined with
`$IFS` instead:

```sh
declare -a n=(x y z); set -- p q; IFS=:
a=${n[@]:0:2}; echo "[$a]"     # bash [x y]    osh [x:y]
a=${@:1:2};    echo "[$a]"     # bash [p q]    osh [p:q]
a=${!n[@]};    echo "[$a]"     # bash [0 1 2]  osh [0:1:2]
```

The `[*]` spellings all join with `$IFS`'s first character and are correct, as
are the *quoted* forms — `a="${n[@]:0:2}"` really is `x:y` in bash, so the two
contexts genuinely disagree and the join cannot simply be changed in one place.
`${a[@]@Q}`/`${a[@]#x}` (`ArrayBulk`) and `${!prefix@}` (`VarNames`) are the
other way round again: those *do* join with `$IFS` even unquoted-and-unsplit, and
osh matches. Measured against bash 5.2.37 over `${n[@]}`, `${n[*]}`, both slice
spellings, `@Q`, `#`, both key spellings, `${!pre@}`/`${!pre*}`, `${!ref}`, `$@`,
`$*` and both positional slices under three `$IFS` settings.

**Fix.** A `Shell::joined_value(part) -> Option<Str>` beside
`Shell::split_items`, answering `join_elements(&items, false)` — a space — for
`ArraySlice { star: false }` and `ArrayKeys { star: false }` and `None` (defer to
`expand_dynamic`) otherwise. It is called from the `other` arm of the four
one-string expanders: `expand_word_joined` (the `[[ ]]`/`case` word and the
here-string), `expand_assignment_value_inner` (`a=…`, `a[0]=…`),
`expand_decl_assignment` (`export`/`declare`/`local`/`readonly` operands) and
`expand_word_pattern_inner` (the `case`/`[[ == ]]` pattern side).

The original write-up guessed the fix belonged in the `!split` branch of
`Shell::expand_word_annotated`, on the theory that a **redirect target** is
space-joined too. Measurement says otherwise: in posix mode (where a redirect
target is expanded but not split) bash gives `: > ${n[@]:0:2}` the name `x:y`,
i.e. `at_sep` — so that branch is a *fourth* context with the quoted answer, and
was deliberately left alone. Four contexts, three answers.

Covered by
`tests/corpus/a-joined-context-joins-a-slice-and-a-key-list-with-a-space.sh`
(both slice spellings, both key spellings, associative keys, both positional
slices, `${n[@]}`, `@Q`, `#`, `${!pre@}`, the quoted contrast and a literal-glued
form, each through an assignment, an array-element assignment,
`export`/`declare`/`local`/`readonly`, `[[ ]]`, `case` word *and* pattern and a
here-string, under three `$IFS` settings).

**Impact.** Only a one-string context holding an unquoted array *slice* or key
list under an `$IFS` whose first character is not a space. Narrow, but it was a
silent wrong value rather than an error.
