### TD-OILS-UNSET-THROUGH-A-REFERENCE-TO-AN-ELEMENT-CONSULTS-READONLY. `n=(a b c); readonly n; declare -n r='n[0]'; unset -v r` refused where bash removes the element — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::unset_element_unchecked`
(new), and `Shell::unset_element` reduced to the guard in front of it.

**What.** bash consults the readonly guard for a subscript the *writer wrote*
and not for one that arrived **through a reference**: `unset 'n[0]'` on a
readonly `n` is refused, and `declare -n r='n[0]'; unset -v r` performs the very
same removal. Measured with `n=(a b c)` and `readonly n`:

```text
                                       bash                     osh (before)
unset -v 'n[0]'                        `unset: n: cannot unset:  the same — agreed
                                       readonly variable`, s=1
declare -n r=n; unset -v 'r[0]'        the same refusal          the same — agreed
declare -n r=n; unset -v r             the same refusal          the same — agreed
declare -n r='n[0]'; unset -v r        s=0, `declare -ar n=      the refusal
                                       ([1]="b" [2]="c")`
declare -n r='n[@]'; unset -v r        s=0, `declare -ar n=()`   the refusal
s=hi; readonly s
  declare -n r='s[0]'; unset -v r      s=0, `s` gone             the refusal
```

A readonly *scalar* is removed outright this way, which is as far from the
guard as it gets. bash's `unset_builtin` reaches `unbind_array_element` directly
for a nameref whose value carries a subscript, and nothing on the way asks about
the attribute.

**Fixed 2026-08-05.** `unset_element` is now the guard plus
`unset_element_unchecked`, and the reference branch calls the latter. The
written-subscript path keeps both halves, so nothing else moves.

**Corpus:** `unset-through-a-reference-to-an-element-follows-the-base.sh`.
