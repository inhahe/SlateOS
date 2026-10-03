## TD-C-FIVE-APP-MODULES-ARE-COMPILED-TESTED-AND-UNREACHABLE

**Date:** 2026-09-13. **Lane:** C.
**Where:** `apps/procexplorer/src/features.rs` — 85 KB, 55 public items, 17
public types, **37 passing tests**.

**It is not one module; it is five.** Sweeping every app crate for a `mod x;`
whose public types are never named in any other file of the crate:

| module | types | size | tests | what it is |
|---|---|---|---|---|
| `procexplorer/features.rs` | 17 | 83 KB | 37 | window picker, deadlock analyser, affinity, priority, memory map, env viewer |
| `imageviewer/video.rs` | 13 | 77 KB | 63 | video playback |
| `sysinfo/hwquery.rs` | 5 | 73 KB | 35 | hardware query |
| `installer/grub.rs` | 9 | 48 KB | 44 | GRUB configuration — in the *installer* |
| `settings/remote.rs` | 10 | 46 KB | 35 | remote settings |

**327 KB and 214 passing tests, reachable by nobody.** Verified three ways for
each: the module is declared with `mod x;`, none of its public types is named
in any sibling file, and there is no glob import that could be hiding the use.
Corroborating evidence arrived independently — clippy reports dead-code
warnings inside `sysinfo/hwquery.rs`, which is what an unreachable module looks
like from the compiler's side.

`installer/grub.rs` is the one to look at first if only one is looked at: an
installer that cannot configure a bootloader is a different severity of problem
from a process explorer missing a window picker.

**A note on how easily this hides.** Earlier the same night I converted a
hairline separator in `settings/remote.rs` — carefully, with a git-archaeology
step to recover its original palette role. That work was real and it was spent
on a file no user can reach. Nothing in the tree told me; the tests passed
before and after.

**In short:** the process explorer has a second module of features — a window
picker that identifies a process by clicking its window, a blocking analyser
that traces what a process is waiting on and detects deadlocks, CPU affinity
control, priority control, a memory-map viewer and an environment browser. It
is compiled, it is tested, and **no part of the application calls any of it**.
A user cannot reach a single one of those features.

**How sure.** `mod features;` is declared in `main.rs` and nothing else
references it. Of the module's seventeen public types — `WindowPicker`,
`BlockingAnalyzer`, `AffinityMask`, `PrioritySelector`, `EnvViewer`,
`MemoryMap` and the rest — **not one is named anywhere in `main.rs`**. The only
names in common are `new`, `render`, `all`, `color`, `label`, `select`, `size`
and `hover`, which are methods on unrelated types.

**Why nothing noticed.** It has 37 tests and they all pass, because they test
the module directly. This is `known-issues.md` lesson 47 at module scale, and
the irony is sharp: `procexplorer`'s own `tick_interval` doc cites that lesson
by name — *"a system monitor that monitors nothing, with every one of its
tests still passing"* — while this module sat beside it unreachable.

**How it was found.** Not by reading, and not by the tests. A colour-literal
count came back at 27 for the crate when `main.rs` had 1; the other 26 were in
a file I had never opened, and opening it to convert its colours is what
exposed that nothing calls it.

**What the fix is.** A decision first, then work: either wire the features into
the UI (they appear to be complete — the analyser has a deadlock detector and
the affinity control has a mask editor), or delete the module. Both are
defensible; shipping 85 KB of tested, unreachable features is not. This is
worth the operator's input, because "delete a working deadlock detector" and
"add six features to the process explorer" are very different amounts of work
and only one of them is a bug fix.

**Its 26 colour literals are deliberately left alone** until that is settled.
Converting the colours of a module nobody can see would be the most literal
possible instance of the thing this file exists to prevent.
