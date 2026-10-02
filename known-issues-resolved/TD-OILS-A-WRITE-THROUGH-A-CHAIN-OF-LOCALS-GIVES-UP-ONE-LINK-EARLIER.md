### TD-OILS-A-WRITE-THROUGH-A-CHAIN-OF-LOCALS-GIVES-UP-ONE-LINK-EARLIER. bash's write walk descends variable contexts one hash table at a time — FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, `Shell::scalar_write_dest`,
`Shell::apply_assignment_inner` and `Shell::arith_write_dest` — all three
resolved a write target with `Shell::walk_ref_name`, the **read** walk. They now
go through `Shell::resolve_ref_write`, which tries the new
`Shell::walk_ref_name_writing` first and falls back to the read walk when no
local context holds the name (bash's own fallback: `bind_variable`'s loop enters
only `vc_isfuncenv`/`vc_isbltnenv` contexts, so a purely global reference drops
through to `bind_variable_internal` and the ordinary walk).

**The rule.** A read resolves a reference by looking each link up the ordinary
way — every scope at once, innermost binding first (`find_variable_nameref`). A
write does not. `bind_variable` (variables.c:3275) descends the stack of variable
contexts one hash table at a time:

```c
  for (vc = shell_variables; vc; vc = vc->down)
    if (vc_isfuncenv (vc) || vc_isbltnenv (vc))
      {
	v = hash_lookup (name, vc->table);
	nvc = vc;
	if (v && nameref_p (v))
	  { nv = find_variable_nameref_context (v, vc, &nvc); … }
	if (v)
	  return (bind_variable_internal (v->name, value, nvc->table, 0, flags));
      }
```

and `find_nameref_at_context` (variables.c:2172) follows only as much of the
chain as *one* table holds — `nv2 = hash_lookup (newname, vc->table); if (nv2 ==
0) break;`. Three things follow, and none of them is what a read does. The
original entry recorded only the third.

**(a) The write lands in a context the read never names.** The walk stops in
whichever context resolved the chain, and binds there:

```sh
q=GLOBQ
o() { local -n p=q; local q=OUTERQ; i; echo "o=$q"; }
i() { local q=INNERQ; echo "r=$p"; p=W; echo "i=$q"; }
o; echo "g=$q"      # bash: r=INNERQ i=INNERQ o=W g=GLOBQ
```

The read finds the innermost `q`; the write binds the *outer* function's.

**(b) The walk never climbs back up.** A reference in the global table resolves
against the global table, even where a local of that name exists:

```sh
declare -n G1=zz
i1() { local -n a=G1; local zz=INNERZZ; a=X; echo "i=$zz"; }
i1; echo "g=${zz-UNSET}"      # bash: i=INNERZZ g=X
```

**(c) The counter restarts per context, one higher.** `find_nameref_at_context`
sets `level = 1` where `find_variable_nameref` sets it to 0, so one context
follows **seven** links where a read follows eight — and past the cap
`&nameref_maxloop_value` comes back to `bind_variable`, which is the one place a
write says anything at all about giving up (the read walk gives up silently):

```c
  else if (nv == &nameref_maxloop_value)
    {
      internal_warning (_("%s: circular name reference"), v->name);
      return (bind_global_variable (v->name, value, flags));
    }
```

— the value goes to the **global** of the name the walk started from, and the
local reference survives. A corollary of the per-context restart is that a chain
*spread over two contexts* is followed **further** than a read would follow it.

**The fix.** `walk_ref_name_writing` mirrors the C loop: it starts at the
innermost context that holds the name, and for each context from there down runs
an inner walk capped at seven that follows a link only while
`bound_in_context(next, ctx)` — bash's `hash_lookup (name, vc->table) != 0`, the
context's **own** table rather than what is visible from it. That distinction is
the whole of (b), and getting it wrong was the one real trap: osh's scope model
is a flat map plus a stack of `local_frames` holding the *parked* outer binding,
so "context `d`'s own table holds `name`" is "`local_frames[d-1]` has an entry
for it", not "looking up `name` from depth `d` finds something". Past the cap the
walk answers `RefWalk::closed(RefTarget::global_of(name))`, which reproduces the
warning-and-global-bind without a new `RefWalk` variant.

`arith_write_dest` needed a second change: `bind_int_variable` calls
`find_variable(lhs)` for the *kind* (integer attribute, implicit array) and then
`bind_variable(lhs, …)` for the store, so `(( x = … ))` takes a read walk and
then a write walk. It now does exactly that — one `resolve_ref_use_walks(name, 1)`
for the warning, then `resolve_ref_write`.

**Corpus:** `a-write-follows-a-nameref-chain-one-variable-context-at-a-time.sh`,
which also pins down which assignment forms go through `bind_variable` at all
(scalar, `+=`, `read`, `printf -v`, `(( ))`, `${x:=v}`) and which do not (element
write, `read -a`, `for`, compound literal, `unset`, `declare`).

**How it was found:** reading `find_nameref_at_context` (variables.c:2173) while
fixing TD-OILS-A-NAMEREF-CHAIN-IS-FOLLOWED-PAST-THE-DEPTH-BASH-GIVES-UP-AT, and
confirming the seven-link difference by measurement. (a) and (b) turned up only
once the walk was written and measured against bash form by form.
