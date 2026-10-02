### TD-OILS-ARITH-EMPTY-PARAM-BECOMES-ZERO. an empty parameter leaves a `0` behind in the expression — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_arith_params`, whose
substitution branches ended
`out.extend_from_slice(if val.is_empty() { b"0" } else { val })`.

**What.** bash splices a parameter's *value* into the arithmetic text and leaves
an empty one empty; osh wrote a `0` in its place. An expansion is textual, so
the `0` is not an invisible identity — it is a token where bash has none, and
the characters on either side of the splice no longer close up:

```sh
echo $(( 1 + $novar ))    # bash: syntax error: operand expected   osh: 1
echo $(( -$novar ))       # bash: syntax error: operand expected   osh: 0
echo $(( 1 + $novar 2 ))  # bash: 3                                osh: syntax error
echo $(( 1 $novar 2 ))    # bash blames "1  2"      osh blames "1 0 2"
echo $(( 1$novar 2 ))     # bash blames "1 2"       osh blames "10 2"
```

The entry as first written claimed the impact was "error messages only, and only
for an expression that was already going to fail". That was wrong, and the
measurement above is why: three of the five rows differ in the *answer*, in both
directions — osh silently evaluated two expressions bash refuses, and refused
one bash evaluates. The reason the bug looked cosmetic is that the shapes people
actually write agree by coincidence: an expression that is empty all through is
0 either way, because an empty arithmetic expression is already 0.

**Fixed.** The three branches that splice a parameter's text — the nested
`$((…))` branch, the `${…}` branch and the bare-`$name` branch — now end
`out.extend_from_slice(bytes::trim(&val))` with no empty-to-`0` rewrite. The
`$(…)`/backtick branches already spliced raw and were left alone. Nothing was
needed on the evaluator side: `$(( ))` was already 0, so the guard was buying a
property the evaluator supplied anyway.

A nested `$(( … ))` keeps its number because it is *evaluated* rather than
spliced — `$(( 1 $(( )) 2 ))` really is `1 0 2` in bash too — so the trim there
is only stripping the evaluator's own formatting, not a value.

Corpus:
`an-empty-parameter-splices-into-arithmetic-as-nothing-not-as-zero.sh`, which
covers the splice joining with its neighbours (`a$novar`, `${novar}1`,
`1${novar}2`, an operator held in a variable), the all-empty expressions that
stay 0, the command-substitution branches, and the other arithmetic contexts
(`(( … ))`, a subscript, a `for` header, `let`, `declare -i`, a slice).
