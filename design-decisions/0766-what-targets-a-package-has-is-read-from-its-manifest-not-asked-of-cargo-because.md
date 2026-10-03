## 766. What targets a package has is read from its manifest, not asked of cargo — because the half that owns a cargo is not always the half that runs

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** `scripts/coreutils-check.sh` compiles a crate twice — once for
Windows, once for Linux through WSL — because each build hides half the code
from the other. It was told to compile "the library and the programs" of
whatever crate it was pointed at. That works until you point it at a crate that
has no library, which is what every ordinary program is: it died on
`-p shell` with "no library targets found in package 'shell'". So it now has to
find out, per crate, whether there is a library to compile. The decision is
where that answer comes from: ask cargo, which knows for certain, or read the
crate's `Cargo.toml` and work it out. It reads the manifest.

**Why the question is not symmetric.** `--lib` and `--bins` fail in opposite
directions, and only one of them is loud:

| Flag | Package has none of that kind | |
|---|---|---|
| `--lib` | cargo errors and nothing is built | loud — you find out immediately |
| `--bins` | cargo accepts it and builds nothing | **silent** — the run ends "result: clean" |

That asymmetry rules out the one-line fix of dropping `--lib`. A crate with
neither a library nor a binary would then be compiled with `--bins`, compile
nothing, and be reported clean — which is precisely the defect this script was
written to close, reappearing inside the script. Such a package is now refused
by name (exit 64), not scoped down to nothing.

**The two ways to get the answer.**

| | Ask cargo | Read the manifest |
|---|---|---|
| *What changes:* | the answer is whatever cargo itself would do, by construction | the answer is derived from cargo's documented rules, and a run needs no cargo on the host |
| Cost | a workspace load per query: measured 2026-09-04 against this tree's 2,950 members, `cargo read-manifest` 8–9 s per package, `cargo metadata --no-deps --offline` 13.8 s | one `git ls-files` plus one `grep`, ~3 s worst case and a dozen `stat`s in the ordinary case |
| Where it can run | only where a cargo exists | anywhere |
| Failure mode | none | over-count on `autolib = false` / `autobins = false`, which is the loud direction |

**Why the manifest won, and it is not mainly the seconds.** It is *where* the
seconds could be spent. This script exists to compile the Linux half on a
Windows host, and it explicitly declines the host half when there is no cargo
there — so "the side that has a cargo" and "the side that is doing the work"
are not the same side in the configuration the script was written for. Scope is
computed once, on the near side, and handed to both halves as arguments; a
mechanism that needed a local cargo could not do that without either two code
paths or a second workspace load inside WSL.

**Why this is not a heuristic, which is the objection it invites.** A cargo
library target exists if and only if the manifest declares `[lib]` or the file
`src/lib.rs` is present. There is no third spelling: a library whose source
lives elsewhere must say so with `[lib] path = ...`, which is a `[lib]` section.
So the test is a complete case analysis of cargo's rules, not a guess at them.
The `autolib`/`autobins` opt-outs make it over-count — it would pass `--lib` for
a crate that has suppressed its `src/lib.rs` — and over-counting lands in the
loud column above: cargo says the target is missing and the script fails with
it. Under-counting, the silent direction, cannot happen.

**The locator is the part that could have gone wrong.** Finding a package's
manifest from its name means, in the worst case, reading 2,950 manifests. The
first draft ran one `awk` per file, which on Windows is 2,950 process spawns and
most of a minute — a locator that costs more than the build it is scoping is a
locator that gets deleted. It now guesses the conventional directory first
(a dozen `stat`s), narrows with a single `grep -l` over the whole list, and
verifies the survivors' `[package] name` with `awk`. The verification is not
optional: `[[bin]] name = "shell"` is a legal line in a manifest for a different
crate, and in a tree whose coreutils crate builds ~180 named binaries, matching
on the line alone would attribute one crate's targets to another.

**What it does not decide.** It does not change *what* is compiled for a package
that has both kinds of target — that is still the library and the binaries, and
still not the integration tests, for the reasons the script's header gives. It
also does not settle which packages the pre-push gate should check; that is step
4 of `TD-B-THE-UNIX-HALF-OF-COREUTILS-IS-NEITHER-LINTED-NOR-TESTED-BY-DEFAULT`,
which this change unblocks by making a bin-only crate measurable at all.
