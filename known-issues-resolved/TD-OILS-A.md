### TD-OILS-A--G-COMPOUND-THROUGH-A-REFERENCE-TO-AN-ELEMENT-BINDS-THE-SPELLING-TOO. `f() { declare -g r=(x y); }` with a global `declare -n r='n[1]'` makes a variable named `n[1]`; osh refuses it — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::declare_compounds_scoped`,
the `spelled` computation (which asks `make_local`) and the `-A` kind check.

**What.** The rule
TD-OILS-A-DECLARATION-BUILTIN-THAT-BINDS-A-LOCAL-THROUGH-A-REFERENCE-TO-AN-ELEMENT-MAKES-A-LOCAL-NAMED-BY-THE-SPELLING
fixed for a *local* binding is not about locals at all on the compound path: it
is about being inside a function. `-g` does not switch it off — it only changes
*which binding of the spelling* the literal lands in. With a global
`declare -n r='n[1]'` and a global `n=(a b c)`:

```text
                                              bash                osh
declare -g r=zz     (inside a function)       `n[1]` becomes      agreed
                                              `zz`
declare -g r=(x y)  (inside a function)       `declare -a         `` `n[1]': not a
                                              n[1]=([0]="x"       valid identifier ``
                                              [1]="y")``, global
declare -g r=(x y)  (at top level)            the refusal         agreed
declare r=(x y)     (at top level)            the refusal         agreed
declare -g r=(x y)  (plain ref `r=n`,         follows, `n`        agreed
                     inside a function)       becomes `(x y)`
declare -g r=(x y)  (local ref, no global     plain global        agreed
                     `r`, inside a function)  `r=(x y)`
```

The earlier reading — "nothing at all, s=0, no output" — was the same `set`-style
measurement artifact that misread the local case: `declare -p 'n[1]'` shows the
binding plainly.

Where it lands is bash's ordinary bind rather than a forced global: `-g` from a
deeper frame overwrites a caller's spelling-named local instead of reaching past
it, and only makes a global when no binding of that name is visible. Which
reference is consulted is the usual question of which binding the declaration
writes — `-g` writes the global one, so it follows a *global* `declare -n` and
ignores a local one.

The `-A` kind check is a separate matter and does *not* move to the spelling: it
is the attribute half, and attributes through an element reference have always
landed on the base array. That half is
TD-OILS-AN-ASSOCIATIVE-COMPOUND-THROUGH-A-REFERENCE-TO-AN-ELEMENT-IS-BLAMED-BY-THE-WRONG-RULE,
which now carries the `-g` rows too; it is still open.

**Fixed 2026-08-05.** `Shell::declare_compounds_scoped` passes "inside a
function" (`!local_frames.is_empty()`) rather than `make_local` to
`Shell::spelled_local_name`; the scalar path keeps `make_local`, since
`declare -g r=zz` still stores the element. The store needed no other change:
`enter_global_scope` swaps only the operand's own name, so a write to the
spelling already hits the live binding, which is bash's `bind_variable`.
Corpus: `a-g-compound-through-a-reference-to-an-element-binds-the-spelling-too.sh`
— the rows that turn on the base array are held back for that entry's fix.
