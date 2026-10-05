### TD-OILS-A-COMPOUND-LITERAL-WAS-TRANSFORMED-A-SECOND-TIME-WHERE-STEP-ONE-HAD-ALREADY-MADE-THE-VARIABLE. `readonly g=(1 2)` under a merely-dying global chain put the elements on the frame's reference — 2026-08-12 — FIXED 2026-08-12

**Where:** `userspace/oils/src/interp.rs`, [`Shell::global_bind_names`]. The two
commands a compound operand decomposes into were given two different names to
bind wherever the global side of the lookup came back empty: the builtin road
took the *global-only* walk (`find_global_variable_last_nameref`,
declare.def:735) and the compound-literal road took the **live** chain's last
cell (`nameref_transform_name` → `find_variable_last_nameref`,
variables.c:2333). That is what the C says, but the literal only ever *reaches*
that line where `find_global_variable (name)` is empty (`do_compound_assignment`,
subst.c:3494) — and where the global-only walk answered, step 1 has already made
a variable at the name it answered with (`bind_global_variable (name, NULL,
ASS_FORCE)`, declare.def:792, or a kind letter's fresh array). So it is no
longer empty, and the literal simply follows the builtin there.

The roads part only where the global-only walk answered **nothing** — among
globals, a cycle — because then step 1 made nothing either and the question is
live again.

**Reproduce.** A chain that merely dies, rather than closing:

```sh
( declare -n g=z
  f() { local -n g=w; readonly g=(1 2); }; f; declare -p g z w )
# bash: declare -a z=([0]="1" [1]="2")  and the `-r` alone on w
# osh (before): the elements on w too
```

**The fix.** `global_bind_names`' dead-chain arm stopped asking which road it
was on: where `global_chain_path` answers, that path is the answer for both
commands; only where it answers nothing does the live chain's last cell (or, for
a kind letter, the name as written) come back into it. One arm in place of a
`(compound, path)` pair.

**Tests.** Two new sections in corpus
`a-global-scope-declaration-whose-global-chain-dies-binds-the-live-chains-last-reference.sh`
(`readonly`/`export`/`declare -g` over a dying chain, and the cycle that still
parts the two) and the matching block in the unit test of the same name. The
240-shape probe matrix (`/tmp/kind_matrix.sh`) went 210 → 214.
