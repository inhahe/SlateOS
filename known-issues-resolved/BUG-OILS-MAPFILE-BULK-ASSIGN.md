### BUG-OILS-MAPFILE-BULK-ASSIGN. `mapfile` assigned the array only after the whole read, and `-O` wrongly cleared it first — 2026-07-27 — ✅ RESOLVED 2026-07-27

**How it was found.** The first run of the new differential harness
(`scripts/osh-bash-diff.py`, see below) against a 25-case corpus: 24 matched,
the `mapfile-callback` case did not. That case had been written from the bash
manual's wording, not from a hand-probe, which is exactly the kind of gap the
harness exists to close.

**Symptom.** Two divergences, both verified against bash 5.x
(`C:/Program Files/Git/usr/bin/bash.exe`) before and after the fix:

| Case | bash | osh (before) |
|---|---|---|
| `cb() { echo "have=$((${#arr[@]} - 1))"; }; mapfile -t -C cb -c 1 arr < f` (f = `a b c`) | `have=-1`, `have=0`, `have=1` — each callback sees the elements read *before* it | `have=-1` three times — the array was still empty in every callback |
| `arr=(x y z w v); mapfile -t -O 1 arr < f; echo "${arr[*]}"` | `x a b c v` — `-O` overwrites in place | `a b c` — the array was cleared first, like a plain `mapfile` |

**Root cause.** `builtin_mapfile` buffered every line into a local `Vec` and did
one bulk `arrays.insert()` at the end. That made the in-progress array invisible
to the `-C` callback (which is arbitrary shell code running in the current
shell, so it *can* read it), and it conflated "start from index `origin`" with
"replace the whole array". bash's rule: the callback is evaluated after the line
is read but **before** that element is assigned, and `-O` suppresses the
array-clearing entirely — elements outside the written range survive, and
writing past the end leaves an index gap rather than compacting
(`arr=(x)` + `-O 2` → indices `0 2 3 4`).

**Fix.** Assign each element inside the read loop, immediately *after* its
callback fires, re-looking up the array through
`self.arrays.entry(array.clone())` every iteration — the callback may have
removed or replaced the binding, so a reference held across the call could
dangle. A new `origin_given` flag makes the pre-read reset `or_default()`
(keep existing elements) instead of `insert(BTreeMap::new())` (clear).

**Tests.** `mapfile_callback_sees_earlier_elements`,
`mapfile_origin_overwrites_without_clearing`. Both failed before the change.
Suite: 727 + 15 pass, `cargo clippy -p oils --all-targets` clean; corpus 25/25.
