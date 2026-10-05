### TD-OILS-A-DECLARATION-BUILTIN-THAT-BINDS-A-LOCAL-THROUGH-A-REFERENCE-TO-AN-ELEMENT-MAKES-A-LOCAL-NAMED-BY-THE-SPELLING. `f() { declare -n r='n[1]'; declare r=zz; }` binds a local literally *named* `n[1]` — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_declare_scoped` (the
scalar operand path, where `target` is applied as `(base, sub)`),
`Shell::declare_compounds_scoped` (the compound operand path), and whatever
turns a nameref's value into a `RefTarget` on the read/write side.

**What.** A declaration builtin that binds a **local** follows a nameref the
current frame holds — and then makes a local whose *name* is the reference's
target **spelling, verbatim**. When the target carries a subscript that name is
not an identifier at all: bash happily creates a variable called `n[1]`, and
`n[1+1]`, `n[ 1 ]`, `m[k]`, `n[@]` alike — the text is never parsed. Measured
(`n=(a b c)` global, everything else inside `f`):

```text
                                        bash                       osh
declare -n r='n[1]'; declare r=zz       a local named `n[1]`,      a local array `n`
                                        `$r` is `zz`, `${n[1]}`    with `[1]="zz"`;
                                        still `b`, `set` lists     `${n[1]}` reads `zz`
                                        `n[1]=zz`
declare -n r='n[1]'; declare r=(x y)    the same, holding the      `` `n[1]': not a
                                        array; `${r[*]}` is        valid identifier ``
                                        `x y`
declare -n r='n[1]'; declare -i r=7+1   `declare -i n[1]="8"`      —
declare -n r='n[1]'; declare -A r=([k]=v)
                                        `declare -A n[1]=([k]="v" )`
                                                                   —
declare -n r='n[1]'; declare r          `declare -- n[1]`,         —
                                        created but unset
declare -n r='n[1]'; declare -i r       `declare -i n[1]`          —
declare -n r='n[@]'; declare r=zz       a local named `n[@]`       the refusal
declare -n r=n; declare r=(x y)         a local *array* `n`,       the same — agreed
                                        shadowing the global
declare -n r=nosuch; declare r=(x y)    a local `nosuch`           the same — agreed
```

So there is no special case at all: the rule is simply *make a local of the
target's spelling*, and the odd-looking outcomes follow from a spelling that is
not an identifier. **Every** shape of the operand makes one, valued or not — a
valueless `declare -i r` leaves `declare -i n[1]` created-but-unset. (An earlier
reading of this entry said a valueless operand created nothing; that was a
measurement artifact. It was taken with `set`, which lists only *valued* names.
`declare -p 'n[1]'` finds it, and bash reports `declare: n[1]: not found` for a
plain array, so the lookup really is by literal name.)

The binding is real, frame-scoped and reachable: any nameref whose value is that
same spelling finds it (`g() { declare -n s='n[1]'; echo "$s"; }` prints `zz`
while the frame lives), a write through such a reference lands on it, `declare -p
'n[1]'` prints it, and a nested frame making its own leaves the outer one
untouched. What does *not* see it is a plain `${n[1]}`, which is still the array
element — the two names only look alike.

Note the boundary: this is the **local**-binding path only. At global scope, and
under `-g` with a scalar value, bash follows the reference properly and stores
the array element (`declare -n r='n[1]'; declare r=zz` at top level really does
set `n[1]`), while a *compound* value there is refused with `` `n[1]': not a
valid identifier ``. The one shape that fits nothing is `-g` with a compound
value *inside a function*, which does nothing at all and says nothing — logged
separately below.

**Proper fix.** Where the operand follows a reference and the declaration binds
a local, use the target's spelling as the whole name and drop the subscript
— `(base_name, subscript)` becomes `(spelling, None)` — in both
`builtin_declare_scoped` and `declare_compounds_scoped`, and only when the
operand has a value. The read/write side then needs a nameref whose value is a
subscripted spelling to look for a binding of that literal name *before* it
treats the text as an element reference.

**Impact.** osh writes the caller's array where bash writes a throwaway local,
so a function using this shape silently corrupts data the caller still owns —
and refuses outright on the compound form.

**Fixed 2026-08-05,** in three places, all in `interp.rs`:

* `Shell::spelled_local_name` states the rule once — a local-binding
  declaration through a reference to an element is about the target's
  *spelling* as a whole name — and both declaration paths ask it.
  `builtin_declare_scoped` substitutes `(spelling, None)` for
  `(base, sub)` where it applies the resolved target;
  `declare_compounds_scoped` uses the spelling as its `target` and points the
  store at it through `apply_assignment_spelled`, since re-resolving the
  operand would find the element the name only looks like.
* `Shell::resolve_ref_name` now looks the reference's value up as a *name*
  (`Shell::name_is_bound`) before treating it as an element reference, which is
  what makes such a local reachable: any nameref spelled the same way finds it,
  from this frame or a deeper one, while `${n[1]}` still reads the array. It
  falls through to the plain-name step rather than returning, so the name is
  re-examined at the top of the walk like any other link.

One structural limit remains, documented at `spelled_local_name`: osh keys its
variable tables by `String`, so a subscript that is not valid UTF-8 cannot be a
name here and falls back to the element store. bash would bind it. This is the
same limit as TD-OILS-NONUTF8-ENV-NAME, reached from a different direction, and
the shell grammar cannot spell such a reference — only a `declare -n r=$'m[\xff]'`
built out of `$'…'` reaches it.

Two neighbours were measured on the way and logged separately rather than folded
in: TD-OILS-UNSET-N-ON-A-LOCAL-BINDING-FOLLOWS-A-REFERENCE-WHOSE-TARGET-DOES-NOT-EXIST
(`declare +n r`, which turns out to follow after all in exactly the cases where
the target is absent) and TD-OILS-SET-LISTS-A-DECLARED-BUT-UNVALUED-ARRAY.

**Corpus:**
`a-local-declared-through-a-reference-to-an-element-is-named-by-the-spelling.sh`
