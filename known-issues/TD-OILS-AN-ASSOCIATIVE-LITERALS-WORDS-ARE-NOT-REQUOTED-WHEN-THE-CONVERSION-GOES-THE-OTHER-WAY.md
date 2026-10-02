### TD-OILS-AN-ASSOCIATIVE-LITERALS-WORDS-ARE-NOT-REQUOTED-WHEN-THE-CONVERSION-GOES-THE-OTHER-WAY. bash expands a compound literal in the shape of the variable it *found* and assigns it in the shape it converted to — 2026-08-14 — ⚠️ OPEN

**Where:** `userspace/oils/src/interp.rs`, [`Shell::declare_compounds_scoped`],
which applies the array kind and only then expands the literal. bash does it the
other way round, and the seam shows.

`do_compound_assignment`'s scoped branches call `expand_compound_array_assignment
(v, value, flags)` with the variable **as found**, convert it, and only then
`assign_compound_array_list`. Where `v` is associative the expansion runs
`quote_compound_array_list`, which single-quotes each word's subscript and value
separately so a `[k]=v` survives re-parsing. If the command asked for `-a`, those
already-quoted words are then assigned to an *indexed* array, quotes and all:

```sh
declare -n g=z                                        # so step 1 goes to `z`
f() { local -A g=([j]=9); readonly -a g=(x "a b"); declare -p g; }; f
# bash: declare -ar g=([0]="'x'" [1]="'a b'")
# osh : declare -ar g=([0]="x"   [1]="a b")
```

and a subscript in the literal becomes an arithmetic error, the indexed
assignment being handed `['5']=`:

```sh
declare -n g=z
f() { local -A g=([j]=9); readonly -a g=([5]=q w); }; f
# bash: '5': syntax error: operand expected (error token is "'5'")
# osh : (assigns element 5)
```

The reverse direction is clean, `quote_compound_array_list` being called only for
an associative `v`: an indexed local converted to associative by `readonly -A`
matches already.

**Proper fix.** Split the compound literal's *expansion shape* from its
*assignment shape*: expand in the kind the target held when the operand was
reached, requote the resulting words when that kind was associative and the
command asked for the other one, then convert and assign the requoted words in
the asked kind.

**Why it is not done yet.** The road is only reachable through a `declare -n`
restart that parts step 1's name from the literal's, over a frame-local
associative array, with `-a` and a globally-scoped assignment builtin — and what
it reproduces is a self-evident bash bug (values that grow quotes, subscripts
that become arithmetic errors). Inverting the expand/convert order in
`declare_compounds_scoped` touches every compound operand of every declaration
builtin, which is a great deal of risk for four shapes of the 240-shape kind
matrix. The substantive half — *which* variable is refused, and that the
conversion happens at all — is fixed above.

**How it was found:** fixing
TD-OILS-A-COMPOUND-LITERAL-REFUSES-A-KIND-CHANGE-THE-BUILTIN-BEFORE-IT-HAS-ALREADY-MADE
opened the conversion road that the old refusal had kept shut.
