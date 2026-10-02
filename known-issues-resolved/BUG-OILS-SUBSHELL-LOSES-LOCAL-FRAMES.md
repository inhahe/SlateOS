### BUG-OILS-SUBSHELL-LOSES-LOCAL-FRAMES. `local` inside a subshell or command substitution of a function is refused — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `clone_for_subshell` set
`local_frames: Vec::new()`, on the reasoning that "a subshell body is not
itself a function frame". Every `local` reached from inside a subshell,
command substitution or pipeline element therefore saw an empty frame
stack and refused with "can only be used in a function"; worse, the name
it should have bound stayed unset.

A subshell in bash is a *fork*: it inherits the whole variable-context
stack, so `local` there works and its binding disappears with the
subshell, exactly as it would in the function proper.

```sh
f1() { ( local q=1; echo "q=$q" ); echo "after=${q-unset}"; }
f1              # bash: "q=1" / "after=unset"
                # osh:  "local: can only be used in a function" / "q=" / "after=unset"

g=outer
f3() { ( local g=inner; echo "in=$g" ); echo "out=$g"; }
f3              # bash: "in=inner" / "out=outer".  osh: "in=outer"

f4() { echo "sub=$( local s=1; echo "s=$s" )"; }
f4              # bash: "sub=s=1".  osh: refuses, "sub=s="

f7() { local o=1; ( local i=2; echo "o=$o i=$i" ); }
f7              # bash: "o=1 i=2".  osh: refuses, "o=1 i="
```

Measured with `target/dvscratch/px85.sh` — every section diverged. Also
affected pipeline elements (`f5() { local p=1 | cat; }`) and background
subshells.

**Fix.** `clone_for_subshell` now clones `local_frames` (and
`local_opt_saves`, which is kept in lockstep with it). The frames are a
copy, so bindings made inside the subshell are not visible after it —
that falls out of the subshell's own state being discarded. Nothing pops
an inherited frame: `call_function` pops only what it pushed. This also
makes `temp_shadow`'s `local_depth`, which was already cloned, mean the
same thing on both sides of the fork.

The model it implies is confirmed by `local -p`: in
`f() { local a=1; ( local b=2; local -p ); }` bash lists *both* names,
i.e. the subshell's `local` went into the inherited frame rather than a
new one. Pinned by `userspace/oils/tests/corpus/subshell-local.sh`.
