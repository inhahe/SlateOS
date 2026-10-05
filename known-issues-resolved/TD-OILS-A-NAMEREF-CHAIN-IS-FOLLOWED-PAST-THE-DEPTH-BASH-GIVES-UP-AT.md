### TD-OILS-A-NAMEREF-CHAIN-IS-FOLLOWED-PAST-THE-DEPTH-BASH-GIVES-UP-AT. bash follows 8 links and then quits, silently — FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, `Shell::resolve_ref_name` — the loop
walked the chain to its end, keeping a `seen` list and reporting a cycle whenever
a name repeated. bash's `find_variable_nameref` (variables.c:2074) is a different
loop:

```c
  level = 0;
  orig = v;
  while (v && nameref_p (v))
    {
      level++;
      if (level > NAMEREF_MAX)
	return ((SHELL_VAR *)0);	/* error message here? */
      newname = nameref_cell (v);
      if (newname == 0 || *newname == '\0')
	return ((SHELL_VAR *)0);
      oldv = v;
      ...
      v = find_variable_internal (newname, flags);
      if (v == orig || v == oldv)
	{
	  internal_warning (_("%s: circular name reference"), orig->name);
	  if (variable_context && v->context)
	    return (find_global_variable_noref (v->name));
	  else
	    return ((SHELL_VAR *)0);
	}
    }
```

`NAMEREF_MAX` is 8 (variables.h:172). Two things follow, and osh gets both wrong:

**(a) The depth cap.** A chain of more than eight links resolves to *nothing*,
with no diagnostic at all — it is not a cycle, just too long. Measured:

```sh
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do eval "declare -n n$i=n$((i+1))"; done
n13=DEEP
echo "[${n5}]"   # bash: [DEEP]   osh: [DEEP]     (8 links)
echo "[${n4}]"   # bash: []       osh: [DEEP]     (9 links)
echo "[${n1}]"   # bash: []       osh: [DEEP]     (12 links)
```

**(b) Which cycles warn.** bash notices a cycle only when the walk lands back on
the variable it *started* from (`v == orig`) or on the one it has just left
(`v == oldv`, i.e. `declare -n x=x`). A cycle that closes further along is never
recognised as one: bash simply spins until the depth cap and gives up quietly.
osh's `seen` list reports every repeat. Measured:

```sh
declare -n a1=a2; declare -n a2=a3; declare -n a3=a2
echo "${a1}"     # bash: empty, silent
                 # osh : empty, plus "warning: a1: circular name reference"
a1=X             # bash: silent, nothing stored — and the rest of the command
                 #       list is abandoned, exactly as a store through a cycle
                 #       bash *does* recognise is
                 # osh : stores through to the end of the chain, and warns
unset a1         # bash: silent; a1 unset, a2/a3 intact — and osh agrees on the
                 #       effect, so only the diagnostic differs
```

The abandoned command list is the sharpest of the three, because it changes what
runs. Measured with `exec 2>&1`:

```sh
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do eval "declare -n n$i=n$((i+1))"; done
n13=DEEP
n4=X; echo "st=$?"; declare -p n13     # bash: prints nothing at all — the `echo`
                                       #       and the `declare` never run
n5=Y; echo "st=$?"; declare -p n13     # bash: st=0, declare -- n13="Y"
```

Note that the *escape* rule in (b) is keyed to the same test: only a cycle bash
recognises can escape to global scope, so `a1` above does not escape either. osh
already models the escape (TD-OILS-A-NAMEREF-CYCLE-THAT-CLOSES-ON-A-LOCAL-…), but
it applies it to cycles bash would not have noticed.

**Fixed** in `Shell::walk_ref_name`, which is now bash's loop link for link: a
`level` counter capped at `NAMEREF_MAX` = 8, and a cycle test comparing the name
just reached against the name the walk started from and against the name it came
from. The answer is a `RefWalk` — what the chain named, plus whether it closed on
itself — because the two ways of reaching nothing are not equally loud, and only
the second says anything. `resolve_ref_name` is a one-line wrapper handing back
the target, so its twenty-odd callers were untouched; `warn_circular_walks` and
`resolve_ref_write_walks` take the flag instead of inferring it from a `None`,
and the `for`-loop control variable asks the walk directly (`is_circular_ref` is
gone). `last_nameref_of` — bash's `find_variable_last_nameref`, which `declare
+n` walks — counts its links the same way and gives up at the same eight, so it
returns `Option<String>` now.

Everything downstream of "the chain named nothing" was already right, which is
why the fix is confined to the walk: a store through a too-deep chain fails and
abandons the command list exactly as one through a cycle does, an element store
falls back on the reference's own name and takes the attribute off it, and
`for n4 in p q` reports 1 without running a body. Only the diagnostic differed.

Corpus: `a-nameref-chain-is-followed-eight-links-and-no-further.sh`. Unit test:
`a_nameref_chain_is_followed_eight_links_and_no_further`.

**How it was found:** while modelling `unset name[sub]`'s two walks
(`unset-removes-an-element-through-the-variable-the-walk-found.sh`), probing what
`find_variable_last_nameref` does with a cycle that does not include the operand.
