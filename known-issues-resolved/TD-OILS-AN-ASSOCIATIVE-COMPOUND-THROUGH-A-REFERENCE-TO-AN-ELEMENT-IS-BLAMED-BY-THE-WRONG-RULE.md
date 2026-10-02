### TD-OILS-AN-ASSOCIATIVE-COMPOUND-THROUGH-A-REFERENCE-TO-AN-ELEMENT-IS-BLAMED-BY-THE-WRONG-RULE. `declare -n r='n[1]'; declare -A r=([k]=v)` says `` `n[1]': not a valid identifier `` where bash says `n: cannot convert indexed to associative array` — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::declare_compounds_scoped`.

**What.** `-A` names a *kind*, and bash decides the kind against the reference's
**base** before it ever asks whether the operand names an element. Measured with
`n=(a b c)`:

```text
                                       bash                     osh
declare -n r='n[1]'; declare -A r=([k]=v)
                                       `n: cannot convert       `` `n[1]': not a valid
                                       indexed to associative   identifier ``
                                       array`
declare -n r='n[1]'; declare r=(x y)   `` `n[1]': not a valid   the same — agreed
                                       identifier ``
```

The blame is the **base**, unsubscripted, which is a further sign that the kind
check runs on the array the reference points into rather than on the operand.

Re-measured 2026-08-05: `-a` is the same rule in the other direction —
`declare -A m=([k]=K); declare -n r='m[k]'; declare -a r=(x y)` gives
`m: cannot convert associative to indexed array` — and the check comes *before*
the bad-subscript line of the entry above, so `declare -n r='n[@]';
declare -A r=([k]=v)` gives the conversion error alone. Where the kinds already
agree nothing is said and the identifier refusal stands. Inside a function
*without* `-g`, where the literal binds a local named by the spelling, there is
no check at all: `declare -A r=([k]=v)` through `n[1]` or `n[@]` succeeds and
leaves `declare -A n[1]=([k]="v" )` with `n` untouched.

`-g` is the corner that shows the check is not the element store's. Since
TD-OILS-A--G-COMPOUND-THROUGH-A-REFERENCE-TO-AN-ELEMENT-BINDS-THE-SPELLING-TOO a
`-g` compound binds the spelling like the local one does — and bash *still* asks
the base:

```text
                                       bash                     osh
f() { declare -gA r=([k]=v); }         `f: n: cannot convert    binds `n[1]`, s=0,
  with a global `declare -n r='n[1]'`  indexed to associative   nothing said
  and `n=(a b c)`                      array` and then
                                       `declare: n: …` again,
                                       s=1, the list continues
f() { declare -gA r=([q]=v); }         binds `m[k]`, s=0 —      the same — agreed
  with `declare -A m=([k]=K)`          the kinds agree
f() { declare -g r=(x y); }            `declare -a nosuch` and  binds `nosuch[1]`
  with `declare -n r='nosuch[1]'`      `declare -a nosuch[1]=   alone
                                       ([0]="x" [1]="y")`
```

So bash reaches the base twice over: to kind-check it, and to *make* it — an
absent base is brought into being, empty, in the kind the literal asks for, even
though the value goes to the spelling. Which also says why the local case is
silent: without `-g` bash never reaches for the base at all.

Note the two error lines and the status. Where the element-store path discards
the rest of the parse unit (the top-level rows above print no `s=`, because a
declaration builtin's failed assignment word ends a non-interactive shell), the
`-g` shape reports from the compound machinery — tagged with the function's name
— *and* from the builtin, then merely fails: `echo "s=$?"` runs and prints 1.

**Fixed 2026-08-05.** `Shell::declare_compounds_scoped` reaches the base before
the `target` match, wherever the declaration would not bind a local. That is
one `array_kind_conflict` on `RefTarget::base` — raised through the shared
`Shell::compound_kind_refusal`, so it takes the two shapes of the entry above —
followed, where the operand goes on to bind a spelling, by an
`array_kind_apply` on a base that is not already an array of either kind. The
apply is bash's `find_or_make_array_variable`, which is also why a *scalar*
base is converted (its value carried in as element `0`) and an existing array
of the agreeing kind is left alone. Nothing is made at top level, where the
operand is refused as an identifier and the unit thrown away: bash's shell is
over by then, so a base it may have made is unobservable, and one osh left
behind would not be. Corpus:
`the-kind-of-a-compound-through-a-reference-to-an-element-is-the-base-arrays.sh`.

The *scalar* operand path asked the same question with the same misplaced blame;
that is the entry below, fixed the same day.
