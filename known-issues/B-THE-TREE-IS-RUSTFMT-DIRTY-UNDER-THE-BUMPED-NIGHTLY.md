## `B-THE-TREE-IS-RUSTFMT-DIRTY-UNDER-THE-BUMPED-NIGHTLY` (lane B, 2026-08-26) — **open**, tech debt

**In short:** After the nightly toolchain bump, a large fraction of the tree no
longer matches what `rustfmt` would produce. This is not anyone's uncommitted
mistake — the formatter's own defaults changed. The practical cost is that any
commit which touches a stale file and runs `cargo fmt` drags in reformatting of
lines it did not mean to change, which buries the real diff.

**Measured, not estimated:** 26 of 40 randomly sampled files under `userspace/**`
are rustfmt-dirty at HEAD — about 65%. The deltas are overwhelmingly trailing
commas and line re-wrapping; a whitespace-normalised comparison of the 20 files
reformatted in `fd78fa67b` found no non-whitespace change other than trailing
commas, so the reformat is semantics-free.

**Why it has not simply been fixed.** A tree-wide `cargo fmt` is the obvious
answer and is the wrong move while three lanes are live: it would rewrite files in
all three ownership zones at once and conflict with every in-flight branch. It is
a coordination problem, not a technical one.

**Workaround in use.** When a task must touch stale files, split it in two: one
commit that is *only* the reformat of the files being touched, then the semantic
change on top. `fd78fa67b` followed by `045f603e1` is the worked example.

**Proper fix.** One tree-wide `cargo fmt` run, agreed between the three lanes,
landed on `main` at a moment when all three have merged up and none has
substantial uncommitted work — then all three merge it down before resuming. Worth
raising with the operator to pick the moment; until then the split-commit
workaround is adequate and cheap.

**Where it bites:** the whole tree; most visibly `userspace/**`. Check with
`rustfmt +nightly-x86_64-pc-windows-gnu --edition 2024 --check <files>`.
