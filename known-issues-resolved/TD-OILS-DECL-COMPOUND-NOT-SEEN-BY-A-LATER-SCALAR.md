### TD-OILS-DECL-COMPOUND-NOT-SEEN-BY-A-LATER-SCALAR. A compound operand bound too late for a later scalar operand's value — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the word-expansion loop in
`Shell::exec_simple` (`for (wi, w) in sc.words.iter().enumerate()`), which
expanded **every** word into `argv` before `Shell::exec_declare_compounds` bound
any compound operand.

**What:** a declaration builtin's compound operand is bound during the
word-expansion pass, which osh already models (see `DeclCompounds`) — but bash
does it *at the operand's position in the word list*, so a word written after it
is expanded with it already bound. osh binds them all after the whole list has
expanded, so a later **scalar** operand's value sees nothing:

```sh
declare -a r=(x y) s=${r[1]}     # bash: s=y      osh: s=
declare -A A=([k]=v) s=${A[k]}   # bash: s=v      osh: s=
declare a=1 r=(x y) s=${r[0]}$a  # bash: s=x      osh: s=
```

A later **compound** operand already sees it (`declare -a r=(x y) t=(${r[1]})`
is `y` in both), because those are bound in operand order — so the gap is only
between a compound and a scalar written after it.

**Second symptom, same cause — a compound is lost when a *later* word fails to
expand.** Because bash has already performed it, the binding stands:

```sh
declare r=(a) x=$((1/0))   # bash: rc 1, r is (a)   osh: rc 1, r unset
```

osh expands the whole list first, so the failure comes before any compound binds
and `r` is never created. Two compounds already behave correctly — `declare
r4=(a) r5=($((1/0)))` leaves `r4` bound and `r5` unset in both — which is the
same "compounds are ordered among themselves, but not against the scalars" gap.

**The measured rule (bash 5.2.37), which is entirely consistent:** a compound
assignment is *performed* as the expansion pass reaches it; a scalar one is only
*expanded* there, and the assignment itself is deferred to the builtin. Hence:

- a scalar after a compound sees it (`s=y` above), and so does a compound after
  a compound;
- a compound after a *scalar* does **not** (`declare a=1 b=($a)` leaves `b`
  empty), nor does a scalar after a scalar (`declare a=1 b=$a` → `b` empty),
  nor a scalar before a compound (`declare s=${r[1]} r=(x y)` → `s` empty);
- a scalar's value is expanded at its own position but assigned later, so
  `a=OLD; declare a=NEW s=$a` leaves `s` as `OLD` — while the builtin then
  applies its scalars in order, so `declare c=x c+=y` is `xy`;
- the compound's binding stands even when a later operand is refused
  (`readonly ro=1; declare -a r=(x y) ro=2` fails but leaves `r` bound), which
  osh already matches.

`local` behaves identically. osh matched every one of these except the first
group.

**Fixed 2026-08-04.** The two are now **interleaved**: the word-expansion loop
in `Shell::exec_simple` calls the new `Shell::bind_compounds_before` before
expanding the word at index `wi`, which binds every compound operand written
ahead of that word, and once more with `usize::MAX` after the loop for the ones
written after every word. `Shell::exec_declare_compounds` and
`Shell::declare_compounds_scoped` are therefore called **once per operand**,
taking `flag_limit: usize, positions: &[usize]` in place of the whole
`DeclWords` — both are known by the time the operand is reached, because every
flag necessarily precedes the first compound one (getopt stops at the first
non-option word, which is why `declare q=(1) -x` refuses `-x` as a *name*), so
`argv[1..flag_limit]` is exactly what had expanded when the first operand was
reached and `positions[k]` is just `argv.len()` at the moment operand `k` was.
The per-operand state lives in the new `CompoundBinding`, and the flag prescan
being *pure* is what makes re-running it per operand safe: the first operand's
`DeclCompounds` speaks for all of them, and the rest only extend its `bound`.

`Shell::in_declare_global_scope` needed no rework after all: `-g` does not reach
value expansion (`local x=L; declare -g a=(1) y=$x` binds `y` to `L` in bash),
so it still wraps each *binding* rather than the word loop, and entering and
leaving it per operand is the same as per half — leaving parks what the operand
made of the global back in the frame that shadows it, which is where the next
enter reads it from.

Two smaller things fell out. The three post-loop expansion-failure checks became
`Shell::command_word_expansion_failure`, since a binding has to make the same
check before it — a word *before* an operand failing means the operand is never
reached and never binds, which is the entry's second symptom. And `word_starts`
went away entirely: `CompoundBinding::positions` records the same numbers, so
the `set -x`/`-p` splice reads them instead and no per-word bookkeeping is
needed for commands that have no compound operands at all.

**Pinned by** `tests/corpus/a-declaration-builtins-compound-operand-binds-where-it-stands.sh`
(17 sections) and the `a_declaration_builtins_compound_operand_binds_where_it_stands`
unit test in `src/interp.rs`. The corpus case has to keep the failure cases'
bindings in *this* shell, so it reads their diagnostics from a scratch file
rather than down a `2>&1 |` pipeline, and wraps the word-expansion failures in
an `eval` because such an error abandons the rest of the parse unit it happened
in — which would otherwise swallow the very `declare -p` that asks what bound.
