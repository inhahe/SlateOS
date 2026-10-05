### TD-OILS-A-NAMEREF-CHAIN-THAT-OUTRUNS-THE-LINK-LIMIT-IS-TAKEN-FOR-ONE-THAT-CLOSED. A walk that gave up on the eighth link is answered like one that closed on itself — 2026-08-11 — ✅ FIXED 2026-08-12

**Where:** `userspace/oils/src/interp.rs`, [`Shell::walk_ref_name`] and its
callers. `find_variable_nameref`'s cycle escape (variables.c:2080-2110) fires
only on `v == orig || v == oldv`. A chain read from a name *in front of* the
loop — `g` → `z` → `y` → `z` — never returns to `g`, so the test never fires:
the walk spins to `NAMEREF_MAX` (8, variables.h:172) and gives up **quietly**, a
third outcome distinct from both "found" and "cycle detected". osh conflates it
with the detected cycle's NULL.

```sh
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local g=5; declare -g g=1; declare -p g; }; f )
# bash: declare -- g     (the frame's own binding is *emptied*)
# osh : declare -- g="5"
```

Affected: 8 shapes of the 56-shape second probe matrix (`/tmp/cyc_probe2.sh`,
48/56), three of which also empty the frame's binding as above.

**Fixed** by making the *global* walk tri-state and modelling what bash does
with each answer, rather than by changing `walk_ref_name`:

* `Shell::global_chain_target` became `Shell::global_chain_end`, answering
  `Reached` / `Ended` / `GaveUp` — "the chain arrived at a variable", "it ran
  out on a name nothing answers to", and "it closed on itself or outran the
  limit". Only `Ended` shares a tail with the live chain (the global walk can
  only get there by re-entering a name a frame shadows), so only there does one
  name serve both halves of the decomposition.
* New `Shell::live_last_nameref` mirrors `find_variable_last_nameref (name, 0)`
  (variables.c:2117) — the **live** chain's last reference *cell*, which is what
  `bind_variable_internal` (variables.c:3098) falls back on when the global walk
  answered nothing. Its three outcomes (`Here` / `Cell` / `Nothing`) are the
  three things bash then does.
* New `Step1Bind` on `GlobalBind` carries step 1's own binding where the literal
  after it binds a different name: `EmptyGlobal` (a fresh empty global at the
  cell) or `EmptyLive` (the live binding written with a NULL value — bash's
  `assign_value:` path, which empties a frame-local **in place**).
* The literal's own transform reads `ASS_CHKLOCAL` (`nameref_transform_name`,
  variables.c:2333), so `readonly`/`export` — which carry `W_CHKLOCAL`
  standingly (execute_cmd.c:4221), unlike any letter of `declare` — walk the
  live chain and land where step 1 did instead of parting from it.

The key misreading that had defeated two earlier attempts: step 1 of
`declare -g g=(1 2)` is a bare `declare -g g`, **not** `declare -ga g`.
`fix_assignment_words` (execute_cmd.c:4180-4249) sets `W_ASSIGNARRAY` only from
a preceding `-a` *option word* and never from the literal's shape.

`/tmp/cyc_probe2.sh` 48/56 → 55/56 (the one remaining row is the bash segfault
below). `/tmp/cyc_probe.sh` 27/27 and `/tmp/kind_matrix.sh` 233/240 unchanged.
Corpus:
`a-nameref-chain-that-outruns-the-link-limit-parts-the-two-halves-a-closed-one-does-not.sh`;
test `a_nameref_chain_that_outruns_the_link_limit_parts_the_two_halves`.

**Still unmodelled:** a nameref cell holding an *element* reference
(`declare -n g='a[0]'`) takes bash's `assign_array_element` road out of
`bind_variable_internal`, which osh does not follow; no probe row reaches it.

**Do not imitate:** bash 5.2.37 **segfaults** (rc 139) on

```sh
declare -n g=z; declare -n z=y; declare -n y=z
f() { local -a g=(9); declare -g g=1; }; f
```

reproducibly. osh answers sanely; that row is deliberately not pinned.

**How it was found:** the two nameref-cycle probe matrices written while fixing
TD-OILS-A-GLOBAL-SCOPE-DECLARATION-BOUND-THE-NAME-IT-WAS-WRITTEN-WITH-WHEN-ITS-GLOBAL-CHAIN-LED-NOWHERE.
The first (`/tmp/cyc_probe.sh`) and the 240-shape kind-letter matrix
(`/tmp/kind_matrix.sh`) no longer see it.
