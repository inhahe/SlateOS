### TD-OILS-BULK-CASE-OP-JOINS-A-NOSPLIT-CONTEXT-WITH-IFS. `IFS=:; x=${a[@]^^}` is `A:B:C:D` in osh and `A:B C:D` in bash — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::joined_value`, the `!split`
context's joiner, which had arms for `ArraySlice` and `ArrayKeys` but none for
`ArrayBulk`, so every bulk operator fell through to `expand_dynamic_with`'s
`join_derived(&items, star)` (`at_sep()`, i.e. `$IFS`'s first character) where
half of them want a space.

**What.** In an assignment or a redirect target a list-valued expansion is
space-joined under every `$IFS` — `IFS=:; a=(a:b c:d); x=${a[@]}` is
`a:b c:d`, and osh already matched that for the plain reference and, since
`TD-OILS-NOSPLIT-SLICE-AND-KEYS-JOIN-WITH-IFS`, for slices and key lists. Half
the **bulk operators** were missed. With `n=(a:b c:d)` and the value of `x=…`
shown:

| shape | `IFS=:` | `IFS=` | `IFS=' :'` | osh was, under `IFS=:` |
|---|---|---|---|---|
| `${n[@]}` | `a:b c:d` | `a:b c:d` | `a:b c:d` | ✓ |
| `${n[@]:0}` | `a:b c:d` | `a:b c:d` | `a:b c:d` | ✓ |
| `${!n[@]}` | `0 1` | `0 1` | `0 1` | ✓ |
| `${n[@]#a}` | `:b:c:d` | `:b c:d` | `:b c:d` | ✓ |
| `${@#a}` | `:b:c:d` | `:b c:d` | `:b c:d` | ✓ |
| `${n[@]@Q}` | `'a:b':'c:d'` | `'a:b' 'c:d'` | `'a:b' 'c:d'` | ✓ |
| `${n[@]^^}` | `A:B C:D` | `A:B C:D` | `A:B C:D` | ✗ `A:B:C:D` |
| `${n[@]/a/Z}` | `Z:b c:d` | `Z:b c:d` | `Z:b c:d` | ✗ `Z:b:c:d` |
| `${@^^}` | `A:B C:D` | `A:B C:D` | `A:B C:D` | ✗ `A:B:C:D` |
| `${n[*]#a}` | `:b:c:d` | `:bc:d` | `:b c:d` | ✓ |
| `${n[*]^^}` | `A:B:C:D` | `A:BC:D` | `A:B C:D` | ✓ |

(The `${@#a}` row read `:b c:d` when this entry was first written. That was a
misreading of the probe output; bash gives `:b:c:d`, and the positionals answer
exactly as a named array does throughout this table.)

**Quoting is the other half of the rule, and the entry above omitted it.** The
space belongs to the *unquoted* spelling alone. Quoting the very same expansion
puts `$IFS` back:

| shape | unquoted `x=…` | quoted `x="…"` |
|---|---|---|
| `${n[@]^^}` | `A:B C:D` | `A:B:C:D` |
| `${n[@]/a/Z}` | `Z:b c:d` | `Z:b:c:d` |
| `${n[@]#a}` | `:b:c:d` | `:b:c:d` |
| `${n[@]@Q}` | `'a:b':'c:d'` | `'a:b':'c:d'` |

This is unlike the slice's space, which survives quoting-by-context, and it is
what makes the fix's *placement* matter: `expand_dynamic_with` sees both
spellings arrive together and cannot tell them apart, while `joined_value` is
reached only from a word's unquoted parts.

**Note on the "Proper fix" as first written.** It proposed sharing
`split_items`' `trims_an_array` predicate — "every operator except a trim of a
**named** array is a list". That is wrong twice over. It splits the operators in
the wrong place (`@Q`/`@E`/`@P`/`@a` keep `$IFS` here as well as trim does), and
it distinguishes named arrays from the positionals, which this context does not.
Building it produced two regressions (`x=${n[@]@Q}` became `'a:b' 'c:d'`,
`x=${@#a}` became `:b c:d`) and they were caught only by re-probing. The
splitting classification and the joining classification are genuinely different
questions about the same operators; they are deliberately two tests.

**Fixed** by adding a guarded `ArrayBulk` arm to `Shell::joined_value`, beside
the existing `ArraySlice`/`ArrayKeys` arms, and a new free function
`bulk_joins_with_ifs(op)` carrying the measured split: `Trim`, `Transform` and
`BadTransform` join with `$IFS`; `Replace` and `Case` take the space. The
`[*]` spellings never reach the arm (it matches `star: false`) and stay on
`star_sep()`. Corpus case
`a-joined-context-joins-a-case-and-a-replace-list-with-a-space.sh` pins all four
`$IFS` regimes, both quotings, the positionals, the other assignment spellings,
a redirect target, and the empty and one-element lists.
