### TD-OILS-A-SUBSCRIPT-THAT-NAMES-AN-ARRAY-WHOLE-IS-NO-PLACE-TO-STORE. `n[*]=v` and `declare -n g='n[*]'; g=v` said `*: syntax error` where bash says `n[*]: bad array subscript` — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::whole_array_sub` and
`Shell::warn_whole_array_sub` (beside `Shell::ref_target_value`), consulted by
`Shell::scalar_write_dest`, `Shell::arith_write_dest`,
`Shell::apply_assignment_inner` (twice) and `Shell::assign_elem` (twice).

**What.** `n[*]` and `n[@]` name an array *whole*, which is no place to put one
value. bash refuses every such store with `base[sub]: bad array subscript`,
spelling the whole reference and storing nothing; osh handed the subscript to
`eval_arith_index`, which reported `*: syntax error: operand expected` and then
*stored at index 0* on the paths that carried on. Measured against bash 5.2.37
with `n=(3 5 7)` and `declare -n g='n[*]'`:

```text
                    bash                            osh (before)
n[*]=SV             `n[*]: bad array subscript`,    `*: syntax error`, same discard
                    rest of the parse unit dropped
declare 'n[*]=D'    same, status 1, carries on      `*: syntax error`, discarded
printf -v 'n[*]' x  same, status 1, n unchanged     `printf: *: syntax error`, stored
read -r 'n[@]'      same                            `read: @: syntax error`, stored
g=NEW               `n[*]: bad array subscript`     `*: syntax error`
printf -v g x       same, status 1                  `printf: *: syntax error`
(( g += 1 ))        same ×2, status 0               `((: *: syntax error`
"${g:=D}"           same                            `` `n[*]': not a valid identifier ``
```

**Fixed 2026-08-05.** `Shell::whole_array_sub` is the one place that says which
subscripts these are, and it recognises them as **tokens** — by the two bytes
themselves, never evaluated. That is what bash does too: `n["*"]=v`, `n[ * ]=v`
and `n[$s]=v` with `s='*'` are ordinary subscript expressions and fail as
arithmetic in both shells. `Shell::warn_whole_array_sub` gives the one
diagnostic, untagged by whichever builtin asked.

Three things had to be got right beyond the wording:

* **Where the subscript came from decides the associative case.** A *written*
  `m[*]=v` on an associative array stores under the key `*` like any other, and
  `$((m[*]))` reads that key — but the same subscript arriving through a
  `declare -n` reference is refused even there. So the refusal for a reference
  happens in `apply_assignment_inner` *before* the rewrite that would turn
  `g=v` into `ka[*]=v`, and the written-subscript sites carry the `!is_assoc`
  guard instead.
* **What follows the refusal differs by caller.** A plain assignment and
  `${g:=v}` drop the rest of the parse unit; `declare`/`local`, `printf -v` and
  `read` report status 1 and carry on; arithmetic reports it *without* failing,
  which is why `arith_write_dest` answers `Ok(None)` — `(( g += 1 ))` complains
  twice, once for the read walk and once for the write, and still exits 0.
* **It comes before the readonly guard and before the array is made.**
  `readonly ro; ro[*]=X` is the bad subscript, not the readonly variable, and
  `nope[*]=Q` leaves `nope` still not found.

**Corpus:** `a-subscript-that-names-an-array-whole-is-no-place-to-store.sh`.
