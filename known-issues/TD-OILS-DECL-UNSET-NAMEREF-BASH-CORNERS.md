### TD-OILS-DECL-UNSET-NAMEREF-BASH-CORNERS. two `declare +n` corners where bash contradicts itself — 2026-07-31 — OPEN (WONTFIX candidate)

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare_scoped`, the
`nameref_off_name` / `unset_nameref` handling added for
TD-OILS-DECL-UNSET-NAMEREF-DOES-NOT-FOLLOW above.

Two shapes left divergent on purpose, because bash's own answers there do
not follow from any rule the rest of `+n` obeys. Probes:
`target/dvscratch/px76.sh`, `target/dvscratch/px77.sh`.

**1. `+n` with another letter, through a reference onto an array
*element*, with a subscript of its own.** bash refuses the name it builds
and *still* takes the attribute off — where every other refusal (readonly
assignment, `-a`/`-A` conversion, `noassign`) leaves the reference alone:

```sh
declare -a a=(1 2); declare -n r='a[1]'
declare -x +n 'r[1]'   # bash declare: `a[1][1]': not a valid identifier, rc=1
                       #      …and declare -- r="a[1]" — attribute gone
                       # osh  same refusal and rc, but declare -n r="a[1]"
```

osh puts the removal with the other attribute applications, after the
refusal's `continue`, which is what makes the *other* five refusals come
out right. Matching this one case would mean a special-case removal
inside that single refusal branch. Not worth it.

**2. `+n` on a *local* reference that also names an array or a
subscript.** These make bash emit uninitialised memory in the diagnostic
and produce an impossible variable, so there is no behaviour to match:

```sh
f() { local w=5; local -n r=w; declare +n 'r[1]'; declare -p r; }
f                      # bash declare -a r=()          ← no diagnostic at all
                       # osh  declare -- r="w"
g() { local w=5; local -n r=w; declare +n 'r[1]=9'; declare -p r; }
g                      # bash declare: `<garbage bytes>': not a valid identifier
                       #      declare -an r=()     ← array *and* nameref
                       # osh  declare -a r=([1]="9")
t() { local -n g=z; local -n z=g; local +n -a g=5; declare -p g; }
t                      # bash local: `<garbage bytes>': not a valid identifier
                       #      st=0, declare -a g=()
                       # osh  st=0, declare -a g=([0]="5")
u() { local -n g=z; local -n z=g; local +n g[1]=5; declare -p g; }
u                      # bash local: `<garbage bytes>': not a valid identifier
                       #      st=1, declare -an g=()
                       # osh  st=0, declare -a g=([1]="5")
```

`t` and `u` are the array spellings of the store refusal that
TD-OILS-A-PLUS-N-DECLARATION-STORES-THROUGH-THE-REFERENCE-IT-IS-REMOVING
fixed, and they are held out of it for this reason: bash is refusing a
name it *built* from the operand, and on the array path the buffer it
hands `valid_nameref_value` was never filled — so the quoted text is
whatever was on the stack, and even the status disagrees between the two
(`t` 0, `u` 1) for nothing visible in the source.

The scalar spellings beside them are ordinary and now match: `local +n
g=5` through a cycle, and the case once listed third here —
`f() { local w=5; local -n r=w; declare +n r=9; }`, which bash answers
`` `9': not a valid identifier `` and `declare -n r="w"`, abandoning the
operand entirely. That one is no longer divergent; it is the plain shape
of the store refusal, which osh now makes.

**Proper fix if ever wanted:** case 1 only, by moving the `+n` removal
ahead of the `t.sub`/`osub` refusal. Leave case 2 alone; matching a
use-of-uninitialised-memory diagnostic is not a goal.
