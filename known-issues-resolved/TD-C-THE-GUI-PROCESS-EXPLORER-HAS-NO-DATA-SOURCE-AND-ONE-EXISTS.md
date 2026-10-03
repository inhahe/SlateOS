## TD-C-THE-GUI-PROCESS-EXPLORER-HAS-NO-DATA-SOURCE-AND-ONE-EXISTS -- FIXED 2026-09-13

**Fixed 2026-09-13.** `ProcessExplorerState::refresh` reads the real `/proc`
through the shared `procinfo` crate -- the one this lane asked lane B to
factor out, so that using it rather than writing a parser here is the whole
point of its existing.

Four judgements are in the code rather than here:

- **A failure is not an error to show.** `/proc` is absent on a developer
  host and on a machine where it has not been mounted yet. The refresh keeps
  the list it had rather than emptying the window on a boot-order accident.
- **A process that vanishes between the listing and the read is skipped**,
  not fatal. Racing with the thing being measured is what a process list is.
- **`cpu_percent` stays zero.** A percentage needs two samples and a refresh
  is one; `update_histories` is where a rate belongs. Inventing a number
  there would be the same mistake this entry was written about.
- **`D` maps to Sleeping** rather than gaining a variant: to a user,
  uninterruptible sleep is a process that is not running and cannot be
  stopped, which is what Sleeping already means in this window.

The test runs against a fixture directory via `ProcFs::at`, not the
machine's own `/proc` -- a real process list changes between the two lines of
an assertion. It asserts the mapping this crate actually wrote: the state
letter, both page-to-byte conversions, ticks to milliseconds, and that a
directory with no `stat` is skipped rather than fatal.

**Date:** 2026-09-08. **Lane:** C.
**Where:** `apps/procexplorer/src/main.rs` — `ProcessExplorer::refresh`.

**In short:** the graphical process explorer does not show you the processes
running on the machine. It shows a fixed list of made-up ones. Everything else
about it works — sorting, filtering, the tabs, the graphs — but the step that
would ask the system what is actually running was never written, and the
placeholder that stands in its place says so in a comment nobody sees.

**The evidence, in its own words:**

```rust
pub fn refresh(&mut self) {
    // Placeholder: in production, call kernel syscalls here:
    //   - sys_process_list() -> Vec<ProcessInfo>
    //   ...
    // For now, the data vectors are populated externally or via
    // `load_demo_data()` for development/testing.
```

**And the data source exists.** `userspace/htop` reads `/proc/<pid>/` for the
same facts — per-process stats, `/proc/stat`, `/proc/meminfo` — and has done
for some time. So this is not blocked on the kernel; the GUI explorer simply
never learned to read what the terminal one already reads.

**Why this outranks wiring `features.rs`.** That module (2,539 lines, six
finished widgets) is unreachable, and connecting it looks like the obvious next
job. It is not: pointed at `load_demo_data()`, an affinity editor and a memory
map would render invented numbers with the same confidence as real ones. **A
richer display of false data is worse than a plain one**, because it invites
the user to act on it — the affinity and priority widgets exist precisely to
*change* things.

**The proper fix, and the part that needs a decision.** Read `/proc`. The
question is *whose* reader:

| | |
|---|---|
| Copy htop's into `apps/procexplorer` | A third copy of `/proc` parsing in the tree, free to drift from htop's — and the two would then disagree about what a process's state letter means. |
| Extract htop's readers into a shared crate | The right shape, but `userspace/**` is **lane B's**, so this needs a `requests/` note rather than an edit. |
| A `sysinfo`-style crate in lane C | Avoids the lane boundary and creates the second copy anyway. |

Extracting is the one to ask for. Filed as a request when this is picked up;
recorded here so the finding is not lost in the meantime.

**Do not wire `features.rs` first.** It would make the wrong data more
convincing.
