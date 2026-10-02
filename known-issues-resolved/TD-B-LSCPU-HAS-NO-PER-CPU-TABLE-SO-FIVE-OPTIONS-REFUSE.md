## TD-B-LSCPU-HAS-NO-PER-CPU-TABLE-SO-FIVE-OPTIONS-REFUSE — 2026-09-15 — ✅ FIXED 2026-09-26 (lane B)

**Fixed by replacing the program.** `userspace/lscpu` is now a port of
util-linux 2.39.3's `lscpu` (as Ubuntu 24.04 ships it, design-decisions
§1038), built on the `smartcols` crate: `-e`, `-p`, `-C`, `--hex`,
`--online`, `--offline` and `--sysroot` all do what upstream's do, per CPU,
from each CPU's own `topology/`, `cache/` and `cpufreq/` files. Checked by
`scripts/lscpu-diff.sh` against the real `lscpu` on util-linux's own
snapshots of nineteen machines, trees of its own, and WSL itself. The
reversed-range underflow noted below went with the old parser; the port's
CPU-list parser is upstream's `cpulist_parse`, which refuses `5-2`. What
follows is the entry as it was.


**In short:** `lscpu -e`, `-p`, `--hex`, `--online` and `--offline` now refuse
with exit 1 instead of printing the ordinary CPU summary and exiting 0. All
five need a **per-CPU topology table** this lscpu does not build, and refusing
is the honest interim, not the destination.

**Why they were worse than missing.** Each was in the parser, in `--help`, and
read by nothing. `lscpu -p` is the one that bites: a script asking for a
parseable table got the human-readable one *and exit 0*, so it parsed a heading
as data rather than failing.

**What implementing them needs.** `collect_cpu_info` builds one aggregate
`CpuInfo`. `-e` and `-p` need a row per CPU — cpu, core, socket, node, and the
four cache columns — which means reading
`/sys/devices/system/cpu/cpuN/topology/*` per CPU rather than the summary
files. `--online`/`--offline` then filter those rows, and `--hex` changes how
the masks in them are printed. **Doing it half-way would be worse than the
refusal**: a per-CPU table with invented topology is a fabrication of exactly
the kind this tree has been clearing out today, and a plausible wrong row is
harder to notice than a missing one.

**Measured against util-linux 2.40 before refusing.** GNU's message for
`--online`/`--offline` without `-e`/`-p` is copied verbatim, including its
naming of `--all` whichever of the three was given, and it exits 1 with an
empty stdout. Worth recording *how* that was measured: the first attempt used
`wsl -- bash -c 'lscpu --offline >/dev/null 2>&1; echo $?'` and reported exit
**0**, because `wsl.exe`'s inline argument handling mangles `>` and `$`. The
heredoc form (`bash -s <<'EOF'`) gives 1, which is the real answer. That hazard
is documented in this repo and I used the broken form anyway.

**Also in this file, unfixed:** `parse_cpu_range` computes `count += e - s + 1`
with no check that `e >= s`. `/sys/devices/system/cpu/online` holding a
reversed range (`5-2`) would underflow — a panic in debug, a wrap in release.
It is not fixed here because `lscpu/Cargo.toml` has **no `[lints]` section at
all**, so `arithmetic_side_effects` is off across the crate and this is one
instance of a crate-wide gap rather than a lone bug. See
`TD-B-USERSPACE-CRATES-DO-NOT-INHERIT-THE-WORKSPACE-LINTS`.

**Where it lives:** `userspace/lscpu/src/main.rs` — the refusal arms in the
option loop, `refuse_unimplemented`, and `parse_cpu_range`.
