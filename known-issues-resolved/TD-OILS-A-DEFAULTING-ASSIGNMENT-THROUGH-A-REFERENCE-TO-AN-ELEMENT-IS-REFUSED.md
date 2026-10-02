### TD-OILS-A-DEFAULTING-ASSIGNMENT-THROUGH-A-REFERENCE-TO-AN-ELEMENT-IS-REFUSED. `declare -n r='n[0]'; echo "${r:=D}"` said `` `n[0]': not a valid identifier `` where bash stores `D` in `n[0]` — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::assign_elem`.

**What.** `${name:=word}` writes through a nameref like any other store, and a
reference designating an **element** is a perfectly good destination for it.
osh refused every target carrying a subscript, whether the subscript came from
the reference or was written beside it — but only the second is a subscript on
a subscript. Measured:

```text
                                       bash                osh (before)
n=(); declare -n r='n[0]'
  echo "[${r:=D}]"                     `[D]`               `` `n[0]': not a
                                       `declare -a n=      valid identifier ``
                                       ([0]="D")`
n=(a); declare -n r='n[0]'
  echo "[${r:=D}]"                     `[a]`, no store     the same — agreed
declare -a k=(); declare -n g='k[*]'
  echo "[${g:=v}]"                     `k[*]: bad array    the same — agreed
                                       subscript`
n=(a b c); declare -n r='n[0]'
  echo "[${r[1]:=D}]"                  refused             the same — agreed
```

The present-element case agreed only because nothing is ever stored there: the
refusal was unreachable when the element already had a value.

**Fixed 2026-08-05.** `assign_elem` reached its refusal through
`RefTarget::as_name`, which answers `None` for any subscripted target. The two
are now split: with no subscript of its own, a subscripted target *is* the
element destination `base[sub]`, taken through `Shell::sub_word` and handed to
the ordinary subscripted store — which brings that path's readonly guard, key
rules and diagnostics with it. Only a target carrying *both* subscripts is
refused.

Two orderings come with it, both measured. The base is bound where it is
written, so a reference on it is stripped (`Shell::unreference_elem_base`)
*before* the whole-array refusal reports anything — bash gives the warning
first and calls `base[*]` a bad subscript second. And a subscript that will not
evaluate has already been reported by the read that led here, the bytes being
the reference's rather than the caller's, so the store does not report it again.

**Corpus:** `a-defaulting-assignment-through-a-reference-to-an-element.sh` —
which subscripts element 0 throughout, bash crashing on every other shape (see
TD-OILS-BASH-CRASHES-ON-A-DEFAULTING-ASSIGNMENT-THROUGH-A-REFERENCE).
