### TD-OILS-AN-ELEMENT-WRITE-TO-A-MAINTAINED-NAME-SKIPS-ITS-SUBSCRIPTS-OWN-REFUSALS. `GROUPS[-9]=9` was silently discarded where bash calls it a bad subscript — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, the `noassign` short-circuit at the
head of the scalar arm of an assignment statement. It had no
`a.index.is_none()` guard, so an *element* write to a name the shell maintains
returned before the subscript was so much as looked at.

```sh
$ ( GROUPS[-9]=9; echo "st=$?" )
bash: GROUPS[-9]: bad array subscript
osh : st=0

$ ( i=0; GROUPS[i++]=9; echo "i=$i" )
bash: i=1
osh : i=0

$ ( BASH_LINENO[-1]=9 )   # and the same for FUNCNAME, BASH_SOURCE, BASH_ARGV
```

**Why bash does it.** The `att_noassign` refusal is `bind_array_variable`'s
(arrayfunc.c:280), not the assignment's:

```c
  else if ((readonly_p (entry) && (flags&ASS_FORCE) == 0) || noassign_p (entry))
    { if (readonly_p (entry)) err_readonly (name); return (entry); }
  else if (array_p (entry) == 0)
    entry = convert_var_to_array (entry);
```

It sits *below* every test the subscript owes and *above* the widening. So an
element write reaches its own complaints first — the negative subscript,
`GROUPS[]`, `GROUPS[*]`, a malformed arithmetic subscript — and its side
effects happen either way. And because the refusal is above
`convert_var_to_array`, a refused element leaves no array entry behind for a
name that had none.

The refusal itself is silent and is not an error: `return (entry)` is non-NULL,
so the write simply does not happen. The status an assignment-only command
answers with still moves, a refusal being one more thing that sets it —
`FUNCNAME[0]=$(exit 3)` is 1 where the same on an ordinary array is 3.

The sibling writers (`read`, `printf -v`, `(( … ))`) reach that same line
through [`Shell::refuse_elem_store`] and already had it right; a declaration
builtin refuses the whole name earlier and louder ("variable may not be
assigned value").

**Fixed** in this commit. The head-of-arm refusal is now subscript-less, like
the readonly guard beside it, and the two element arms call the new
[`Shell::refused_maintained_elem`] where bash's `bind_array_variable` does —
below the subscript's judgements, below the readonly check, above
[`Shell::array_kind_apply`].

Corpus: `an-element-write-to-a-maintained-name-still-answers-for-its-subscript.sh`.
Unit test:
`an_element_write_to_a_maintained_name_still_answers_for_its_subscript`.

**How it was found:** measuring the bound a negative subscript counts back
from, for
TD-OILS-A-READONLY-DECLARATION-MARKS-BEFORE-IT-STORES-SO-IT-REFUSES-ITS-OWN-ELEMENT.
Filed then as a call-stack-array bug; it turned out to be every `att_noassign`
name, `GROUPS` included, and to be about the assignment statement rather than
the `DynAssign::Discard` table row.
