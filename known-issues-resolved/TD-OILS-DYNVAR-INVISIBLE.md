### TD-OILS-DYNVAR-INVISIBLE. Reading a dynamic special variable does not make it visible to the listings — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `Shell::dyn_declared` and
`dynamic_special_value` / `param_value`.

bash's `att_invisible` on the dynamic special variables is cleared by
*any* touch, and a plain read is one. TD-OILS-DYNVAR-DECLARED implemented
the declaration half; the read half is still missing. Measured against
bash 5.2.37, with `p` = `declare -p SECONDS`, `i` = `declare -i` and
`set` = a bare `set`, values masked:

```
pristine            p=[declare -i SECONDS="N"]  i=[]                        set=[]
read (: $SECONDS)   p=[declare -i SECONDS="N"]  i=[declare -i SECONDS="N"]  set=[SECONDS=0]
SECONDS=5           p=[declare -i SECONDS="N"]  i=[]                        set=[SECONDS=5]
assign, then read   p=[declare -i SECONDS="N"]  i=[declare -i SECONDS="N"]  set=[SECONDS=5]
declare SECONDS     p=[declare -i SECONDS="N"]  i=[declare -i SECONDS="N"]  set=[SECONDS=1]
export SECONDS      p=[declare -ix SECONDS="N"] i=[declare -ix SECONDS="N"] set=[SECONDS=1]
RANDOM pristine     p=[declare -i RANDOM="N"]   i=[declare -i RANDOM]
RANDOM read         p=[declare -i RANDOM="N"]   i=[declare -i RANDOM="N"]
RANDOM=5            p=[declare -i RANDOM="N"]   i=[declare -i RANDOM="N"]
```

So a read promotes for both listings; a *scalar assignment* promotes for
`set` but not for `declare -i` on `SECONDS`, while on `RANDOM` it
promotes for both — the difference being that `SECONDS`'s assignment hook
rebases a counter without storing, where `RANDOM`'s reseeds and bash
marks the variable set either way.

**The proper fix.** `dyn_declared` is populated from `&mut self` paths
only. A read goes through `param_value`, which is `&self`, so promotion
on read needs interior mutability — a `Cell<u16>` bitmask over the
`DYNAMIC_SPECIALS` rows would do it without allocating, and
`Shell::rng` already establishes the pattern (`Cell` on the `&self` read
path). Do that rather than making `param_value` take `&mut self`: it is
called from expansion paths that hold other borrows.

**Fixed 2026-07-31**, by the `Cell` route above, and with the *concept*
corrected along the way. Measuring the pristine state of all thirteen
names showed there is no invisibility to model at all: a bare
`declare -p` lists an untouched `SECONDS` all along, as
`declare -- SECONDS` — no value, and not even the `-i`. What bash has is
a slot whose **value cell and attributes the value function fills in**,
on every *lookup*. So `declare -i` passes over a pristine `SECONDS`
because it is not an integer yet, and a bare `set` passes over it because
it has no value yet, and one read fixes both at once:

```
                declare -p             declare -i             set
pristine        declare -- SECONDS     —                      —
after a read    declare -i SECONDS=N   declare -i SECONDS=N   SECONDS=N
```

`dyn_declared: HashSet<String>` became `dyn_touched: Cell<u32>`, a
bitmask set by `dynamic_special_value` — so an expansion, a `${x-y}`
default, `(( x ))`, `[ -v x ]`, `declare -p NAME` and the declaration
builtins all fill the slot in, while `${!prefix@}`, which asks for names
rather than values, does not. `unset` clears the bit with the rest of the
binding. A bare `set` now also lists the names that were valued from the
start (`PPID`), which osh was omitting. Corpus case
`dynamic-var-visible.sh`; unit test
`looking_a_dynamic_variable_up_fills_its_slot_in`.

The assignment half followed in TD-OILS-DYNVAR-ASSIGN-CELL, also fixed.
