### TD-OILS-NAMEREF-LOCAL-FRAME-DECLARATION-IS-A-BASH-BUG. a declaration against a circular nameref *local to the frame* leaves bash confused — 2026-08-04 — WONTFIX (bash bug, deliberately not copied)

Everything here is bash 5.2.37 inside `f() { declare -n c1=c2; declare -n c2=c1;
…; }` — the cycle made of *locals*, so the reference being declared is in the
same frame as the declaration. At top level every one of these shapes agrees
with osh exactly; it is only the shared frame that breaks bash.

**Array-making operands.** The array-making rule of
TD-OILS-NAMEREF-WARNING-COUNT — a subscript
or `-a`/`-A` drops the nameref attribute and makes the operand's own array —
was scoped to exclude the `-a`/`-A` half when the declaration binds a *local*,
because bash's answers there are not a specification of anything. With the
reference itself local to the frame, bash keeps the nameref attribute *and*
makes the array:

```
$ f() { declare -n c1=c2; declare -n c2=c1; local -a c1; declare -p c1; }; f
declare -an c1=()
```

`declare -an` is impossible — a name is a reference or an array, not both — and
the neighbouring shapes are worse:

| input (inside `f`, after `declare -n c1=c2; declare -n c2=c1`) | bash 5.2.37 | osh |
|---|---|---|
| `local -a c1` | `declare -an c1=()` | `declare -n c1="c2"` |
| `declare -i 'c1[0]'` | `declare -ain c1=()` | `declare -ai c1` |
| `local 'c1[0]'=9` | `` local: `9': invalid variable name for name reference ``, then `declare -an c1=()` | `declare -a c1=([0]="9")` |
| `local -a c1=x` | `` local: `????????': not a valid identifier `` — the quoted bytes are **uninitialised memory**, varying run to run and not valid UTF-8 — then `declare -an c1=()` | `declare -n c1="c2"` |

The last is decisive: a diagnostic that prints indeterminate bytes is a bug, not
behaviour to match. osh keeps the subscript half of the rule here — the array is
made, and made out of the operand's own name, which is the sane reading of what
bash was reaching for — and declines the `-a`/`-A` half, which would otherwise
have to produce the impossible `declare -an`.

**Where.** `userspace\oils\src\interp.rs`, `Shell::builtin_declare_scoped`, the
`makes_array` predicate: `subscript.is_some() || (!make_local && (indexed ||
assoc))`. The `!make_local` guard is the exclusion, and mirrors the one already
on `unreference`.

**The counts here are not matched either, and deliberately.** Where the operand
is dropped osh happens to agree (`local -a c1`: two walks each), but where it
makes something osh stops at one walk and bash takes two — bash is walking the
chain again on its way into the state above, so reproducing the count would mean
reproducing the state. Separately, `local -a c1` in a frame that merely
*shadows* a global circular nameref walks twice in bash and not at all in osh;
the result matches, only the count differs.

**Taking the letter *off* in the same frame is confused the same way.** These
are not array-making at all, and osh asks the chain nothing — the operand names
the reference itself, which is right here — while bash walks it anyway:

| input (inside `f`) | bash 5.2.37 | osh |
|---|---|---|
| `declare +n c1` | 2 walks, `declare -- c1="c2"` | 0 walks, same result |
| `declare -x +n c1` | 2 walks, `declare -x c1="c2"` | 0 walks, same result |
| `declare -n c1` (re-declaring) | 3 walks, unchanged | 0 walks, same result |
| `declare +n c1=5` | 2 walks, then `` declare: `5': not a valid identifier ``, leaving `declare -n c1="c2"` | 0 walks, `declare -- c1="5"` |
| `unset -n c1` | 0 walks, `declare -- c1` | identical |

`declare +n c1=5` is the tell: bash has quoted the **value** as though it were a
name, the same confusion as `` local: `9': invalid variable name for name
reference `` above. At top level the same line on the same cycle warns twice and
quietly leaves the reference alone in *both* shells, so the frame is the whole
of the difference. osh takes the letter off and stores, which is what `+n`
means.

The 0 walks are not an oversight but the same `!make_local` scoping: `follow` in
`builtin_declare_scoped` is off for a `+n` operand binding a local, because
`local -n r=w; declare -x +n r` must export `r` and leave `w` alone — a rule
verified against bash. bash resolves the chain anyway and throws the answer
away, which is where its extra warnings come from; asking osh to walk a chain it
has already decided not to follow, only in the frame where bash is demonstrably
confused, would be copying the artifact rather than the rule.

**No corpus case.** Output containing uninitialised bytes cannot be diffed.
