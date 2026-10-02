### TD-OILS-UNQUOTED-ARRAY-EXPANSION-IS-A-JOINED-STRING. `IFS=:; echo ${a[@]}` is one field in osh and three in bash — 2026-08-04 — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the `other` arm of
`Shell::expand_word_annotated`, which asked `expand_dynamic` for a single string
and then field-split it. Every multi-element reference therefore had to survive a
round trip through a joined string, and the join it got was the `[@]`-form space.

**What.** An unquoted `${a[@]}` is a *list* in bash, and osh joined it with a
space and split the result, so an `$IFS` without a space in it lost the
boundaries:

```sh
declare -a n=(x y z)
IFS=:
printf '<%s>' ${n[@]}    ; echo    # bash <x><y><z>   osh <x y z>
IFS=
printf '<%s>' ${n[*]}    ; echo    # bash <x><y><z>   osh <xyz>
printf '<%s>' ${n[*]@Q}  ; echo    # bash <'x'><'y'><'z'>   osh <'x''y''z'>
```

**Fixed.** `Shell::split_items` now classifies the parts that expand to a list,
and the `other` arm lays that list out instead of a string. The rule the fix
implements was **measured** across five `$IFS` settings × ~30 forms (bash 5.2.37)
and is not the one this entry originally proposed:

* Unquoted, the `[*]` spelling says *nothing* — `${a[*]}` splits exactly as
  `${a[@]}`, and `$*` as `$@`. The star only chooses a separator inside double
  quotes.
* With a **non-null** `$IFS` the answer is "join the items with `$IFS`'s first
  character, then field-split" — *not* "split each item and force a break
  between". The two differ on empty items: `z=('' q ''); IFS=:` gives `<><q>` in
  bash, which is the join talking (`:q:`), where a forced break would give a
  third, empty field.
* With a **null** `$IFS` nothing can split, so the list itself is the fields:
  consecutive items are separate fields and an empty item adds no characters.
  Adjacent literal text still joins the ends (`A${n[@]}B` → `<Ax><y><zB>`).
* Two forms are lists only as a way of building one string, and stay a single
  field under a null `$IFS`: a `#`/`##`/`%`/`%%` trim of a *named array*
  (`${a[@]#x}` → `< y z>`) and `${!a[*]}` (→ `<0 1 2>`), both joined with
  `at_sep`. The same trim of the *positionals* (`${@#x}`) is a real list —
  measured, not derivable. `${!prefix*}` is joined too but with `star_sep`
  (nothing), which osh already did.
* The **`!split`** contexts (assignment RHS, redirect target) were left alone
  *here*: there `${n[@]}` is space-joined under every `$IFS`, which osh already
  matched. (The slice and key-list forms of the same contexts were not, and were
  fixed separately — see `TD-OILS-NOSPLIT-SLICE-AND-KEYS-JOIN-WITH-IFS`.)

Covered by
`tests/corpus/an-unquoted-list-parameter-splits-as-a-list-not-as-a-joined-string.sh`
(29 forms × 4 `$IFS` settings, plus elements holding `$IFS` characters, empty
elements, and the `!split` contrast). The unit test
`an_array_subscript_can_point_as_well_as_list_keys` had baked in the old
`${!h[*]}` answer (`01x`) and was corrected to bash's `0 1x`.

**Still divergent (separate entry):**
`TD-OILS-QUOTED-OPERAND-WORD-LIST-DOES-NOT-SPLIT-ITS-LITERAL`. (The operand
entry that stood here, `TD-OILS-A-DEFAULT-WORDS-NESTED-LIST-IS-NOT-SPLIT`, is
resolved.)
