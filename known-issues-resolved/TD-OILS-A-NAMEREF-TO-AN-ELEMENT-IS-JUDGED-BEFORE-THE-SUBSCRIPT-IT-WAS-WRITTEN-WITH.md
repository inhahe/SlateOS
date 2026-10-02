### TD-OILS-A-NAMEREF-TO-AN-ELEMENT-IS-JUDGED-BEFORE-THE-SUBSCRIPT-IT-WAS-WRITTEN-WITH. `declare -n r='n[1]'; r[-1]=9` says `not a valid identifier` where bash says `bad array subscript` — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::apply_assignment_inner` and
the nameref resolution beneath it. An element write through a reference whose
*target* carries a subscript is refused for the target's spelling before the
write's **own** subscript is looked at, and the chain is walked one time too
many.

```sh
$ declare -n r='n[1]'; n=(a b c); r[-1]=9
bash: r[-1]: bad array subscript
osh : `n[1]': not a valid identifier

$ declare -n r='n[1]'; n=(a b c); r[1+]=9
bash: 1+: syntax error: operand expected (error token is "+")
osh : `n[1]': not a valid identifier

$ declare -n a=b; declare -n b=a; a[]=9
bash: a[]: bad array subscript                  # no warning at all
osh : warning: a: circular name reference       # …twice, then the refusal
      warning: a: circular name reference
      a[]: bad array subscript
```

bash's order for `X[sub]=v` is fixed by where each test sits:

1. **`array_variable_name` (arrayfunc.c:1413)** validates the *bracket
   structure* — `if (ni <= ind + 1 || s[ni] != ']') { err_badarraysub (s); …
   return NULL; }` — and it runs **before** `find_variable`. That is why
   `a[]=9` on a circular chain costs no lookup and so warns not at all.
2. **`find_variable (vname)`** — exactly one nameref walk, hence exactly one
   circular warning.
3. **the `[*]`/`[@]` refusal** (`ALL_ELEMENT_SUB (sub[0]) && sub[1] == ']'`,
   arrayfunc.c:370) — after the walk, so one warning precedes it.
4. **`array_expand_index`** — arithmetic, so `1+` is a syntax error here.
5. **the negative-underflow refusal** (arrayfunc.c:437).
6. **only then** `bind_array_variable` → `find_variable_nameref_for_create`
   → `legal_identifier (nameref_cell (var)) == 0` → `sh_invalidid`
   (variables.c:2276). `legal_identifier` rejects `n[1]`, where the
   `valid_nameref_value` used when the nameref was *created* accepts it —
   which is why the name can be stored and then refused at use.

Non-negative subscripts already agree (`r[0]=9` and `r[5]=9` both give
`` `n[1]': not a valid identifier `` in both shells), so this is the ordering
alone: every case where some *earlier* test should have fired first.

Every other caller that writes an element by *name* is wrong the same way,
and osh's is tagged where bash's is not, because bash never reaches the
builtin's own complaint:

```sh
$ declare -n r='n[1]'; n=(a b c); read 'r[-1]' <<< x
bash: r[-1]: bad array subscript
osh : read: `n[1]': not a valid identifier
```

— likewise `printf -v 'r[-1]'` and `${r[-1]:=v}`. `read 'r[]'` and
`printf -v 'r[]'` already agree, both shells refusing the *word* before
anything looks at `r`.

The walk counts a circular chain earns are the other half of the same
ordering, and are uniform once the declaration's own two warnings are
discounted (`local -n g=g` warns twice before the function body starts, which
is what made the escaped-cycle rows look different): **0** walks when the
bracket structure is bad, **1** when the subscript is refused, and the full
count — 2 indexed, 1 associative — when the store goes through. The reference
survives every refusal and is unmade only by the store that succeeds, which
is the second walk's doing (`find_variable_nameref_for_create` →
`make_new_array_variable (nameref_cell (entry))`, arrayfunc.c:266):

```sh
$ declare -n a=b; declare -n b=a; trap 'declare -p a b' EXIT; a[-1]=9
bash: warning: a: circular name reference          # once
      a[-1]: bad array subscript
      declare -n a="b"                             # …and still a reference
$ … a[1]=9
bash: warning: a: circular name reference          # twice
      warning: a: circular name reference
      declare -a a=([1]="9")                       # …and no longer one
```

osh warns [`Shell::resolve_ref_elem_write`]'s full count up front, before it
knows which of the three the write is.

**Fixed** in this commit, in three parts — the filed root cause was three
faults wearing one shirt.

* **The walk's cost was paid up front.** `set_scalar_target_checked` called
  the eager resolver, so the warnings and [`Shell::break_circular_ref`] both
  landed before the write knew which of the six steps it would reach. It now
  resolves with [`Shell::resolve_ref_elem_write_deferred`] and carries the
  unpaid cost on the new `ScalarDest::debt` down to
  [`Shell::assign_elem_owing`], which spends it at bash's two lookup points.
  0 walks now fall below the bracket test, 1 below the subscript's own
  refusals, and the full count only at the bind — which is also the only
  place a circular reference is unmade, exactly as
  `find_variable_nameref_for_create` → `make_new_array_variable
  (nameref_cell (entry))` (arrayfunc.c:266) has it.
* **The readonly refusal was above the subscript.** For an *element*
  destination it ran in [`Shell::scalar_write_guarded`], before anything had
  looked at the brackets. It is now [`Shell::refuse_elem_store`], called from
  the bind, where arrayfunc.c:273 puts it — and it refuses `noassign` in the
  same silent breath bash does.
* **The store's name and the blame's name were one name.** They are now two:
  `name` goes where the chain points, `blame` quotes the operand as
  **written**, which is the only thing bash keeps
  `assign_array_element_internal`'s `char *name; /* only used for error
  messages */` (arrayfunc.c:394) for. All four spellings of `bad array
  subscript`, the `[*]`/`[@]` refusal and the readonly one now use it.

`Shell::resolve_ref_elem_write` had no callers left and was deleted.

Corpus:
`a-nameref-to-an-element-is-judged-before-the-subscript-it-was-written-with.sh`.
Unit test:
`a_nameref_to_an_element_is_judged_before_the_subscript_it_was_written_with`.

**How it was found:** probing the spelling of element-write diagnostics
through a nameref, for
TD-OILS-A-DECLARATION-OPERANDS-NAMEREF-IS-FOLLOWED-AT-THE-WRONG-END-INSIDE-A-FUNCTION.
