## TD-C-ALLOW-DEAD-CODE-IS-HIDING-WHOLE-UNWIRED-MODULES

**Date:** 2026-09-08. **Lane:** C.
**Where:** `apps/procexplorer/src/features.rs` (2 540 lines),
`apps/sysinfo/src/hwquery.rs` (2 163), `apps/settings/src/remote.rs` (1 593).
The pattern is wider: 23 files under `gui/` and `apps/` carry a crate- or
module-level `#![allow(dead_code)]`.

**In short:** three substantial modules are compiled, declared, and used by
nothing — and the compiler already knew. Each begins with
`#![allow(dead_code)]`, which switches off the one check that would have said
so. Remove the line from `procexplorer/src/features.rs` and the build
immediately reports **56** "never used / never constructed" warnings. The lint
was not wrong; it was turned off.

**Census 2026-09-13, and five of them are now gone.**

The "23 files" above is the count of files carrying *any* crate- or
module-level allow, not the dead-code ones. Measured properly — by disabling
each inner `#![allow(...dead_code...)]`, type-checking the crate with
`--all-targets`, counting what appears, and restoring — there are **17**, and
they divide sharply:

| | files | items hidden |
|---|---|---|
| masking nothing | **5** | 0 |
| masking something | 12 | **198** |

**The five that masked nothing are removed**, because an allow over nothing is
pure cost: it silences the lint for everything added to that file afterwards
and buys not one warning today. `installer/src/lib.rs`, `guitk/src/scaling.rs`
and `guitk/src/signal.rs` lost the line outright; `rssreader` and `torrent`
kept the rest of theirs — `clippy::too_many_arguments` in one,
`arithmetic_side_effects` and `indexing_slicing` in the other — because
removing an unrelated allow is a different change and would have been smuggled
in under this one. All four crates: clippy clean, tests pass.

**Where the 198 are**, and the shape of the list is the point:

| items | file |
|---|---|
| 68 | `apps/imageviewer/src/video.rs` |
| 50 | `apps/settings/src/remote.rs` |
| 39 | `apps/procexplorer/src/features.rs` |
| 16 | `apps/explorer/src/fileops.rs` |
| 8 | `apps/explorer/src/columns.rs` |
| 5 each | `apps/match3`, `apps/pinball` |
| 3 | `gui/toolkit/src/grid.rs` |
| 1 each | `explorer/thumbs.rs`, `screenrecorder`, `soundrecorder`, `guitk/pathbar.rs` |

The top three are 157 of the 198 and are C-Q17's subject exactly — the video
player, the remote-settings page and the window picker are three of the five
features that entry asks the operator about. They are not this entry's to
remove. `guitk/src/grid.rs` is the `GridView` nothing constructs, found
separately the same day.

**A correction to this lane's own first count.** "47 inner allows" was reported
here an hour before the sweep ran, from `grep -c "#!\[allow"` — which counts
every crate-level allow of any lint. The real figure is 17. Counting a *shape*
that resembles the thing rather than the thing itself is the error this file
records more often than any other, and it does not stop being that when the
person making it is the one writing the entry.

**Verified, not inferred.** All three crates are pure binaries — no `lib.rs`,
no `[lib]` — so a public item that nothing references really is unreachable.
That check matters: `apps/installer/src/grub.rs` looked identical to a
reachability sweep and is **not** the same case, because `installer` *does*
have a lib target, which makes its `pub` items API rather than dead code. It
is excluded from this entry for that reason.

| module | what it holds | note |
|---|---|---|
| `procexplorer/src/features.rs` | `WindowPicker` (crosshair "click a window to find its process"), `ProcessAction`, `BlockingInfo`/`BlockingLink` (which process is blocking which) | `mod features;` is declared in `main.rs` and never `use`d. |
| `sysinfo/src/hwquery.rs` | hardware enumeration | same shape. |
| `settings/src/remote.rs` | remote-settings surface | same shape. |

**Why this is worth its own entry rather than three deletions.** The
suppression is the bug. Today's sweeps found roughly 21 600 lines of
unreachable code across the shell and the apps, and this is the mechanism that
let a good part of it accumulate unnoticed: a module is written, the lint
complains because nothing calls it yet, the lint is silenced *to get a clean
build*, and the "yet" never arrives. Deleting these three without removing the
suppression pattern leaves the trap armed for the next module.

**Proper fix, in order:**

1. **Remove `#![allow(dead_code)]` from these three files** and read what the
   compiler says. That is a one-line change per file that produces an exact,
   trustworthy inventory — far better than any sweep I can write, because it
   is the compiler's own reachability analysis rather than an approximation of
   it.
2. For each item it names: wire it or delete it. `WindowPicker` and the
   blocking analysis are real features a process explorer should have, so
   these are probably wirings rather than deletions — unlike the settings
   panels deleted today, which were duplicates of a working app.
3. **Then look at the other 20 files.** Some uses of the attribute are
   legitimate (a struct field kept for an ABI, a variant reserved by a spec);
   each needs a reason next to it, and the ones without a reason are this bug
   again.

**A caution learned while writing this.** My first pass concluded these
modules were unreachable and the compiler disagreed — zero warnings. The
sweep was right and the *compiler* was silenced, but I only found that out by
checking why they disagreed instead of trusting my own tool. A grep for the
suppression had already run and shown eight hits, all in `alarmclock`, because
it was piped through `head -8`. The answer was in the truncated part.
