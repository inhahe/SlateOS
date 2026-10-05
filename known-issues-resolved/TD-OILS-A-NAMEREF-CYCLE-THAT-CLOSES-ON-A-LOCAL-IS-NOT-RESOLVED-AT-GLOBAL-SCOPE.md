### TD-OILS-A-NAMEREF-CYCLE-THAT-CLOSES-ON-A-LOCAL-IS-NOT-RESOLVED-AT-GLOBAL-SCOPE. `local -n r=r` reads the string `r` where bash reads the global `r` — FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, `Shell::resolve_ref_name` — the arm
that deliberately exempts a self-naming reference from the cycle test:

```rust
// A nameref naming *itself* is not a cycle: inside a function
// `local -n r=r` legitimately means the caller's `r`, and bash
// rejects the global form at declaration time instead.
Some(v) if !v.is_empty() && v != cur.as_bytes() => {
```

The comment is right that the shape is legitimate and wrong about what it
means. bash *does* treat it as a cycle — and then resolves it specially, at
**global** scope with namerefs not followed. osh does neither: it falls out of
the walk with `RefTarget::plain("r")`, so the reference reads and writes its own
cell, and none of the resolution-time warnings are emitted.

**Reproduce.**

```sh
r=GLOBALR
h() { local -n r=r; r=W; declare -p r; printf 'read=<%s>\n' "$r"; }
h
echo "after: global r=<$r>"; declare -p r
```

bash 5.2.37:

```
line 2: local: warning: r: circular name reference
line 2: warning: r: circular name reference
line 2: warning: r: circular name reference
declare -n r="r"
line 2: warning: r: circular name reference
read=<W>
after: global r=<W>
declare -- r="W"
```

osh:

```
line 2: local: warning: r: circular name reference
declare -n r="W"
read=<>
after: global r=<GLOBALR>
declare -- r="GLOBALR"
```

Three separate departures, in order of how much they cost:

1. **The write lands in the reference's own cell.** osh's `r=W` overwrites the
   nameref's value, so the variable stops pointing at anything meaningful
   (`declare -n r="W"`) while the global it should have written keeps its old
   value. bash binds the *global* `r` and leaves the local a reference
   (`declare -n r="r"`). With no global of that name at all bash creates one —
   `g() { local -n q=q; q=W2; }; g; declare -p q` prints `declare -- q="W2"`,
   where osh says `q: not found`.
2. **A read yields the reference's own value as a string.** `$r` is `r` in osh
   and `GLOBALR` in bash. Note the resolution really is *global*, not "the
   caller's": with `outer() { local r=OUTER; inner; }` and `inner() { local -n
   r=r; echo "$r"; }`, bash prints `GLOBALR`, not `OUTER`.
3. **The resolution-time warnings are missing.** bash warns once per walk, the
   same way it does for a mutual cycle — which osh already implements for
   `a→b→a` in `resolve_ref_use_walks` / `warn_circular_ref`. It also warns
   *twice* at declaration where osh warns once: `declare.def`'s `check_selfref`
   (prefixed `local:`/`declare:`) and then `bind_variable_internal`'s, which
   repeats the test as it binds (variables.c:3382).

**In bash's own words.** `find_variable_nameref` closes the walk on `oldv`, not
only on the name it started from, which is exactly the self-naming case
(variables.c:2098):

```c
      v = find_variable_internal (newname, flags);
      if (v == orig || v == oldv)
	{
	  internal_warning (_("%s: circular name reference"), orig->name);
#if 1
	  /* XXX - provisional change - circular refs go to
	     global scope for resolution, without namerefs. */
	  if (variable_context && v->context)
	    return (find_global_variable_noref (v->name));
	  else
#endif
	  return ((SHELL_VAR *)0);
	}
```

`variable_context && v->context` is what makes this a *function-local*
provision: a self-naming reference at global scope has nowhere to escape to and
resolves to nothing — which is the answer both shells already agree on for
`${!r}` (`r: invalid indirect expansion`), since an indirection asks through
`find_variable_last_nameref`, which has no such provision.

**Proper fix.** Close `resolve_ref_name`'s cycle test over the self-naming case
too — `seen` already catches `a→b→a`, so this is one arm to delete — and give
the resolver the escape bash has: when the walk closes and the reference is
*local*, resolve the name once more in the global table **without following
namerefs** and hand that back; when it is global, keep answering `None`. The
write side (`resolve_ref_write_walks`) must then stop calling
`break_circular_ref` for this shape, since bash keeps the reference intact and
binds the global instead. The warning counts fall out of the existing per-walk
machinery once the case reaches it, plus one more `check_selfref` at bind time
in the declaration builtins (`interp.rs`'s existing site is only the first of
bash's two).

**Found by** the corpus case `a-reference-with-no-cell-points-nowhere.sh`, which
originally carried a `local -n r=r` row. The row was dropped from that case —
its subject is a reference with an *empty* cell, which this is not — rather
than encoding the wrong answer; it belongs in a case of its own once this is
fixed.

**Fixed 2026-08-10.** The cycle test now closes over the self-naming shape, and
a walk that closes on a *function-local* variable re-resolves the name in the
global table with namerefs not followed — bash's `find_global_variable_noref`
escape — while one that closes on a global still answers nothing. Reads, writes,
`declare -p`, `[ -v ]`, arithmetic and element access all go through the escape;
a write binds the global (creating it if absent) and leaves the reference
intact.

Measuring the warning counts that fall out of it turned up a second, larger
rule that had to be implemented for this to match byte-for-byte: bash asks after
a variable *again* after reading its value, and how many times it walks a
nameref chain doing so depends on how the parameter is spelled and on what the
name reached. That is `get_var_and_type` (subst.c), now modelled by
`Shell::var_and_type_walks` / `VarForm` / `ResolvedKind`: an element subscript
costs 2, `[@]`/`[*]` costs 1 where the variable exists and 2 where it does not,
and a bare name costs 1 plus one more where the value is set and the name did
not reach an array, plus one more again for `@a`/`@A` on a name that reached
nothing.

`export`/`readonly` mark the global the escape names, at the cost bash's
`set_var_attribute` pays for it (`Shell::attr_ref_walks`), and an unsubscripted
arithmetic operand asks `set -u` about that global rather than about the
reference standing in front of it, sharing bash's single `find_variable`
between the check and the read (`Shell::arith_unbound_name`). The residual gaps
this measurement left are logged separately as
`TD-OILS-REDECLARING-A-SELF-NAMING-NAMEREF-WARNS-TOO-FEW-TIMES`,
`TD-OILS-A-SUBSCRIPTED-ARITHMETIC-OPERAND-DOES-NOT-FOLLOW-A-NAMEREF-CYCLES-ESCAPE`
and `TD-OILS-READ-A-AND-MAPFILE-DO-NOT-TAKE-A-NAMEREF-CYCLES-GLOBAL-ESCAPE`.

**Corpus:** `a-nameref-cycle-that-closes-on-a-local-resolves-at-global-scope.sh`
and `export-and-readonly-mark-the-global-a-nameref-cycle-escaped-to.sh`.
