### TD-OILS-A-DECLARATION-WITH-NOTHING-TO-DO-BINDS-A-NULL-THROUGH-THE-REFERENCE. `declare -n q='n[1]'; declare q` makes bash's `n` read as empty — 2026-08-06 — ⚖️ WAIVED 2026-08-15 by the operator (`design-decisions.md` §309)

> **RESOLVED 2026-08-15 — deliberately NOT reproduced.** The operator answered
> `open-questions.md` Q40 with **option B**: osh keeps `Str` array elements and
> the array reads normally. This is a **knowing, documented divergence from
> measured bash** — the first of its kind in oils — not an unfixed bug. Do not
> "fix" it by implementing option A.
>
> **This entry stays open-shaped on purpose**, because it is the record that
> makes the divergence reversible: if a real script is ever found that depends
> on the null-element behaviour, everything needed to reproduce it is below.
>
> **The precedent it set matters more than the case.** Byte-fidelity with bash
> now has an *"unless it is a defect"* clause. Per §309, a measured behaviour may
> be waived only when all three hold: (i) it is unreachable except through a
> construct built to reach it, (ii) it is inconsistent with bash's own
> observable model, and (iii) reproducing it would degrade osh's value model.
> **Any future waiver must be argued against those three tests, here, in
> writing** — a waiver that is not written down is a divergence, not a decision.
> This does not loosen §305's frozen fidelity scope: §305 says what is in scope,
> §309 says something in scope may still be waived as a defect.

**Where:** `userspace/oils/src/interp.rs` — `Shell::declare_ref_bind_read` reads
the element the reference designates but performs no store; the store would have
to reach `Shell::arrays` / `Shell::assoc`, whose element type is `Str`.

**What.** The bind described in the entry above is passed a **null value**, not
an empty string. bash's `array_insert` puts that null pointer in the element,
and from then on every reader of the array trips over it — the array reads as
having no elements at all, while the elements are still there:

```text
$ n=(a b c); declare -n q='n[1]'; declare q
$ declare -p n            # bash: declare -a n      osh: declare -a n=([0]="a" [1]="b" [2]="c")
$ echo "${#n[@]} [${!n[@]}] [${n[@]}]"
  bash: 0 [] []           osh: 3 [0 1 2] [a b c]
$ echo "${n[1]-UNSET}"    # bash: UNSET             osh: b
$ n[5]=z; declare -p n    # bash: declare -a n=([0]="a" [1]= [2]="c" [5]="z")
```

The last line is the tell: `[1]=` prints without quotes, and `[0]`/`[2]` are back
— so nothing was ever removed. It is the null in element 1 that the readers
cannot walk past, and one further store past the end makes them able to again.

The same happens to an associative base (`declare -A m=([k]=v [j]=w)` reads as
`declare -A m` afterwards), to a scalar base (`n=abc` becomes an empty
`declare -a n` rather than osh's `declare -a n=([0]="abc")`), and to a
**readonly** one — the bind carries `ASS_FORCE`, so `readonly n` does not stop
it and no `readonly variable` is reported. Every flag that makes the command ask
for something takes it off this path, exactly as in the entry above.

**Why.** `declare q` → `bind_variable("q", NULL, ASS_FORCE)` →
`bind_variable_internal` follows the last nameref, sees `valid_array_reference
("n[1]")` and calls `assign_array_element("n[1]", NULL, …)` →
`bind_array_variable("n", 1, NULL, ASS_FORCE)` →
`bind_array_var_internal` → `make_array_variable_value(…, NULL, …)` returns
`NULL` → `array_insert(array_cell(n), 1, NULL)`.

**Proper fix.** This is very likely an upstream bash bug rather than a designed
behavior — a null element value is not otherwise reachable, and the array is
left in a state no bash-level operation can produce or explain. Reproducing it
would mean making osh's element type nullable (`Option<Str>`) and teaching every
array reader — listing, `${!a[@]}`, `${#a[@]}`, `${a[@]}`, `${a[i]-D}` — to stop
at the first null, which is a large change to the value model in service of a
defect. **Asked** — `open-questions.md` **Q40**, with the three options (reproduce
it, waive it, or reproduce only the visible half) and the recommendation to
waive. Nothing here is blocked on the answer; osh does the sane thing meanwhile
and the array keeps its elements. Probes: `/d/tmp/hh/bo.sh` (T-series)
and `/d/tmp/hh/bp.sh` (U-series).
